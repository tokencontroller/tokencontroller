import { expect, test } from "@playwright/test";
import { openFromMenu, signIn } from "./helpers";

// Docs: Rules. An organization rule runs back over unassigned sessions since
// the cutoff, so Tom's session in kestrel/tools lands on a project.
test("a people manager adds an organization rule", async ({ page }) => {
  await signIn(page, "nadia");
  await openFromMenu(page, "Rules");
  await expect(page.getByRole("table", { name: "Organization rules" }).getByText("feature key in branch").filter({ visible: true })).toBeVisible();
  await page.getByLabel("Rule name").fill("repository kestrel/returns is Shop");
  await page.getByLabel("Signal").selectOption("repository");
  await page.getByLabel("Pattern", { exact: true }).fill("^kestrel/returns$");
  await page.getByLabel("Sets tag name").selectOption("project");
  await page.getByLabel("Tag value").fill("shop");
  await page.getByRole("button", { name: "Add the rule" }).click();
  await expect(page.getByRole("table", { name: "Organization rules" }).getByText("repository kestrel/returns is Shop").filter({ visible: true })).toBeVisible();
});
