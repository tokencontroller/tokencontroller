import { expect, test } from "@playwright/test";
import { fillToMemberLimit, signIn } from "./helpers";

// Pictures of the main screens with the example organization, for people who
// read the product rather than run it. Not a test of anything: run with
// `pnpm screenshots`; files land in screenshots/ (or TC_SCREENSHOT_DIR).
const dir = process.env.TC_SCREENSHOT_DIR ?? "screenshots";

// The mock build holds fixed months. A running service (TC_SIGN_IN_LINK_CMD)
// has the seed's months: last month is closed, this month is open.
function month(offset: number): string {
  const date = new Date();
  date.setUTCDate(1);
  date.setUTCMonth(date.getUTCMonth() + offset);
  return date.toISOString().slice(0, 7);
}
const real = Boolean(process.env.TC_SIGN_IN_LINK_CMD);
const openMonth = real ? month(0) : "2026-09";
const closedMonth = real ? month(-1) : "2026-08";

const screens = [
  { who: "mara", path: "/review", name: "01-review-and-submit" },
  { who: "mara", path: "/", name: "02-overview-agent-manager" },
  { who: "priya", path: "/", name: "03-overview-people-manager" },
  { who: "priya", path: `/approval?period=${openMonth}`, name: "04-approval" },
  { who: "nadia", path: "/", name: "05-overview-controller" },
  {
    who: "nadia",
    path: `/bills?period=${closedMonth}`,
    name: "06-bills-and-reconciliation",
  },
  { who: "nadia", path: "/close", name: "07-monthly-close" },
  { who: "nadia", path: `/close/${closedMonth}`, name: "08-cost-sheet" },
  { who: "priya", path: "/budgets", name: "09-token-budgets" },
  { who: "nadia", path: "/reports", name: "10-reports" },
  { who: "nadia", path: "/token-entries", name: "11-token-entries" },
  {
    who: "priya",
    path: "/projects/storefront",
    name: "12-project-and-tickets",
  },
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
    await page.screenshot({
      path: `${dir}/${screen.name}.png`,
      fullPage: true,
    });
  });
}

test("token forecast and token budget vs actual", async ({ page }) => {
  await signIn(page, "kai");
  await page.goto("/budgets");
  await page.getByLabel("Ticket for the token forecast").selectOption("KD-260");
  await page.getByRole("button", { name: "Make a token forecast" }).click();
  await page.getByRole("list", { name: "Similar past tickets" }).waitFor();
  await page.screenshot({
    path: `${dir}/31-token-forecast.png`,
    fullPage: true,
  });
  await page.goto("/reports");
  await page.getByRole("tab", { name: "Token budget vs actual" }).click();
  await page.getByRole("button", { name: "ticket: KD-214" }).click();
  await page
    .getByRole("dialog", { name: "ticket: KD-214" })
    .getByRole("table", { name: "Token entries" })
    .waitFor();
  await page.waitForLoadState("networkidle");
  await page.screenshot({ path: `${dir}/32-token-budget-vs-actual.png` });
  await page
    .getByRole("dialog", { name: "ticket: KD-214" })
    .getByRole("button", { name: "Close" })
    .click();
  await page.getByRole("switch", { name: "Share view" }).click();
  await page.getByText(/· share of the period/).waitFor();
  await page.screenshot({
    path: `${dir}/33-token-budget-share-view.png`,
    fullPage: true,
  });
  // The token forecast made for KD-231 in September, read again with the token budget it became.
  await page.goto("/budgets");
  await page.getByLabel("Ticket for the token forecast").selectOption("KD-231");
  await page.getByText("accepted as a token budget").waitFor();
  await page.screenshot({
    path: `${dir}/34-token-forecast-accepted.png`,
    fullPage: true,
  });
});

test("token entry", async ({ page }) => {
  await signIn(page, "priya");
  await page.goto("/token-entries");
  await page
    .getByRole("table", { name: "Token entries" })
    .locator("tbody tr")
    .filter({ hasText: "Traced the declined card redirect" })
    .first()
    .click();
  await page
    .getByRole("dialog", { name: "Token entry" })
    .getByText(/ input · /)
    .waitFor();
  await page.screenshot({ path: `${dir}/35-token-entry.png` });
});

