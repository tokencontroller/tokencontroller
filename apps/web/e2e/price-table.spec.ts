import { expect, test } from "@playwright/test";
import { openFromMenu, signIn } from "./helpers";

// Docs: Price table. Managed rows are read-only; an org manager adds a row
// for a model the managed table lacks. Count errors and messages not priced yet are listed.
test("an org manager adds a price for a model not priced yet", async ({ page }) => {
  await signIn(page, "nadia");
  await openFromMenu(page, "Price table");
  const missing = page.getByRole("table", { name: "Messages without a cost" });
  await expect(missing.getByText("anthropic/claude-nova-2").filter({ visible: true })).toBeVisible();
  await expect(missing.getByText("count error").filter({ visible: true })).toBeVisible();
  await page.getByRole("button", { name: "Add a row" }).click();
  await page.getByLabel("Model").fill("claude-nova-2");
  await page.getByLabel("Price per million").fill("3.00");
  await page.getByLabel("Valid from").fill("2026-01-01");
  await page.getByRole("button", { name: "Add the row" }).click();
  await expect(page.getByText("Row added")).toBeVisible();
  await page.getByRole("button", { name: "Add a row" }).click();
  await page.getByLabel("Token kind").fill("output");
  await page.getByLabel("Price per million").fill("15.00");
  await page.getByRole("button", { name: "Add the row" }).click();
  // Both prices sit on one row for the model.
  const row = page.getByRole("table", { name: "Price table" }).locator("tr").filter({ hasText: "claude-nova-2" });
  await expect(row.getByText("your row").filter({ visible: true })).toHaveCount(1);
  await expect(row.getByText("15.00").filter({ visible: true })).toBeVisible();
  await expect(missing.getByText("anthropic/claude-nova-2")).toHaveCount(0);
});
