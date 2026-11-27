// Dev/CI runtime setup: python-build-standalone + baked pip deps.
// Usage: bun scripts/setup.ts [--root <dir>] [--triple <t>] [--force] [--ship] [--update-lock]
// Root (default: parent of scripts/, i.e. apps/desktop; env YTDLPD_ROOT) holds runtime.lock.json and runtime/.
import { createHash } from "node:crypto";
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { basename, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";

const args = process.argv.slice(2);
const flag = (n: string) => args.includes(`--${n}`);
const opt = (n: string) => (args.includes(`--${n}`) ? args[args.indexOf(`--${n}`) + 1] : undefined);

const root = resolve(opt("root") ?? process.env.YTDLPD_ROOT ?? join(import.meta.dir, ".."));
const PY_VERSION = "3.14.10";
const REPO = "astral-sh/python-build-standalone";
const BAKED = ["certifi", "requests", "urllib3", "websockets", "mutagen", "brotli", "pycryptodomex"];

function hostTriple(): string {
  const arch = process.arch === "arm64" ? "aarch64" : "x86_64";
  if (process.platform === "win32") return `${arch}-pc-windows-msvc`;
  if (process.platform === "darwin") return `${arch}-apple-darwin`;
  return `${arch}-unknown-linux-gnu`;
}
const TRIPLES = [
  "x86_64-pc-windows-msvc",
  "aarch64-pc-windows-msvc",
  "x86_64-apple-darwin",
  "aarch64-apple-darwin",
  "x86_64-unknown-linux-gnu",
  "aarch64-unknown-linux-gnu",
];
const triple = opt("triple") ?? hostTriple();
if (!TRIPLES.includes(triple)) throw new Error(`unsupported triple ${triple}`);
const isWin = triple.includes("windows");

const lockPath = join(root, "runtime.lock.json");
const lock: any = existsSync(lockPath) ? JSON.parse(readFileSync(lockPath, "utf8")) : {};
const gh = async (url: string) => {
  const r = await fetch(url, { headers: { "User-Agent": "ytdlpd-setup" } });
  if (!r.ok) throw new Error(`GET ${url}: ${r.status}`);
  return r;
};
const sha256 = (b: Uint8Array) => createHash("sha256").update(b).digest("hex");
const run = (cmd: string, a: string[], cwd?: string) => {
  const r = spawnSync(cmd, a, { cwd, stdio: "inherit" });
  if (r.status !== 0) throw new Error(`${cmd} ${a.join(" ")} failed (${r.status})`);
};

// 1. resolve PBS tag (pinned in lock, else releases/latest) and its SHA256SUMS
const tag: string = lock.pbs?.tag ?? (await (await gh(`https://api.github.com/repos/${REPO}/releases/latest`)).json()).tag_name;
const asset = `cpython-${PY_VERSION}+${tag}-${triple}-install_only_stripped.tar.gz`;
const base = `https://github.com/${REPO}/releases/download/${tag}`;
const sums = await (await gh(`${base}/SHA256SUMS`)).text();
const line = sums.split("\n").find((l) => l.trim().endsWith(` ${asset}`) || l.trim().endsWith(`*${asset}`));
if (!line) throw new Error(`${asset} not in SHA256SUMS of ${tag}`);
const expected = line.trim().split(/\s+/)[0];
if (lock.pbs?.assets?.[triple] && lock.pbs.assets[triple].sha256 !== expected) throw new Error("lock sha256 differs from release SHA256SUMS: fail closed");

const rtRoot = join(root, "runtime");
const rt = join(rtRoot, triple);
const py = join(rt, "python");
const stamp = join(rt, ".stamp");
const stampVal = `${asset}|${expected}|${JSON.stringify(lock.deps ?? null)}|${flag("ship")}`;
// our yt-dlp plugins (extractor fixes upstream lacks) -> python/plugins, put on sys.path by engine::runtime;
// synced on every run so a changed plugin needs no runtime rebuild
const syncPlugins = () => {
  rmSync(join(py, "plugins"), { recursive: true, force: true });
  cpSync(join(root, "plugins"), join(py, "plugins"), { recursive: true, filter: (p) => !p.includes("__pycache__") });
};
if (!flag("force") && existsSync(stamp) && readFileSync(stamp, "utf8") === stampVal) {
  syncPlugins();
  console.log("runtime up to date (use --force to rebuild)");
  process.exit(0);
}

// 2. download + verify (fail closed) + unpack
rmSync(rt, { recursive: true, force: true });
mkdirSync(rt, { recursive: true });
console.log(`downloading ${asset}`);
const buf = new Uint8Array(await (await gh(`${base}/${encodeURIComponent(asset)}`)).arrayBuffer());
const got = sha256(buf);
if (got !== expected) throw new Error(`sha256 mismatch for ${asset}: ${got} != ${expected}`);
const tgz = join(rt, asset.replace("+", "_"));
writeFileSync(tgz, buf);
run("tar", ["-xzf", basename(tgz)], rt); // bsdtar on Windows 10+, tar elsewhere; archive root is python/
rmSync(tgz);
const exe = isWin ? join(py, "python.exe") : join(py, "bin", "python3");
if (!existsSync(exe)) throw new Error(`python missing after unpack: ${exe}`);

// 3. pip deps into python/site via the runtime's own interpreter
const site = join(py, "site");
mkdirSync(site, { recursive: true });
const pip = (extra: string[]) =>
  run(exe, ["-I", "-m", "pip", "install", "--target", site, "--only-binary=:all:", "--no-warn-script-location", "--disable-pip-version-check", ...extra]);
const pinned: Record<string, string> | undefined = lock.deps;
const ytdlp = pinned?.["yt-dlp"] ?? (await (await gh("https://pypi.org/pypi/yt-dlp/json")).json()).info.version;
pip(["--no-deps", `yt-dlp==${ytdlp}`]);
// D-104: yt-dlp-ejs must be the exact pair from the wheel METADATA
const meta = readdirSync(site).find((d) => /^yt_dlp-.*\.dist-info$/.test(d))!;
const req = readFileSync(join(site, meta, "METADATA"), "utf8").match(/^Requires-Dist: yt-dlp-ejs==([^\s;]+)/m);
if (!req) throw new Error("yt-dlp METADATA has no pinned yt-dlp-ejs");
pip(["--no-deps", `yt-dlp-ejs==${req[1]}`]);
// baked natives + their deps (resolved normally; pinned versions from the lock when present)
pip(BAKED.map((n) => (pinned?.[n] ? `${n}==${pinned[n]}` : n)));

// record installed versions
const listing = spawnSync(exe, ["-I", "-c",
  "import importlib.metadata as m,json,sys;print(json.dumps({d.metadata['Name'].lower():d.version for d in m.distributions(path=[sys.argv[1]])}))", site],
  { encoding: "utf8" });
if (listing.status !== 0) throw new Error(listing.stderr);
const deps = JSON.parse(listing.stdout);

// 4. import smoke before pruning and after (fail if pruning breaks anything)
const SMOKE = "import ssl,zlib,bz2,lzma,ctypes,json,xml.etree.ElementTree,http.client,unicodedata,hashlib,sqlite3,decimal,asyncio,"
  + "certifi,requests,urllib3,websockets,mutagen,brotli,Cryptodome.Cipher.AES,yt_dlp,yt_dlp_ejs;"
  + "import yt_dlp.version as v;print(v.__version__)";
const smoke = () => {
  const r = spawnSync(exe, ["-I", "-c", `import sys;sys.path.insert(0,r'${site}');${SMOKE}`], { encoding: "utf8" });
  if (r.status !== 0) throw new Error(`import smoke failed:\n${r.stderr}`);
  return r.stdout.trim();
};
smoke();

// compile once so no worker ever pays for it (write_bytecode is off at runtime)
const stdlib = isWin ? join(py, "Lib") : join(py, "lib", "python3.14"); // unix PBS keeps the stdlib under lib/python3.14
run(exe, ["-I", "-m", "compileall", "-q", "-j", "0", site, stdlib]);

// 5. prune (runtime.prune list, architecture.md section 8)
const PRUNE_DIRS = isWin
  ? ["Lib/test", "Lib/idlelib", "Lib/tkinter", "Lib/turtledemo", "Lib/ensurepip", "Lib/pydoc_data", "Lib/venv",
    "tcl", "share", "include", "Lib/site-packages/pip", "Lib/site-packages/setuptools", "Lib/site-packages/_distutils_hack"]
  : ["lib/python3.14/test", "lib/python3.14/idlelib", "lib/python3.14/tkinter", "lib/python3.14/turtledemo", "lib/python3.14/ensurepip",
    "lib/python3.14/pydoc_data", "lib/python3.14/venv", "lib/python3.14/site-packages/pip", "lib/python3.14/site-packages/setuptools",
    "lib/python3.14/site-packages/_distutils_hack", "lib/pkgconfig", "share", "include",
    ...readdirSync(join(py, "lib")).filter((n) => /^(tcl|tk|itcl|thread)\d/.test(n)).map((n) => `lib/${n}`)];
const PRUNE_FILES = isWin
  ? ["Lib/turtle.py", "DLLs/_tkinter.pyd", "DLLs/tcl86t.dll", "DLLs/tk86t.dll", "DLLs/_ctypes_test.pyd"]
  : ["lib/python3.14/turtle.py", ...readdirSync(join(py, "lib")).filter((n) => /^lib(tcl|tk)/.test(n)).map((n) => `lib/${n}`)];
const rm = (p: string) => rmSync(join(py, p), { recursive: true, force: true });
PRUNE_DIRS.forEach(rm);
PRUNE_FILES.forEach(rm);
function walk(d: string, inLib: boolean) {
  for (const e of readdirSync(d, { withFileTypes: true })) {
    const p = join(d, e.name);
    if (!e.isDirectory()) { if (/^_(test|tkinter|ctypes_test).*\.(pyd|so)$/.test(e.name)) rmSync(p); continue; }
    // Lib/*/tests go (__pycache__ is kept on purpose: without .pyc every worker start recompiles yt-dlp, ~1 s); baked site and site-packages keep their own "tests" dirs (harmless, tiny)
    if (inLib && /^tests?$/.test(e.name)) rmSync(p, { recursive: true, force: true });
    else walk(p, inLib && e.name !== "site-packages");
  }
}
walk(py, false);
walk(stdlib, true);
rmSync(join(site, "bin"), { recursive: true, force: true });
rmSync(join(site, "share"), { recursive: true, force: true });
if (flag("ship")) rmSync(join(py, "libs"), { recursive: true, force: true }); // only linking needs libs/*.lib
const version = smoke();
console.log(`import smoke after prune ok, yt-dlp ${version}`);

// 6. pyo3.cfg for PYO3_CONFIG_FILE (absolute paths)
const abs = (p: string) => p.replaceAll("\\", "/");
writeFileSync(join(rtRoot, "pyo3.cfg"), [
  "implementation=CPython", "version=3.14", "shared=true", "abi3=false",
  `lib_name=${isWin ? "python314" : "python3.14"}`,
  `lib_dir=${abs(isWin ? join(py, "libs") : join(py, "lib"))}`,
  `executable=${abs(exe)}`, "pointer_width=64", "build_flags=", "suppress_build_script_link_lines=false", "",
].join("\n"));

// 7. size + lock
const du = (d: string): number => readdirSync(d, { withFileTypes: true }).reduce((n, e) => n + (e.isDirectory() ? du(join(d, e.name)) : statSync(join(d, e.name)).size), 0);
const sizeMb = +(du(py) / 1048576).toFixed(1);
const assets = { ...(lock.pbs?.assets ?? {}), [triple]: { name: asset, sha256: expected } };
writeFileSync(lockPath, JSON.stringify({ pbs: { tag, python: PY_VERSION, assets }, deps, size_mb: { ...(lock.size_mb ?? {}), [triple]: sizeMb } }, null, 2) + "\n");
syncPlugins();
writeFileSync(stamp, `${asset}|${expected}|${JSON.stringify(deps)}|${flag("ship")}`);
console.log(`runtime ready: ${py} (${sizeMb} MB), lock ${lockPath}`);
