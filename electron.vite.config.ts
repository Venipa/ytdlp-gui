import generouted from "@generouted/react-router/plugin";
import ViteYaml from "@modyfi/vite-plugin-yaml";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react-swc";
import { defineConfig } from "electron-vite";
import { merge } from "lodash-es";
import { readFileSync, readdirSync, statSync } from "node:fs";
import path from "node:path";
import { join, resolve } from "path";
import type { Plugin } from "vite";
import { AliasOptions, ResolveOptions } from "vite";
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
const externalizedEsmDeps = ["lodash-es", "@faker-js/faker", "@trpc-limiter/memory", "got", "encryption.js", "filenamify", "p-queue", "superjson"];
const isProduction = process.env.NODE_ENV === "production";

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
		plugins: [ViteYaml(), injectExtensionsPlugin()],
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
