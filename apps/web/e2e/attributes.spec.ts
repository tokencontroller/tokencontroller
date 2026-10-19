import { expect, test } from "@playwright/test";
import { openFromMenu, signIn } from "./helpers";

// Docs: Attributes. A team brings its department; a person gets a team from a date.
test("an org manager puts a new team on a person", async ({ page }) => {
  await signIn(page, "nadia");
  await openFromMenu(page, "Attributes");
  await page.getByRole("button", { name: "Add a team value" }).click();
  const value = page.getByRole("dialog", { name: "Add a team value" });
  await value.getByLabel("Key").fill("data");
  await value.getByLabel("Default attributes").fill("department:engineering");
  await value.getByRole("button", { name: "Save" }).click();
  await expect(page.getByRole("table", { name: "Values of team" }).getByText("data", { exact: true }).filter({ visible: true })).toBeVisible();

  await page.getByRole("button", { name: "Set an attribute for Tom Lindqvist" }).click();
  const set = page.getByRole("dialog", { name: "Set an attribute for Tom Lindqvist" });
  await set.getByLabel("Value").fill("data");
  await set.getByLabel("From").fill("2026-04-01");
  await set.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText("Attribute set")).toBeVisible();
});
