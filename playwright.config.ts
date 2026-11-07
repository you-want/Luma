import { defineConfig } from "@playwright/test";

// E2E tests run against the plain Vite dev server (no Tauri shell).
// `window.__TAURI__` is not present, so every `invoke` call fails at the
// API boundary — tests either assert that failure path, or install a
// Tauri-mock init script (see `e2e/tauriMock.ts`) to drive happy paths
// with fixture data.

export default defineConfig({
  testDir: "./e2e",
  testMatch: "**/*.e2e.ts",
  fullyParallel: true,
  workers: 2,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? "github" : "list",
  timeout: 30_000,
  use: {
    baseURL: "http://localhost:1030",
    trace: "retain-on-failure",
  },
  webServer: {
    command: "pnpm dev",
    url: "http://localhost:1030",
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
});
