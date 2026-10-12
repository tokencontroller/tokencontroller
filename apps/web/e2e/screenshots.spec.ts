import { expect, test } from "@playwright/test";
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
  { who: "nadia", path: "/", name: "05-overview-controller" },
  { who: "nadia", path: "/bills?period=2026-09", name: "06-bills-and-reconciliation" },
  { who: "nadia", path: "/close", name: "07-monthly-close" },
  { who: "nadia", path: "/close/2026-08", name: "08-cost-sheet" },
  { who: "priya", path: "/budgets", name: "09-token-budgets" },
  { who: "nadia", path: "/reports", name: "10-reports" },
  { who: "nadia", path: "/token-entries", name: "11-token-entries" },
  { who: "priya", path: "/projects/storefront", name: "12-project-and-tickets" },
  { who: "nadia", path: "/tags", name: "13-tags-and-tag-map" },
  { who: "mara", path: "/rules", name: "14-rules" },
  { who: "nadia", path: "/people", name: "15-people" },
  { who: "nadia", path: "/settings", name: "16-settings" },
  { who: "nadia", path: "/billing", name: "19-billing-team-plan" },
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
  await signIn(page, "nadia");
  await page.goto("/close/2026-08");
  await page.waitForLoadState("networkidle");
  await page.screenshot({ path: `${dir}/18-dark-cost-sheet.png`, fullPage: true });
  await context.close();
});

test("invite", async ({ page }) => {
  await page.goto("/i/kestrel-4f2a");
  await page.waitForLoadState("networkidle");
  await page.screenshot({ path: `${dir}/20-invite.png`, fullPage: true });
});

test("a new organization", async ({ page }) => {
  await page.goto("/signup");
  await page.getByLabel("Organization", { exact: true }).fill("Harbor Labs");
  await page.getByLabel("Work email").fill("ada@harbor.example");
  await page.getByRole("button", { name: "Create organization" }).click();
  await page.getByRole("button", { name: "Open the sign-in link" }).click();
  await page.getByLabel("Your name").fill("Ada Brandt");
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByRole("heading", { name: "First steps" })).toBeVisible();
  await page.screenshot({ path: `${dir}/21-first-steps.png`, fullPage: true });
  await page.goto("/billing");
  await page.getByLabel("Billing country").selectOption("AT");
  await page.screenshot({ path: `${dir}/22-billing-solo.png`, fullPage: true });
});

test("feedback panel", async ({ page }) => {
  await signIn(page, "mara");
  await page.goto("/review");
  await page.getByRole("navigation", { name: "Main" }).getByRole("button", { name: "Send feedback" }).click();
  const panel = page.getByRole("dialog", { name: "Send feedback" });
  await panel.getByLabel("What happened, or what you would like").fill("The ticket list in the token entry dialog cuts off on my phone, so I cannot pick CP-12.");
  await panel.getByRole("switch", { name: "Add a screenshot" }).click();
  const box = await panel.getByTestId("screenshot").boundingBox();
  if (box) {
    await page.mouse.move(box.x + box.width * 0.22, box.y + box.height * 0.3);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width * 0.75, box.y + box.height * 0.55, { steps: 4 });
    await page.mouse.up();
  }
  await panel.getByText("What goes along").click();
  await page.screenshot({ path: `${dir}/23-feedback-panel.png` });
});

test("admin area", async ({ page }) => {
  await signIn(page, "nadia");
  await page.evaluate(() => fetch("/api/v1/__mock/operator", { method: "POST" }));
  const shots: [string, string][] = [
    ["/admin", "24-admin-overview"],
    ["/admin/support/5c000000-0000-4000-8000-000000000001", "25-admin-support-conversation"],
    ["/admin/customers/6b657374-0000-4000-8000-000000000001", "26-admin-customer"],
    ["/admin/newsletter", "27-admin-newsletter"],
  ];
  for (const [path, name] of shots) {
    await page.goto(path);
    await page.waitForLoadState("networkidle");
    await page.screenshot({ path: `${dir}/${name}.png`, fullPage: true });
  }
});
