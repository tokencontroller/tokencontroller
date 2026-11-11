import { expect, test, type Page, type Route } from "@playwright/test";
import { signIn } from "./helpers";

// Pictures of every screen with the docs' worked month (March 2026 at
// Kestrel Digital), for people who read the product rather than run it.
// Not a test of anything: run with `pnpm screenshots`; files land in
// screenshots/ (or TC_SCREENSHOT_DIR).
//
// Against the real service (TC_WEB_URL and TC_SIGN_IN_LINK_CMD, see the
// making-launch-assets notes) start from a fresh `tokencontroller seed`.
// The spec then adds one session the price table cannot price yet, and the
// last tests take S4's contract away and give it back through the review
// list, so it runs in order and leaves the worked month as it found it.
// Against the mock build those live-only steps are skipped.
const dir = process.env.TC_SCREENSHOT_DIR ?? "screenshots";
const live = Boolean(process.env.TC_SIGN_IN_LINK_CMD);

// In order, one after the other: the last tests change the log.
test.describe.configure({ mode: "default" });

// Tall enough that a docs page's crop (the top 1,100 pixels) is all screen.
test.use({ viewport: { width: 1360, height: 1100 } });

// Every screen shows the worked month: the app's default range is last
// month, so the browser lives on 10 April 2026, as the mock build does.
test.beforeEach(async ({ page }) => {
  await page.clock.setFixedTime(new Date("2026-04-10T10:00:00Z"));
  if (live) await page.route(/\/api\/v1\/(sessions|messages)(\?|$)/, withRange);
});

/**
 * A workaround until the service and the web app agree: the review list,
 * the overview and the price table ask for sessions and messages with no
 * date range, which the mock answers and the service refuses with "from is
 * required". Against the service the spec adds the range from 1 January to
 * the browser's today. Remove it once the screens work without it.
 */
async function withRange(route: Route): Promise<void> {
  const url = new URL(route.request().url());
  if (!url.searchParams.has("from")) url.searchParams.set("from", "2026-01-01");
  if (!url.searchParams.has("to")) url.searchParams.set("to", "2026-04-10");
  await route.continue({ url: url.toString() });
}

async function shot(page: Page, name: string, fullPage = true): Promise<void> {
  await page.waitForLoadState("networkidle");
  // Toasts fade on their own; a picture should not catch one halfway.
  await expect(page.getByRole("region", { name: "Notifications" })).toHaveText("", { timeout: 8_000 }).catch(() => undefined);
  await page.screenshot({ path: `${dir}/${name}.png`, fullPage });
}

/**
 * Mara's fifth session of March, from an agent that names its model as
 * `anthropic/claude-sonnet-4-5` and sends no provider: it stays "not priced
 * yet" and asks which contract. Sending it again is the same record.
 */
async function sendUnpricedSession(page: Page): Promise<void> {
  const answer = await page.request.post("/api/v1/token-entries", {
    data: {
      agent_harness: "opencode",
      model: "anthropic/claude-sonnet-4-5",
      session_id: "seed-s5",
      started_at: "2026-03-27T15:00:00Z",
      ended_at: "2026-03-27T15:40:00Z",
      tokens: { input: 2_000_000, output: 150_000 },
      summary: "Returns portal: printable labels",
      feature: "KD-2",
      messages: [{ message_id: "1", at: "2026-03-27T15:00:00Z", tokens: { input: 2_000_000, output: 150_000 } }],
    },
  });
  expect(answer.ok(), await answer.text()).toBe(true);
}

test.beforeAll(async ({ browser }) => {
  if (!live) return;
  const page = await browser.newPage();
  await signIn(page, "mara");
  await sendUnpricedSession(page);
  await page.close();
});

const screens = [
  { who: "nadia", path: "/", name: "01-overview-org-manager" },
  { who: "mara", path: "/", name: "02-overview-agent-manager" },
  { who: "mara", path: "/sessions", name: "04-review-list" },
  { who: "priya", path: "/", name: "03-overview-people-manager" },
  { who: "nadia", path: "/bills", name: "06-bills-and-reconciliation" },
  { who: "nadia", path: "/reports", name: "07-reports-by-project" },
  { who: "priya", path: "/submissions", name: "10-submissions" },
  { who: "nadia", path: "/plans", name: "11-plans-and-estimates" },
  { who: "nadia", path: "/tags", name: "12-tags" },
  { who: "nadia", path: "/attributes", name: "13-attributes" },
  { who: "nadia", path: "/grants", name: "14-grants-and-roles" },
  { who: "nadia", path: "/rules", name: "15-rules" },
  { who: "nadia", path: "/price-table", name: "16-price-table" },
  { who: "nadia", path: "/cutoff", name: "19-cutoff" },
  { who: "nadia", path: "/settings", name: "20-settings" },
] as const;

for (const screen of screens) {
  test(screen.name, async ({ page }) => {
    await signIn(page, screen.who);
    await page.goto(screen.path);
    await page.getByRole("main").waitFor();
    await shot(page, screen.name);
  });
}

test("05-review-list-everyone", async ({ page }) => {
  await signIn(page, "nadia");
  await page.goto("/sessions");
  await page.getByRole("checkbox", { name: "Everyone I may view" }).click();
  await page.getByRole("table", { name: "Sessions" }).getByRole("columnheader", { name: "Person" }).waitFor();
  await shot(page, "05-review-list-everyone");
});

test("31-split-a-session", async ({ page }) => {
  await signIn(page, "mara");
  await page.goto("/sessions");
  // S2, the session split 60/40; the mock build's open sessions are others.
  const split = live ? page.getByRole("button", { name: "Split Shared payment component for checkout and returns" }) : page.getByRole("button", { name: /^Split / }).first();
  await split.click();
  await page.getByRole("dialog").waitFor();
  await shot(page, "31-split-a-session", false);
});