test("phone", async ({ browser, baseURL }) => {
  const context = await browser.newContext({
    ...(baseURL ? { baseURL } : {}),
    viewport: { width: 390, height: 844 },
    deviceScaleFactor: 2,
    isMobile: true,
    hasTouch: true,
  });
  const page = await context.newPage();
  await signIn(page, "mara");
  await page.goto("/review");
  await page.waitForLoadState("networkidle");
  await page.screenshot({ path: `${dir}/17-phone-review.png` });
  await context.close();
});

test("dark", async ({ browser, baseURL }) => {
  const context = await browser.newContext({
    ...(baseURL ? { baseURL } : {}),
    viewport: { width: 1360, height: 900 },
    colorScheme: "dark",
  });
  const page = await context.newPage();
  await signIn(page, "nadia");
  await page.goto(`/close/${closedMonth}`);
  await page.waitForLoadState("networkidle");
  await page.screenshot({
    path: `${dir}/18-dark-cost-sheet.png`,
    fullPage: true,
  });
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
  await expect(
    page.getByRole("heading", { name: "First steps" }),
  ).toBeVisible();
  await page.screenshot({ path: `${dir}/21-first-steps.png`, fullPage: true });
});

test("ticket systems", async ({ page }) => {
  await signIn(page, "nadia");
  await page.goto("/settings");
  await page.getByRole("button", { name: "Connect a ticket system" }).click();
  const connect = page.getByRole("dialog", { name: "Connect a ticket system" });
  await connect
    .getByLabel("Jira Cloud site")
    .fill("https://kestrel-labs.atlassian.net");
  await connect
    .getByLabel("Email of the Atlassian account")
    .fill("n.berger@kestrel.example");
  await connect.getByLabel("API token").fill("ATATT3x-labs-9fQe");
  await page.screenshot({ path: `${dir}/28-connect-jira.png` });
  await connect.getByRole("button", { name: "Connect" }).click();
  const map = page.getByRole("dialog", { name: "Map Jira Cloud projects" });
  await map.getByLabel("Project for Storefront").selectOption("storefront");
  await map.getByLabel("Project for Intranet").selectOption("intranet");
  await page.screenshot({ path: `${dir}/29-map-jira-projects.png` });
});

test("enterprise settings", async ({ page }) => {
  await signIn(page, "nadia");
  await page.evaluate(() =>
    fetch("/api/v1/__mock/edition", {
      method: "POST",
      body: JSON.stringify({ edition: "enterprise" }),
    }),
  );
  await page.goto("/settings");
  const signInSection = page
    .locator("section")
    .filter({
      has: page.getByRole("heading", { name: "Sign-in", exact: true }),
    });
  await signInSection
    .getByLabel("Issuer URL")
    .fill("https://login.microsoftonline.com/kestrel/v2.0");
  await signInSection.getByLabel("Client ID").fill("4b1d-tc");
  await signInSection.getByLabel("Client secret").fill("s3cret");
  await signInSection.getByRole("button", { name: "Add sign-in" }).click();
  await page.getByRole("button", { name: "New SCIM token" }).click();
  await page
    .getByRole("dialog", { name: "Copy your SCIM token now" })
    .getByRole("button", { name: "Done" })
    .click();
  await page.waitForLoadState("networkidle");
  await page.screenshot({
    path: `${dir}/30-enterprise-settings.png`,
    fullPage: true,
  });
});

test("people at the member limit", async ({ page }) => {
  await signIn(page, "nadia");
  await page.evaluate(() =>
    fetch("/api/v1/__mock/edition", {
      method: "POST",
      body: JSON.stringify({ edition: "enterprise" }),
    }),
  );
  await fillToMemberLimit(page);
  await page.goto("/people");
  await expect(
    page.getByText(/This instance allows no more members/),
  ).toBeVisible();
  await page.screenshot({ path: `${dir}/36-people-at-member-limit.png` });
});
