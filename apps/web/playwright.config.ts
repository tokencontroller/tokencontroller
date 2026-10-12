import { defineConfig, devices } from "@playwright/test";

// One journey per docs guide (quick start, review, monthly close, CSV export).
// Until the service's API lands they run against the mock build, which holds
// the docs' example organization; then they move to the compose image with
// seed data.
const port = 4173;
const executablePath = process.env.TC_CHROMIUM_PATH;

export default defineConfig({
  testDir: "e2e",
  fullyParallel: true,
  forbidOnly: Boolean(process.env.CI),
  retries: 0,
  reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : "list",
  use: {
    baseURL: process.env.TC_WEB_URL ?? `http://localhost:${port}`,
    trace: "retain-on-failure",
    ...(executablePath ? { launchOptions: { executablePath } } : {}),
  },
  projects: [
    { name: "desktop", testIgnore: /screenshots/, use: { ...devices["Desktop Chrome"], viewport: { width: 1360, height: 900 } } },
    { name: "phone", testIgnore: /screenshots/, use: { ...devices["Pixel 7"] } },
    // Screenshots of the main screens for people who do not run the app.
    { name: "screenshots", testMatch: /screenshots/, use: { ...devices["Desktop Chrome"], viewport: { width: 1360, height: 900 } } },
  ],
  // Against a running service (TC_WEB_URL) nothing is started.
  webServer: process.env.TC_WEB_URL
    ? []
    : {
        command: `pnpm build:mock && pnpm exec vite preview --outDir dist-mock --port ${port} --strictPort`,
        port,
        reuseExistingServer: !process.env.CI,
        timeout: 180_000,
      },
});
