import { expect, test } from "@playwright/test";
import { openFromMenu, signIn } from "./helpers";

// Docs: Grants and roles. Roles bundle grants; an org manager hands on one
// capability on a filter, and takes it away again with a reversal.
test("an org manager hands on view on a client", async ({ page }) => {
  await signIn(page, "nadia");
  await openFromMenu(page, "Grants and roles");
  await expect(page.getByRole("table", { name: "People and roles" }).getByText("Priya Raman").filter({ visible: true })).toBeVisible();
  await page.getByRole("button", { name: "Hand on a grant" }).click();
  const dialog = page.getByRole("dialog", { name: "Hand on a grant" });
  await dialog.getByLabel("Person").selectOption({ label: "Tom Lindqvist" });
  await dialog.getByLabel("Capability").selectOption("view");
  await dialog.getByLabel("On filter").fill("client:acme");
  await dialog.getByRole("button", { name: "Grant" }).click();
  const row = page.getByRole("table", { name: "Grants" }).locator("tr").filter({ hasText: "client: acme" });
  await expect(row.getByText("Tom Lindqvist").filter({ visible: true })).toBeVisible();
  await row.getByRole("button", { name: "Take away" }).click();
  await expect(page.getByText("Grant taken away")).toBeVisible();
});
