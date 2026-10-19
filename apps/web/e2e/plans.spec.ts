import { expect, test } from "@playwright/test";
import { openFromMenu, signIn } from "./helpers";

// Docs: Plans and estimates. The accepted estimate for KD-1 is the plan;
// the estimate for KD-2 is accepted; Mara holds 0.6 of Shop.
test("an org manager accepts an estimate and adds a share", async ({ page }) => {
  await signIn(page, "nadia");
  await openFromMenu(page, "Plans and estimates");
  const table = page.getByRole("table", { name: "Estimates" });
  const kd1 = table.locator("tr").filter({ hasText: "Checkout redesign" });
  await expect(kd1.getByText("−$80.00").filter({ visible: true })).toBeVisible();
  await expect(kd1.getByText("scope").filter({ visible: true })).toBeVisible();
  await page.getByRole("button", { name: "Accept Returns portal: start a return and print the label" }).click();
  await expect(page.getByText("Accepted", { exact: true })).toBeVisible();

  await expect(page.getByRole("table", { name: "Shares" }).getByText("Mara Ortner").filter({ visible: true })).toBeVisible();
  await page.getByLabel("Share person").selectOption({ label: "Tom Lindqvist" });
  await page.getByLabel("Share project").selectOption("shop");
  await page.getByLabel("Fraction").fill("0.6");
  await page.getByLabel("Share from").fill("2026-03-01");
  await page.getByRole("button", { name: "Add the share" }).click();
  await expect(page.getByText("shares add up to 1.2").first()).toBeVisible();
});

test("a classifier estimate comes back as a distribution", async ({ page }) => {
  await signIn(page, "priya");
  await openFromMenu(page, "Plans and estimates");
  await page.getByLabel("Work").fill("Gift cards in checkout");
  await page.getByLabel("Tag set").fill("project:shop");
  await page.getByLabel("Markup, percent").fill("15");
  await page.getByRole("button", { name: "Estimate" }).click();
  await expect(page.getByText("Estimate made")).toBeVisible();
  const row = page.getByRole("table", { name: "Estimates" }).locator("tr").filter({ hasText: "Gift cards in checkout" });
  await expect(row.getByText("15 % markup inside").filter({ visible: true })).toBeVisible();
});
