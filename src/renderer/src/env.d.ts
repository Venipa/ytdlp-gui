/// <reference types="vite/client" />

interface ImportMetaEnv {
	readonly NODE_ENV: "production" | "development";
}
interface ImportMeta {
	readonly env: ImportMetaEnv;
}
