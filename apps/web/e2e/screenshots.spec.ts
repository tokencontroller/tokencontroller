import { test } from "@playwright/test";
import { signIn } from "./helpers";

// Pictures of the main screens with the example organization, for people who
// read the product rather than run it. Not a test of anything: run with
// `pnpm screenshots`; files land in screenshots/ (or TC_SCREENSHOT_DIR).
const dir = process.env.TC_SCREENSHOT_DIR ?? "screenshots";

const screens = [
  { who: "mara", path: "/review", name: "01-review-and-submit" },
  { who: "mara", path: "/", name: "02-overview-agent-manager" },
  { who: "priya", path: "/", name: "03-overview-people-manager" },
  { who: "priya", path: "/approval?period=2026-09", name: "04-approval" },
  { who: "dana", path: "/", name: "05-overview-controller" },
  { who: "dana", path: "/bills?period=2026-09", name: "06-bills-and-reconciliation" },
  { who: "dana", path: "/close", name: "07-monthly-close" },
  { who: "dana", path: "/close/2026-08", name: "08-cost-sheet" },
  { who: "priya", path: "/budgets", name: "09-token-budgets" },
  { who: "dana", path: "/reports", name: "10-reports" },
  { who: "dana", path: "/token-entries", name: "11-token-entries" },
  { who: "priya", path: "/projects/storefront", name: "12-project-and-tickets" },
  { who: "dana", path: "/tags", name: "13-tags-and-tag-map" },
  { who: "mara", path: "/rules", name: "14-rules" },
  { who: "dana", path: "/people", name: "15-people" },
  { who: "dana", path: "/settings", name: "16-settings" },
] as const;

for (const screen of screens) {
  test(screen.name, async ({ page }) => {
    await signIn(page, screen.who);
    await page.goto(screen.path);
    await page.waitForLoadState("networkidle");
    await page.screenshot({ path: `${dir}/${screen.name}.png`, fullPage: true });
  });
}

test("phone", async ({ browser, baseURL }) => {
  const context = await browser.newContext({ ...(baseURL ? { baseURL } : {}), viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, isMobile: true, hasTouch: true });
  const page = await context.newPage();
  await signIn(page, "mara");
  await page.goto("/review");
  await page.waitForLoadState("networkidle");
  await page.screenshot({ path: `${dir}/17-phone-review.png` });
  await context.close();
});

test("dark", async ({ browser, baseURL }) => {
  const context = await browser.newContext({ ...(baseURL ? { baseURL } : {}), viewport: { width: 1360, height: 900 }, colorScheme: "dark" });
  const page = await context.newPage();
  await signIn(page, "dana");
  await page.goto("/close/2026-08");
  await page.waitForLoadState("networkidle");
  await page.screenshot({ path: `${dir}/18-dark-cost-sheet.png`, fullPage: true });
  await context.close();
});
