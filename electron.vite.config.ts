import generouted from "@generouted/react-router/plugin";
import ViteYaml from "@modyfi/vite-plugin-yaml";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react-swc";
import { defineConfig } from "electron-vite";
import { merge } from "lodash-es";
import { execSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join, relative, resolve } from "path";
import type { Plugin } from "vite";
import { AliasOptions, ResolveOptions } from "vite";

console.log("current working dir:", resolve("."));
const resolveOptions: { resolve: ResolveOptions & { alias: AliasOptions } } = {
	resolve: {
		alias: {
			"@renderer": resolve("src/renderer/src"),
			"@main": resolve("src/main"),
			"@preload": resolve("src/preload"),
			"@shared": resolve("src/shared"),
			"@": resolve("src"),
			"~": resolve("."),
		},
	},
};
const externalizedEsmDeps = ["lodash-es", "@faker-js/faker", "@trpc-limiter/memory", "got", "encryption.js", "filenamify", "yt-dlp-wrap", "p-queue"];
const isMac = !!process.env.ACTION_RUNNER?.startsWith("macos-");
const isProduction = process.env.NODE_ENV === "production";

const PY_BYTECODE_PREFIX = "\0py-bytecode:";
const PY_ASSET_RE = /\.py\?asset(&asarUnpack)?(?=&|$)/;
const PY_REQUIREMENTS_PATH = resolve("requirements.txt");
const PY_DEPS_ASSET_DIR = "resources/python-deps";

const COMPILE_SCRIPT = `import py_compile\nimport sys\npy_compile.compile(sys.argv[1], cfile=sys.argv[2])\n`;
const COMPILE_EXT = ".pyc";
function compilePyToPyc(pyPath: string): Buffer {
	const outFile = resolve(tmpdir(), `ytdlp-${basename(pyPath, ".py")}-${Date.now()}.${COMPILE_EXT}`);
	const scriptPath = resolve(tmpdir(), "ytdlp-compile-worker.py");
	writeFileSync(scriptPath, COMPILE_SCRIPT, { encoding: "utf8" }); // ensure UTF-8 script
	try {
		let compiled = false;
		for (const py of ["python", "python3", "py"]) {
			try {
				execSync(
					// explicitly set PYTHONIOENCODING to utf-8 for output
					`${py} "${scriptPath}" "${pyPath}" "${outFile}"`,
					{
						stdio: "pipe",
						windowsHide: true,
						env: { ...process.env, PYTHONIOENCODING: "utf-8" }, // redundant, but help just in case
					}
				);
				compiled = true;
				break;
			} catch (err: any) {
				// dump any error output for debugging
				if (
					typeof err === "object" &&
					err !== null &&
					"stderr" in err &&
					Buffer.isBuffer(err.stderr)
				) {
					const stderrStr = err.stderr.toString("utf8");
					if (stderrStr) {
						console.error(`[python-bytecode] ${py} stderr:\n${stderrStr}`);
					}
				}
				// try next
			}
		}
		if (!compiled || !existsSync(outFile)) {
			throw new Error(`[python-bytecode] Failed to compile ${pyPath}. Ensure Python is installed (python/python3/py) and the file exists.`);
		}
		return readFileSync(outFile);
	} finally {
		// Always cleanup
		try { unlinkSync(scriptPath); } catch {}
		if (existsSync(outFile)) try { unlinkSync(outFile); } catch {}
	}
}

function getPythonCandidates(): readonly string[] {
	return ["python", "python3", "py"] as const;
}

function runPythonCommand(commandFactory: (pythonExecutable: string) => string): void {
	for (const py of getPythonCandidates()) {
		try {
			execSync(commandFactory(py), {
				stdio: "pipe",
				windowsHide: true,
			});
			return;
		} catch {
			/* try next */
		}
	}
	throw new Error("[python-bytecode] Failed to run Python command using python/python3/py.");
}

function normalizeToken(value: string): string {
	return value.toLowerCase().replace(/[-_.]/g, "");
}

function extractImportedModules(pyPath: string): Set<string> {
	const sourceCode = readFileSync(pyPath, "utf8");
	const modules = new Set<string>();

	const importMatches = sourceCode.matchAll(/^\s*import\s+([A-Za-z_][A-Za-z0-9_\.]*)/gm);
	for (const match of importMatches) {
		const [topLevel] = match[1].split(".");
		if (topLevel) {
			modules.add(normalizeToken(topLevel));
		}
	}

	const fromImportMatches = sourceCode.matchAll(/^\s*from\s+([A-Za-z_][A-Za-z0-9_\.]*)\s+import\s+/gm);
	for (const match of fromImportMatches) {
		const [topLevel] = match[1].split(".");
		if (topLevel) {
			modules.add(normalizeToken(topLevel));
		}
	}

	return modules;
}

function getRequiredRequirementLines(pyPath: string, requirementsPath: string): string[] {
	if (!existsSync(requirementsPath)) {
		return [];
	}

	const importedModules = extractImportedModules(pyPath);
	if (importedModules.size === 0) {
		return [];
	}

	const requirementsContent = readFileSync(requirementsPath, "utf8");
	const requirementLines = requirementsContent
		.split(/\r?\n/)
		.map((line) => line.trim())
		.filter((line) => line.length > 0 && !line.startsWith("#"));

	return requirementLines.filter((line) => {
		const packageName = line.split(/==|>=|<=|~=|!=|>|<|@/)[0]?.trim();
		if (!packageName) {
			return false;
		}
		return importedModules.has(normalizeToken(packageName));
	});
}

