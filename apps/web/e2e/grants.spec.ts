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

// A people manager is granted on filters: picking the role asks for one.
test("an org manager makes someone a people manager on a team", async ({ page }) => {
  await signIn(page, "nadia");
  await openFromMenu(page, "Grants and roles");
  await page.getByLabel("Role of Tom Lindqvist").filter({ visible: true }).selectOption("people_manager");
  const dialog = page.getByRole("dialog", { name: "Make a people manager" });
  await dialog.getByLabel("On filters").fill("team:web");
  await dialog.getByRole("button", { name: "Make people manager" }).click();
  await expect(page.getByText("Role changed")).toBeVisible();
  const row = page.getByRole("table", { name: "People and roles" }).locator("tr").filter({ hasText: "Tom Lindqvist" });
  await expect(row.getByText("team: web").filter({ visible: true })).toBeVisible();
});
