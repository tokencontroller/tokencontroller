import { expect, test } from "@playwright/test";
import { openFromMenu, signIn } from "./helpers";

// Docs: Bills and reconciliation. The March bill of 1,000: messages 950,
// 50 that no message explains. A bill below its messages is flagged.
test("an org manager reads the reconciliation and adds a bill by hand", async ({ page }) => {
  await signIn(page, "nadia");
  await openFromMenu(page, "Bills");
  const table = page.getByRole("table", { name: "Reconciliation" });
  const march = table.locator("tr").filter({ hasText: "anthropic" }).first();
  await expect(march.getByText("$1,000.00").filter({ visible: true })).toBeVisible();
  await expect(march.getByText("$950.00").filter({ visible: true })).toBeVisible();
  await expect(march.getByText("$50.00").filter({ visible: true })).toBeVisible();

  await page.getByLabel("Bill provider").fill("anthropic");
  await page.getByLabel("First day").fill("2026-03-01");
  await page.getByLabel("Last day").fill("2026-03-31");
  await page.getByLabel("Amount").fill("900");
  await page.getByRole("button", { name: "Add the bill" }).click();
  await expect(page.getByText("a message is wrong").first()).toBeVisible();
});