function installRequirementsToTarget(requirementLines: readonly string[], targetDir: string): void {
	rmSync(targetDir, { recursive: true, force: true });
	mkdirSync(targetDir, { recursive: true });
	const tempRequirementsPath = join(targetDir, "requirements.selected.txt");
	writeFileSync(tempRequirementsPath, `${requirementLines.join("\n")}\n`);
	try {
		runPythonCommand((py) => `${py} -m pip install -r "${tempRequirementsPath}" --target "${targetDir}" --disable-pip-version-check --no-compile`);
	} finally {
		if (existsSync(tempRequirementsPath)) {
			unlinkSync(tempRequirementsPath);
		}
	}
}

function emitDirectoryAssets(emitAsset: (assetName: string, sourceBuffer: Buffer) => void, rootDir: string, currentDir: string = rootDir): void {
	const entries = readdirSync(currentDir);
	for (const entry of entries) {
		const fullPath = join(currentDir, entry);
		const stats = statSync(fullPath);
		if (stats.isDirectory()) {
			emitDirectoryAssets(emitAsset, rootDir, fullPath);
			continue;
		}
		const relativePath = relative(rootDir, fullPath).replace(/\\/g, "/");
		emitAsset(`${PY_DEPS_ASSET_DIR}/${relativePath}`, readFileSync(fullPath));
	}
}

function pythonBytecodePlugin(): Plugin {
	let depsTmpDir: string | null = null;
	let depsEmitted = false;

	return {
		name: "python-bytecode",
		enforce: "pre",
		buildStart() {
			if (!existsSync(PY_REQUIREMENTS_PATH)) {
				return;
			}
			if (depsTmpDir) {
				return;
			}
			depsTmpDir = mkdtempSync(join(tmpdir(), "ytdlp-pydeps-"));
		},
		async resolveId(source, importer) {
			if (!PY_ASSET_RE.test(source)) return null;
			const pyPath = source.replace(/\?.*$/, "");
			const resolved = await this.resolve(pyPath, importer ?? undefined, { skipSelf: true });
			if (!resolved?.id) return null;
			return `${PY_BYTECODE_PREFIX}${resolved.id}`;
		},
		load(id) {
			if (!id.startsWith(PY_BYTECODE_PREFIX)) return null;
			const pyPath = id.slice(PY_BYTECODE_PREFIX.length);

			if (depsTmpDir && !depsEmitted) {
				const requirementLines = getRequiredRequirementLines(pyPath, PY_REQUIREMENTS_PATH);
				if (requirementLines.length > 0) {
					installRequirementsToTarget(requirementLines, depsTmpDir);
					emitDirectoryAssets((assetName, sourceBuffer) => {
						this.emitFile({ type: "asset", name: assetName, source: sourceBuffer });
					}, depsTmpDir);
				}
				depsEmitted = true;
			}

			// const pycBuffer = compilePyToPyc(pyPath);
			const workerAssetName = `${relative(resolve("src/main"), pyPath).replace(/\\/g, "/").replace(/\//g, "__").replace(/\.py$/, "")}.${COMPILE_EXT}`;
			const ref = this.emitFile({ type: "asset", fileName: `resources/python-workers/${workerAssetName}`, source: Buffer.from(readFileSync(pyPath)) });
			return `import { existsSync } from "node:fs";
			import { fileURLToPath } from "node:url";
			const rawPath = import.meta.ROLLUP_FILE_URL_${ref};
			const normalizedPath = rawPath.startsWith("file:") ? fileURLToPath(rawPath) : rawPath;
			const unpackedPath = normalizedPath.replace(/([\\\\/])app\\.asar([\\\\/])/i, "$1app.asar.unpacked$2");
			const resolvedPath = existsSync(unpackedPath) ? unpackedPath : normalizedPath;
			console.log("PyWorker:", {
				resolvedPath,
				normalizedPath,
				unpackedPath,
			});
			export default resolvedPath;`;
		},
		closeBundle() {
			if (depsTmpDir) {
				rmSync(depsTmpDir, { recursive: true, force: true });
				depsTmpDir = null;
			}
		},
	};
}

export default defineConfig({
	main: {
		...resolveOptions,
		plugins: [ViteYaml(), pythonBytecodePlugin()],
		build: {
			bytecode: isProduction ? { transformArrowFunctions: false } : false,
			externalizeDeps: { exclude: [...externalizedEsmDeps] },
			rollupOptions: {
				output: {
					manualChunks: (id: string): any => {
						if (externalizedEsmDeps.find((d) => d === id)) return id;
					},
				},
			},
		},
		publicDir: "./resources",
	},
	preload: {
		...resolveOptions,
		plugins: [ViteYaml()],
		build: { externalizeDeps: { exclude: [...externalizedEsmDeps] }, bytecode: isProduction ? { transformArrowFunctions: false } : false },
	},
	renderer: {
		...merge(resolveOptions, {
			resolve: {
				alias: {
					"@": resolve("src/renderer/src"),
				},
			},
		}),
		plugins: [
			ViteYaml(),
			react({
				plugins: [["@swc/plugin-styled-jsx", {}]],
			}),
			tailwindcss({ optimize: isProduction }),
			generouted({
				source: {
					routes: "./src/renderer/src/pages/**/{page,index}.{jsx,tsx}",
					modals: "./src/renderer/src/pages/**/[+]*.{jsx,tsx}",
				},
				output: resolve("./src/renderer/src/router.ts"),
			}),
		],
	},
});
