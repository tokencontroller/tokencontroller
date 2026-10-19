import { expect, test } from "@playwright/test";
import { openFromMenu, signIn } from "./helpers";

// Docs: Reading reports; Deviation and forecast. March by project: Shop 800
// and no project 150, adding up to 950; deviation of Shop −567.50.
test("an org manager reads cost and deviation for March", async ({ page }) => {
  await signIn(page, "nadia");
  await openFromMenu(page, "Reports");
  const cost = page.getByRole("table", { name: "Cost report" });
  await expect(cost.locator("tr").filter({ hasText: "shop" }).getByText("$800.00").filter({ visible: true })).toBeVisible();
  await expect(cost.locator("tr").filter({ hasText: "no project" }).getByText("$150.00").filter({ visible: true })).toBeVisible();
  await expect(cost.getByText("$950.00").filter({ visible: true })).toBeVisible();

  await page.getByLabel("Group by").selectOption("label");
  await expect(cost.locator("tr").filter({ hasText: "Direct" }).getByText("$700.00").filter({ visible: true })).toBeVisible();

  await page.getByLabel("Group by").selectOption("project");
  await page.getByRole("tab", { name: "Deviation" }).click();
  const deviation = page.getByRole("table", { name: "Deviation report" });
  const shop = deviation.locator("tr").filter({ hasText: "shop" });
  await expect(shop.getByText("$232.50").filter({ visible: true })).toBeVisible();
  await expect(shop.getByText("−$567.50").filter({ visible: true })).toBeVisible();

  await page.getByLabel("Group by").selectOption("person");
  await expect(deviation.locator("tr").filter({ hasText: "Mara Ortner" }).getByText("$139.50").filter({ visible: true })).toBeVisible();
});

test("an agent manager sees only her own cost", async ({ page }) => {
  await signIn(page, "mara");
  await openFromMenu(page, "Reports");
  await page.getByLabel("Group by").selectOption("feature");
  const cost = page.getByRole("table", { name: "Cost report" });
  await expect(cost.locator("tr").filter({ hasText: "KD-1" }).getByText("$580.00").filter({ visible: true })).toBeVisible();
  await expect(cost.getByText("$700.00").filter({ visible: true })).toBeVisible();
});
