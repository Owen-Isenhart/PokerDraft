import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "tests",
  timeout: 30000,
  use: {
    baseURL: process.env.POKERDRAFT_TEST_URL ?? "http://127.0.0.1:3000",
    headless: true,
    launchOptions: {
      executablePath: process.env.CHROMIUM_PATH ?? "/usr/bin/chromium",
      args: ["--no-sandbox"],
    },
    screenshot: "only-on-failure",
  },
  reporter: "list",
});
