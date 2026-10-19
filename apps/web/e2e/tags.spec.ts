import { expect, test } from "@playwright/test";
import { openFromMenu, signIn } from "./helpers";

// Docs: Tags. Tag values are records with default tags; a feature brings its
// project. Jira's sync map maps a Jira project onto a project.
test("an org manager adds a feature with its project and maps Jira", async ({ page }) => {
  await signIn(page, "nadia");
  await openFromMenu(page, "Tags");
  const features = page.getByRole("table", { name: "Values of feature" });
  await expect(features.getByText("Checkout redesign").filter({ visible: true })).toBeVisible();
  await page.getByRole("button", { name: "Add a feature value" }).click();
  const dialog = page.getByRole("dialog", { name: "Add a feature value" });
  await dialog.getByLabel("Key").fill("KD-3");
  await dialog.getByLabel("Title").fill("Gift cards");
  await dialog.getByLabel("Default tags").fill("project:shop");
  await dialog.getByRole("button", { name: "Save" }).click();
  await expect(features.getByText("Gift cards").filter({ visible: true })).toBeVisible();

  await page.getByLabel("Jira site").fill("https://kestrel.atlassian.net");
  await page.getByLabel("Atlassian account email").fill("n.varga@kestrel.example");
  await page.getByLabel("API token").fill("atl-token-1234");
  await page.getByRole("button", { name: "Connect Jira" }).click();
  await page.getByLabel("Project for KD").selectOption("shop");
  await page.getByRole("button", { name: "Save the sync map" }).click();
  await expect(page.getByText("Sync map saved")).toBeVisible();
});
