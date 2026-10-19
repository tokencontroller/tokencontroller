import { test } from "@playwright/test";
import { signIn } from "./helpers";

// Pictures of the main screens with the docs' worked month, for people who
// read the product rather than run it. Not a test of anything: run with
// `pnpm screenshots`; files land in screenshots/ (or TC_SCREENSHOT_DIR).
const dir = process.env.TC_SCREENSHOT_DIR ?? "screenshots";

const screens = [
  { who: "nadia", path: "/", name: "01-overview-org-manager" },
  { who: "mara", path: "/", name: "02-overview-agent-manager" },
  { who: "mara", path: "/sessions", name: "03-check-and-submit" },
  { who: "priya", path: "/submissions", name: "04-submissions" },
  { who: "nadia", path: "/reports", name: "05-reports" },
  { who: "nadia", path: "/bills", name: "06-bills-and-reconciliation" },
  { who: "nadia", path: "/plans", name: "07-plans-and-estimates" },
  { who: "nadia", path: "/tags", name: "08-tags" },
  { who: "nadia", path: "/attributes", name: "09-attributes" },
  { who: "nadia", path: "/grants", name: "10-grants-and-roles" },
  { who: "nadia", path: "/cutoff", name: "11-cutoff" },
  { who: "nadia", path: "/price-table", name: "12-price-table" },
  { who: "mara", path: "/rules", name: "13-rules" },
  { who: "nadia", path: "/settings", name: "14-settings" },
] as const;

for (const screen of screens) {
  test(screen.name, async ({ page }) => {
    await signIn(page, screen.who);
    await page.goto(screen.path);
    await page.waitForLoadState("networkidle");
    await page.screenshot({ path: `${dir}/${screen.name}.png`, fullPage: true });
  });
}

test("15-deviation-report", async ({ page }) => {
  await signIn(page, "nadia");
  await page.goto("/reports");
  await page.getByRole("tab", { name: "Deviation" }).click();
  await page.getByRole("table", { name: "Deviation report" }).waitFor();
  await page.waitForLoadState("networkidle");
  await page.screenshot({ path: `${dir}/15-deviation-report.png`, fullPage: true });
});

test("16-sign-in", async ({ page }) => {
  await page.goto("/sign-in");
  await page.screenshot({ path: `${dir}/16-sign-in.png`, fullPage: true });
});
