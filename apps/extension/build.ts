// bun build.ts [--unsigned]: bundles the extension into dist/chrome and dist/firefox, packs
// dist/ytdlpd-chrome.crx and dist/ytdlpd-firefox.xpi (unsigned; Firefox release builds need AMO signing).
// The signing key comes from env CRX_KEY_PEM, else the dev key in F:\tmp\ytdlpd-rs (generated once, never committed).
// --unsigned (CI without the key, e.g. pull requests from forks): no crx, no id check, no `key` in the manifest.
import { createPrivateKey, createPublicKey, generateKeyPairSync } from "node:crypto";
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { extensionId, packCrx, zip } from "./crx";

const root = import.meta.dir;
const dist = join(root, "dist");
const out = join(dist, "chrome");
const ff = join(dist, "firefox");
const unsigned = process.argv.includes("--unsigned");
const DEV_KEY = process.env.YTDLPD_EXT_DEV_KEY ?? "F:\\tmp\\ytdlpd-rs\\ext-dev-key.pem";
// Firefox gets a fixed id of its own; its origin UUID is random per install, so the bridge accepts any moz-extension origin
const GECKO_ID = "extension@ytdlpd";

function signingKey(): string {
  if (process.env.CRX_KEY_PEM) return process.env.CRX_KEY_PEM;
  if (!existsSync(DEV_KEY)) {
    mkdirSync(dirname(DEV_KEY), { recursive: true });
    const { privateKey } = generateKeyPairSync("rsa", { modulusLength: 2048 });
    writeFileSync(DEV_KEY, privateKey.export({ type: "pkcs8", format: "pem" }));
    console.log(`extension: generated dev key ${DEV_KEY}`);
  }
  return readFileSync(DEV_KEY, "utf8");
}

const priv = unsigned ? undefined : createPrivateKey(signingKey());
const spki = priv && (createPublicKey(priv).export({ type: "spki", format: "der" }) as Buffer);

if (spki) {
  // the bridge refuses any extension id outside its allowlist: a build signed with another key could never pair
  const ids = join(root, "..", "desktop", "crates", "ytdlpd", "src", "browser.rs");
  const allow = /CHROME_IDS[^=]*=\s*&\[([^\]]*)\]/.exec(readFileSync(ids, "utf8"));
  if (!allow) { console.error(`extension: CHROME_IDS not found in ${ids}`); process.exit(1); }
  const id = extensionId(spki);
  if (![...allow[1]!.matchAll(/"([a-p]{32})"/g)].some((m) => m[1] === id)) {
    console.error(`extension: the signing key's id ${id} is not in CHROME_IDS (${ids}); use the release key (CRX_KEY_PEM) or add the id`);
    process.exit(1);
  }
}

rmSync(dist, { recursive: true, force: true });
mkdirSync(out, { recursive: true });

const built = await Bun.build({
  entrypoints: ["background", "content", "popup"].map((n) => join(root, "src", n + ".ts")),
  outdir: out,
  target: "browser",
  naming: "[name].js",
});
if (!built.success) {
  for (const l of built.logs) console.error(l);
  process.exit(1);
}

for (const f of ["popup.html", "popup.css", "tokens.css"]) cpSync(join(root, "src", f), join(out, f));
cpSync(join(root, "_locales"), join(out, "_locales"), { recursive: true });
cpSync(join(root, "..", "..", "icons", "ytdlpd", "web", "icon-192.png"), join(out, "icon.png"));

const manifest = JSON.parse(readFileSync(join(root, "manifest.chrome.json"), "utf8"));
const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
manifest.version = pkg.version;

// Firefox: the same bundle, an event page instead of a service worker, and its own id
cpSync(out, ff, { recursive: true });
const { minimum_chrome_version: _chrome, ...common } = manifest;
const firefox = {
  ...common,
  background: { scripts: ["background.js"], type: "module" },
  browser_specific_settings: {
    gecko: { id: GECKO_ID, strict_min_version: "128.0", data_collection_permissions: { required: ["none"] } },
  },
};
writeFileSync(join(ff, "manifest.json"), JSON.stringify(firefox, null, 2));

if (spki) manifest.key = spki.toString("base64"); // fixes the extension id to the key's
writeFileSync(join(out, "manifest.json"), JSON.stringify(manifest, null, 2));

function filesOf(base: string): Map<string, Uint8Array> {
  const files = new Map<string, Uint8Array>();
  const walk = (dir: string) => {
    for (const n of readdirSync(dir)) {
      const p = join(dir, n);
      statSync(p).isDirectory() ? walk(p) : files.set(relative(base, p).replaceAll("\\", "/"), readFileSync(p));
    }
  };
  walk(base);
  return files;
}

const xpi = join(dist, "ytdlpd-firefox.xpi");
writeFileSync(xpi, zip(filesOf(ff)));
console.log(`extension: ${out}\nextension: ${ff}\nextension: ${xpi}`);
if (priv && spki) {
  const crx = join(dist, "ytdlpd-chrome.crx");
  writeFileSync(crx, packCrx(zip(filesOf(out)), priv, spki));
  console.log(`extension: ${crx}\nextension id: ${extensionId(spki)}`);
}
