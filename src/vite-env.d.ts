/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_BROWSER_MOCK?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
