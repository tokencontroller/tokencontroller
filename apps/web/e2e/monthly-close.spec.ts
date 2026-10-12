import { expect, test } from "@playwright/test";
import { openFromMenu, signIn } from "./helpers";

// Docs: Monthly close. With approval on, September cannot close until every
// submitted token entry in it is approved.
test("a controller approves what is open and closes September", async ({ page }) => {
  await signIn(page, "nadia");
  await openFromMenu(page, "Close");
  await expect(page.getByRole("heading", { name: "Close September 2026" })).toBeVisible();
  await expect(page.getByText("Check the bill")).toBeVisible();
  const close = page.getByRole("button", { name: "Close September 2026" });
  await expect(close).toBeDisabled();
  await expect(page.getByText(/token entries are not approved yet/)).toBeVisible();

  // Approve them in bulk.
  await page.getByRole("link", { name: "Approve them" }).click();
  await expect(page.getByRole("heading", { name: "Approval" })).toBeVisible();
  await page.getByRole("checkbox", { name: "Select all" }).click();
  await page.getByRole("button", { name: /Approve \d+ selected/ }).click();
  await expect(page.getByText("Nothing waits for approval here.")).toBeVisible();

  await openFromMenu(page, "Close");
  await expect(close).toBeEnabled();
  await close.click();
  const confirm = page.getByRole("dialog", { name: "Close September 2026?" });
  await expect(confirm).toContainText("Bills");
  await confirm.getByRole("button", { name: "Close the period" }).click();

  // The cost sheet: closed, and token entries plus residual add up to the bills.
  await expect(page.getByRole("heading", { name: "Cost sheet, September 2026" })).toBeVisible();
  await expect(page.getByTestId("closed-block")).toContainText("Nadia Berger");
  const total = page.getByRole("table", { name: "Reconciliation" }).locator("tfoot td");
  await expect(total.first()).toHaveText("Organization");
});
