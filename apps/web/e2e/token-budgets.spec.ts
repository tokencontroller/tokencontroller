import { expect, test } from "@playwright/test";
import { openFromMenu, signIn } from "./helpers";

// Docs: Token budgets and forecasts, then Token budget vs actual, then
// Reading reports. Kai is a people manager for client: acme. A token forecast
// shows similar past tickets with a confidence and proposes their median
// cost; it becomes a token budget only when he accepts it. Each row of Token
// budget vs actual opens down to tickets and token entries. A report is a
// saved tag filter with a grouping.
test("a people manager accepts a token forecast, opens it in token budget vs actual and saves a report", async ({ page }) => {
  await signIn(page, "kai");
  await openFromMenu(page, "Token budgets");
  await expect(page.getByRole("heading", { level: 1, name: "Token budgets" })).toBeVisible();

  await page.getByLabel("Ticket for the token forecast").selectOption("KD-260");
  await page.getByRole("button", { name: "Make a token forecast" }).click();
  const matches = page.getByRole("list", { name: "Similar past tickets" });
  await expect(matches.getByRole("listitem")).toHaveCount(5);
  await expect(matches.getByRole("listitem").first()).toContainText("confidence");
  await expect(page.getByText("Proposal, the median:")).toContainText("$221.41");
  // He picks the matches that fit; the proposal is the median of those.
  await matches.getByRole("checkbox", { name: "Use KD-131" }).click();
  await expect(page.getByText("Proposal, the median:")).toContainText("$259.03");
  await page.getByRole("button", { name: "Accept as token budget" }).click();

  const budgets = page.getByRole("table", { name: "Token budgets" });
  const accepted = budgets.getByRole("row").filter({ hasText: "KD-260" });
  await expect(accepted).toContainText("All time");
  await expect(accepted).toContainText("$259.03");

  // The token forecast reads again later, with the token budget it became.
  await page.getByLabel("Ticket for the token forecast").selectOption("KD-260");
  await expect(page.getByText("accepted as a token budget")).toBeVisible();
  await expect(page.getByText("It became the token budget on ticket: KD-260")).toBeVisible();
  await expect(page.getByRole("button", { name: "Accept as token budget" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Make a new token forecast" })).toBeEnabled();

  // Token budget vs actual, opened down to the ticket and its token entries.
  await page.getByRole("link", { name: "Token budget vs actual" }).click();
  await page.getByRole("tab", { name: "Token budget vs actual" }).click();
  const report = page.getByRole("table", { name: "Token budget vs actual" });
  const row = report.getByRole("row").filter({ hasText: "ticket: KD-260" });
  await expect(row).toContainText("underrun");
  await row.getByRole("button", { name: "ticket: KD-260" }).click();
  const detail = page.getByRole("dialog", { name: "ticket: KD-260" });
  await expect(detail.getByRole("table", { name: "Tickets" }).getByText("KD-260").filter({ visible: true })).toBeVisible();
  await expect(detail.getByRole("table", { name: "Token entries" }).locator("tbody tr").first()).toBeVisible();
  await detail.getByRole("button", { name: "Close" }).click();
  await expect(detail).toBeHidden();

  // His own report: Acme's cost by model, saved.
  await page.getByRole("tab", { name: "Your reports" }).click();
  await expect(page.getByLabel("Within filter")).toHaveValue("client:acme");
  await page.getByLabel("Group by").selectOption("model");
  await expect(page.getByRole("heading", { name: "Cost by model" })).toBeVisible();
  await page.getByRole("button", { name: "Save as report" }).click();
  const save = page.getByRole("dialog", { name: "Save this report" });
  await save.getByLabel("Report name").fill("Acme cost by model");
  await save.getByRole("button", { name: "Save" }).click();
  await expect(save).toBeHidden();
  await expect(page.getByRole("list", { name: "Saved reports" }).getByRole("button", { name: "Acme cost by model", exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Acme cost by model" })).toBeVisible();
  await expect(page.getByRole("table", { name: "Report" }).getByText("Total").filter({ visible: true })).toBeVisible();
});

// Docs: Token budgets and forecasts. People managers set token budgets within
// the filters they are assigned.
test("a people manager types a token budget within their filter, and not outside it", async ({ page }) => {
  await signIn(page, "priya");
  await openFromMenu(page, "Token budgets");
  await page.getByRole("button", { name: "New token budget" }).click();
  const dialog = page.getByRole("dialog", { name: "New token budget" });
  await dialog.getByLabel(/^Name/).fill("Team web, October");
  await expect(dialog.getByLabel("Within filter")).toHaveValue("team:web");
  await dialog.getByLabel("Applies", { exact: true }).selectOption("once");
  await dialog.getByLabel("Period", { exact: true }).selectOption("2026-10");
  await dialog.getByLabel("Amount in USD").fill("1,400");
  await dialog.getByRole("button", { name: "Save token budget" }).click();
  await expect(dialog).toBeHidden();
  const row = page.getByRole("table", { name: "Token budgets" }).getByRole("row").filter({ hasText: "Team web, October" });
  await expect(row).toContainText("October 2026");
  await expect(row).toContainText("$1,400.00");

  // client: acme is outside her filter.
  await page.getByRole("button", { name: "New token budget" }).click();
  await dialog.getByRole("button", { name: "Remove team" }).click();
  await dialog.getByLabel("Tag name").selectOption("client");
  await dialog.getByLabel("Tag value").fill("acme");
  await dialog.getByRole("button", { name: "Add tag" }).click();
  await dialog.getByLabel("Amount in USD").fill("500");
  await dialog.getByRole("button", { name: "Save token budget" }).click();
  await expect(dialog.getByRole("alert")).toContainText("within the filters they are assigned");
});

// Docs: Token budgets and forecasts. A token forecast is a suggestion; an
// organization can do without them and still type token budgets.
test("a controller switches token forecasts off and on again", async ({ page }) => {
  await signIn(page, "nadia");
  await openFromMenu(page, "Token budgets");
  const forecasts = page.getByRole("switch", { name: "Token forecasts" });
  await expect(forecasts).toBeChecked();
  await forecasts.click();
  await expect(page.getByText("Token forecasts are switched off for this organization.")).toBeVisible();
  await expect(page.getByLabel("Ticket for the token forecast")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "New token budget" })).toBeEnabled();
  await forecasts.click();
  await expect(page.getByLabel("Ticket for the token forecast")).toBeVisible();
});

// Docs: Reading reports. Each person sees the reports for what their role
// covers: an agent manager their own cost and their own token budget.
test("an agent manager reads their own cost and token budget", async ({ page }) => {
  await signIn(page, "mara");
  await openFromMenu(page, "Reports");
  await expect(page.getByRole("tab", { name: "Your cost" })).toBeVisible();
  await expect(page.getByRole("table", { name: "Report" }).getByText("storefront").filter({ visible: true })).toBeVisible();
  await page.getByRole("tab", { name: "Token budget vs actual" }).click();
  const report = page.getByRole("table", { name: "Token budget vs actual" });
  await expect(report.getByRole("button", { name: "Mara, monthly" })).toBeVisible();
  await expect(report.getByRole("button", { name: "Team web, monthly" })).toHaveCount(0);
});

// Docs: Reading reports, Token budget vs actual, Outliers. Every report shows
// another currency at the rate of the period's last day. In the share view,
// a token budget's actual is a share of all the cost the person sees over the
// same period; outliers have no share view, since their ratio reads without
// money.
test("a controller reads token budget vs actual and outliers in another currency and as shares", async ({ page }) => {
  await signIn(page, "nadia");
  await page.goto("/reports?period=2026-09");
  await page.getByRole("tab", { name: "Token budget vs actual" }).click();
  const report = page.getByRole("table", { name: "Token budget vs actual" });
  const mara = report.getByRole("row").filter({ hasText: "Mara, monthly" });
  await expect(mara).toContainText("$");

  await page.getByLabel("Currency").selectOption("EUR");
  const budgets = page.getByRole("tabpanel", { name: "Token budget vs actual" });
  await expect(budgets.getByText("1 USD = 0.92 EUR")).toBeVisible();
  await expect(mara).toContainText("€");

  await page.getByRole("switch", { name: "Share view" }).click();
  await expect(budgets.getByText(/· share of the period/)).toBeVisible();
  await expect(mara.getByText(/^\d+(\.\d+)?%$/).filter({ visible: true })).toBeVisible();
  await expect(mara).not.toContainText("€");
  await expect(budgets.getByText("the shares do not add up")).toBeVisible();

  await page.getByRole("tab", { name: "Outliers" }).click();
  const outliers = page.getByRole("tabpanel", { name: "Outliers" });
  await expect(outliers.getByText("Outliers have no share view")).toBeVisible();
  await expect(outliers.getByText("1 USD = 0.92 EUR")).toBeVisible();
});

// Docs: Reading reports. Saved reports belong to the organization; whoever
// saved one, or a controller, removes it.
test("a people manager removes only the saved reports they saved", async ({ page }) => {
  await signIn(page, "kai");
  await openFromMenu(page, "Reports");
  const saved = page.getByRole("list", { name: "Saved reports" });
  await expect(saved.getByRole("button", { name: "Code review cost by project", exact: true })).toBeVisible();
  await expect(saved.getByRole("button", { name: "Actions for Code review cost by project" })).toHaveCount(0);
  await saved.getByRole("button", { name: "Actions for Cost per team for Acme" }).click();
  await page.getByRole("menuitem", { name: "Remove" }).click();
  await expect(saved.getByRole("button", { name: "Cost per team for Acme", exact: true })).toHaveCount(0);
});