test("08-reports-by-person", async ({ page }) => {
  await signIn(page, "nadia");
  await page.goto("/reports");
  await page.getByLabel("Group by").selectOption("person");
  await page.getByRole("table", { name: "Cost report" }).getByText("Mara Ortner").waitFor();
  await shot(page, "08-reports-by-person");
});

test("09-deviation-report", async ({ page }) => {
  await signIn(page, "nadia");
  await page.goto("/reports");
  await page.getByLabel("Group by").selectOption("feature");
  await page.getByRole("tab", { name: "Deviation" }).click();
  await page.getByRole("table", { name: "Deviation report" }).waitFor();
  await shot(page, "09-deviation-report");
});

test("17-upload-csv-bill", async ({ page }) => {
  await signIn(page, "nadia");
  await page.goto("/bills");
  await page.getByRole("button", { name: "Upload CSV" }).click();
  const dialog = page.getByRole("dialog", { name: "Upload a CSV bill" });
  // Each bill with its contract.
  const text = "provider,from,to,amount,currency,contract,contract_kind\nanthropic,2026-03-01,2026-03-31,270.00,USD,anthropic-api,metered\naws_bedrock,2026-03-01,2026-03-31,500.00,USD,bedrock-api,metered\n";
  await dialog.getByLabel("CSV file").setInputFiles({ name: "bills-2026-03.csv", mimeType: "text/csv", buffer: Buffer.from(text) });
  await dialog.getByRole("table", { name: "What the file holds" }).waitFor();
  await shot(page, "17-upload-csv-bill");
});

test("18-upload-csv-bill-problem", async ({ page }) => {
  await signIn(page, "nadia");
  await page.goto("/bills");
  await page.getByRole("button", { name: "Upload CSV" }).click();
  const dialog = page.getByRole("dialog", { name: "Upload a CSV bill" });
  const text = "provider,from,to,amount,currency\naws_bedrock,2026-03-01,2026-03-31,500.00,USD\nanthropic,2026-03-01,2026-03-31,270.00 USD,USD\n";
  await dialog.getByLabel("CSV file").setInputFiles({ name: "bills-2026-03.csv", mimeType: "text/csv", buffer: Buffer.from(text) });
  await dialog.getByRole("alert").waitFor();
  await shot(page, "18-upload-csv-bill-problem");
});

test("21-add-a-bill-with-its-contract", async ({ page }) => {
  await signIn(page, "nadia");
  await page.goto("/bills");
  await page.getByRole("button", { name: "Add a bill" }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByLabel("Contract").selectOption({ label: "A new contract" });
  await dialog.getByLabel("Contract key").fill("bedrock-api");
  await shot(page, "21-add-a-bill-with-its-contract", false);
});

test("22-add-a-sign-in-rule", async ({ page }) => {
  await signIn(page, "nadia");
  await page.goto("/rules");
  await page.getByRole("button", { name: "Add a rule" }).first().click();
  const dialog = page.getByRole("dialog", { name: "Add a rule" });
  await dialog.getByLabel("Rule name").fill("signed in to the team's Claude organization");
  await dialog.getByLabel("Signal").fill("organization.id");
  await dialog.getByLabel("Pattern").fill("^kestrel-team$");
  await dialog.getByLabel("Sets tag name").selectOption("contract");
  await dialog.getByLabel("Tag value").fill("claude-team");
  await shot(page, "22-add-a-sign-in-rule", false);
});

test("23-sign-in", async ({ page }) => {
  await page.goto("/sign-in");
  await shot(page, "23-sign-in");
});

test("24-invite", async ({ page }) => {
  await page.goto("/i/kestrel-4f2a");
  await page.getByRole("heading", { name: "Join Kestrel Digital" }).waitFor();
  await shot(page, "24-invite");
});

test("25-not-found", async ({ page }) => {
  await signIn(page, "nadia");
  await page.goto("/no-such-page");
  await shot(page, "25-not-found");
});

test("26-phone-review-list", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await signIn(page, "mara");
  await page.goto("/sessions");
  await page.getByRole("main").waitFor();
  await shot(page, "26-phone-review-list");
});

test("27-dark-bills", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "dark" });
  await signIn(page, "nadia");
  await page.goto("/bills");
  await page.getByRole("group").first().waitFor();
  await shot(page, "27-dark-bills");
});

// Live only: S4 loses the contract Tom set by hand, so the checks show the
// Anthropic messages with no contract beside the line from the fallback,
// and Tom's review list asks which contract. Tom answers, and the month is
// back as it was.
test("28-bills-check-with-both-lines", async ({ page }) => {
  test.skip(!live, "needs the real service's log");
  await signIn(page, "nadia");
  const reversed = await page.request.post("/api/v1/reversals", { data: { event_id: "con_seed_s4" } });
  expect(reversed.ok(), await reversed.text()).toBe(true);
  await page.goto("/bills");
  await page.getByText("Same provider, no contract").first().waitFor();
  await shot(page, "28-bills-check-with-both-lines");
});

test("29-review-list-which-contract", async ({ page }) => {
  test.skip(!live, "needs the real service's log");
  await signIn(page, "tom");
  await page.goto("/sessions");
  await page.getByText("Which contract?").waitFor();
  await page.getByLabel("Contract for Claude Code").selectOption("anthropic-api");
  // Only these sessions: no rule for every Claude Code session of Tom's.
  await page.getByRole("checkbox", { name: "Remember" }).click();
  await shot(page, "29-review-list-which-contract");
  await page.getByRole("button", { name: "Set", exact: true }).click();
  await expect(page.getByText("Which contract?")).toBeHidden();
  await shot(page, "30-review-list-contract-set");
});
