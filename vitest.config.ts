import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "jsdom",
    pool: "threads",
    setupFiles: ["./src/test/setup.ts"]
  }
});
