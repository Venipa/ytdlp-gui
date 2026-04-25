import generouted from "@generouted/react-router/plugin";
import ViteYaml from "@modyfi/vite-plugin-yaml";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react-swc";
import { defineConfig } from "electron-vite";
import { merge } from "lodash-es";
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, unlinkSync, writeFileSync } from "node:fs";
import path, { dirname } from "node:path";
import { basename, join, relative, resolve } from "path";
import type { Plugin } from "vite";
import { AliasOptions, ResolveOptions, normalizePath } from "vite";
import "./src/shared/extensions/string.ts";
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

function isPythonAssetRequest(source: string): boolean {
	const [filePath, queryString = ""] = source.split("?", 2);
	if (!filePath.endsWith(".py")) return false;
	const searchParams = new URLSearchParams(queryString);
	return searchParams.has("asset");
}


function pythonAssetPlugin(): Plugin {
	const pythonAssetModuleIds = new Map<string, string>();
	let isServeMode = false;
	return {
		name: "python-bytecode",
		enforce: "pre",
		configResolved(config) {
			isServeMode = config.command === "serve";
		},
		async resolveId(source, importer) {
			if (!isPythonAssetRequest(source)) return null;
			const pyPath = source.replace(/\?.*$/, "");
			const resolved = await this.resolve(pyPath, importer ?? undefined, { skipSelf: true });
			if (!resolved?.id) return null;
			return `${PY_BYTECODE_PREFIX}${resolved.id}`;
		},
		load(id) {
			if (!id.startsWith(PY_BYTECODE_PREFIX)) return null;
			const pyPath = id.slice(PY_BYTECODE_PREFIX.length);
			const normalizedPyPath = normalizePath(pyPath);
			pythonAssetModuleIds.set(normalizedPyPath, id);
			if (isServeMode) {
				return `export default ${JSON.stringify(pyPath)};`;
			}

			// if (depsTmpDir && !depsEmitted) {
			// 	const requirementLines = getRequiredRequirementLines(pyPath, PY_REQUIREMENTS_PATH);
			// 	if (requirementLines.length > 0) {
			// 		installRequirementsToTarget(requirementLines, depsTmpDir);
			// 		emitDirectoryAssets((assetName, sourceBuffer) => {
			// 			this.emitFile({ type: "asset", name: assetName, source: sourceBuffer });
			// 		}, depsTmpDir);
			// 	}
			// 	depsEmitted = true;
			// }

			// const pycBuffer = compilePyToPyc(pyPath);
			const workerAssetName = `${relative(resolve("src/main"), pyPath).replace(/\\/g, "/").replace(/\//g, "__").replace(/\.py$/, "")}.py`;
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
		handleHotUpdate(ctx) {
			const changedPath = normalizePath(ctx.file);
			const moduleId = pythonAssetModuleIds.get(changedPath);
			if (!moduleId) return;
			const module = ctx.server.moduleGraph.getModuleById(moduleId);
			if (!module) return;
			ctx.server.moduleGraph.invalidateModule(module);
			return [module];
		},
	};
}

function venvCopyPlugin(options: { venvPath: string }): Plugin {
	const { venvPath } = options;
  let alreadyCopied = false;
	return {
		name: "copy-venv-to-main-resources",
		apply: "build",
		async closeBundle() {
			const outVenvPath = resolve("out/main/resources/venv");
			// Only copy if .venv exists and we're not already inside out/main/resources/venv
			if (!existsSync(venvPath)) {
				console.warn("[copy-venv-to-main-resources] No .venv directory found, skipping venv copy.");
				return;
			}
			if (alreadyCopied) return;
			alreadyCopied = true; // prevents hmr from copying the venv multiple times
			const copyRecursiveSync = (src, dest) => {
				if (statSync(src).isDirectory()) {
					if (!existsSync(dest)) mkdirSync(dest, { recursive: true });
					for (const file of readdirSync(src)) {
						copyRecursiveSync(join(src, file), join(dest, file));
					}
				} else {
					const isExecutable = basename(dirname(dest)) === "bin";
					writeFileSync(dest, readFileSync(src), { mode: isExecutable ? 0o755 : undefined, flag: isExecutable ? "ax" : undefined });
				}
			};
			// Remove previous if exists
			if (existsSync(outVenvPath)) {
				const deleteRecursiveSync = (dir) => {
					for (const file of readdirSync(dir)) {
						const curPath = join(dir, file);
						if (statSync(curPath).isDirectory()) {
							deleteRecursiveSync(curPath);
							rmSync(curPath, { recursive: true, force: true });
						} else {
							unlinkSync(curPath);
						}
					}
					rmSync(dir, { recursive: true, force: true });
				};
				deleteRecursiveSync(outVenvPath);
			}
			copyRecursiveSync(venvPath, outVenvPath);
			console.log("[copy-venv-to-main-resources] Copied .venv to out/main/resources/venv");
		},
	};
}

function injectExtensionsPlugin(): Plugin {
	return {
		name: "inject-extensions-plugin",
		enforce: "pre",
		// Never claim ids here; let Vite/plugin pipeline resolve modules.
		resolveId() {
			return null;
		},
		async transform(code: string, id: string) {
			if (
				!/src[\\/].*\.(ts|js)$/.test(id) ||
				id.includes("src/shared/extensions/")
			) {
				return null;
			}
			const extensionsDir = resolve("./src/shared/extensions");
			// Helper to get all .ts files in extensionsDir/* and extensionsDir/*/*
			function collectExtensionFiles(rootDir: string): string[] {
				const results: string[] = [];
				for (const f of readdirSync(rootDir)) {
					const absPath = join(rootDir, f);
					const stat = statSync(absPath);
					if (stat.isFile() && f.endsWith(".ts")) {
						results.push(absPath);
					} else if (stat.isDirectory()) {
						for (const subF of readdirSync(absPath)) {
							const subAbsPath = join(absPath, subF);
							const subStat = statSync(subAbsPath);
							if (subStat.isFile() && subF.endsWith(".ts")) {
								results.push(subAbsPath);
							}
						}
					}
				}
				return results;
			}

			const extensionFiles = collectExtensionFiles(extensionsDir).filter((filepath) => {
				const source = readFileSync(filepath, "utf8");
				return source.includes(".prototype.");
			});

			const extensionImports: string[] = extensionFiles.map((absFile) => {
				const importPath = path.relative(path.dirname(id), absFile).replace(/\\/g, "/").replace(/\.ts$/, "");
				return `import "${importPath.startsWith(".") ? importPath : "./" + importPath}";`;
			});

			if (extensionImports.length === 0) {
				return null;
			}

			const newImports = extensionImports.filter((imp) => !code.includes(imp));
			if (newImports.length === 0) {
				return null;
			}
			const injectedCode = [...newImports, code].join("\n");
			return {
				code: injectedCode,
				map: null,
			};
		},
	};
}

export default defineConfig({
	main: {
		...resolveOptions,
		plugins: [ViteYaml(), pythonAssetPlugin(), venvCopyPlugin({ venvPath: resolve(".venv") }), injectExtensionsPlugin()],
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
		plugins: [ViteYaml(), injectExtensionsPlugin()],
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
			injectExtensionsPlugin(),
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
