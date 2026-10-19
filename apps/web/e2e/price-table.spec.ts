import { expect, test } from "@playwright/test";
import { openFromMenu, signIn } from "./helpers";

// Docs: Price table. Managed rows are read-only; an org manager adds a row
// for a model the managed table lacks. Count errors and unknown models are listed.
test("an org manager adds a price for an unknown model", async ({ page }) => {
  await signIn(page, "nadia");
  await openFromMenu(page, "Price table");
  const missing = page.getByRole("table", { name: "Messages without a cost" });
  await expect(missing.getByText("anthropic/claude-nova-2").filter({ visible: true })).toBeVisible();
  await expect(missing.getByText("count error").filter({ visible: true })).toBeVisible();
  await page.getByLabel("Model").fill("claude-nova-2");
  await page.getByLabel("Price per million").fill("3.00");
  await page.getByLabel("Valid from").fill("2026-01-01");
  await page.getByRole("button", { name: "Add the row" }).click();
  await expect(page.getByText("Row added")).toBeVisible();
  await page.getByLabel("Token kind").selectOption("output");
  await page.getByLabel("Price per million").fill("15.00");
  await page.getByRole("button", { name: "Add the row" }).click();
  await expect(page.getByRole("table", { name: "Price table" }).getByText("your row").filter({ visible: true })).toHaveCount(2);
  await expect(missing.getByText("anthropic/claude-nova-2")).toHaveCount(0);
});
