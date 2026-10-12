import { expect, test } from "@playwright/test";
import { openFromMenu, signIn } from "./helpers";

// Docs: Token entry schema. A token entry carries when the session started
// and ended and its tokens by kind. A people manager lists the token entries
// of one agent manager within their filters.
test("a people manager lists one agent manager's token entries and reads one", async ({ page }) => {
  await signIn(page, "priya");
  await openFromMenu(page, "Token entries");
  await page.getByLabel("Agent manager").selectOption({ label: "Mara Ortner" });
  const rows = page.getByRole("table", { name: "Token entries" }).locator("tbody tr");
  await expect(rows.first()).toContainText("Mara Ortner");
  await expect(rows.filter({ hasNotText: "Mara Ortner" })).toHaveCount(0);

  await rows.first().click();
  const entry = page.getByRole("dialog", { name: "Token entry" });
  await expect(entry.getByText(/ to /).first()).toBeVisible();
  await expect(entry.getByText(/\d input · [\d,]+ output/)).toBeVisible();
});

// Docs: Tags (splits) and Token entry schema. One session, several tickets:
// the agent manager splits it by hand into whole percents that add up to 100,
// and the token entry shows the shares they typed. Every member picks from
// the organization's periods, closed ones marked.
test("an agent manager splits a token entry by hand", async ({ page }) => {
  await signIn(page, "mara");
  await openFromMenu(page, "Token entries");
  await expect(page.getByLabel("Period").locator("option", { hasText: "August 2026 · closed" })).toHaveCount(1);

  const row = page.getByRole("table", { name: "Token entries" }).locator("tbody tr").filter({ hasText: "Learning the new framework" });
  await row.click();
  const entry = page.getByRole("dialog", { name: "Token entry" });
  await entry.getByRole("button", { name: "Split", exact: true }).click();
  await entry.getByLabel("Split 1 ticket").selectOption("KD-214");
  await entry.getByLabel("Split 1 share").fill("70");
  await entry.getByLabel("Split 2 ticket").selectOption("KD-231");
  await entry.getByLabel("Split 2 share").fill("30");
  await entry.getByRole("button", { name: "Save split" }).click();
  await expect(page.getByText("Split saved")).toBeVisible();
  await entry.getByRole("button", { name: "Close" }).click();
  await expect(entry).toBeHidden();

  await row.click();
  await expect(entry.getByText("2 splits")).toBeVisible();
  await expect(entry.getByText("70%", { exact: true })).toBeVisible();
  await expect(entry.getByText("30%", { exact: true })).toBeVisible();
});
