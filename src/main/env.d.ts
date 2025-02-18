/// <reference types="vite/client" />


interface ImportMetaEnv {
  readonly MAIN_VITE_ANYSTACK_API_KEY: string
  readonly MAIN_VITE_ANYSTACK_PRODUCT_ID: string
  readonly NODE_ENV: "production" | "development";

}
interface ImportMeta {
  readonly env: ImportMetaEnv
}
