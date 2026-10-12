import { expect, test } from "@playwright/test";
import { openFromMenu, signIn } from "./helpers";

// Docs: Rules. Controllers change the organization's rules; everyone keeps
// their own, which run after the organization's.
test("a controller adds an organization rule, moves it up and sets how several tickets split", async ({ page }) => {
  await signIn(page, "nadia");
  await openFromMenu(page, "Rules");
  const org = page.locator("section").filter({ has: page.getByRole("heading", { name: "Organization rules" }) }).getByRole("list");
  await expect(org.getByRole("listitem")).toHaveCount(5);
  await expect(org.getByRole("listitem").first()).toContainText("finds a ticket key in the branch");

  await page.getByRole("button", { name: "New organization rule" }).click();
  const dialog = page.getByRole("dialog", { name: "New organization rule" });
  await dialog.getByLabel("Name", { exact: true }).fill("Hotfix branches are maintenance");
  await dialog.getByLabel("Text 1").fill("hotfix/");
  await dialog.getByLabel("Tag value 1").fill("maintenance");
  await dialog.getByRole("button", { name: "Add rule" }).click();
  await expect(dialog).toBeHidden();

  const hotfix = org.getByRole("listitem").filter({ hasText: "Hotfix branches are maintenance" });
  await expect(hotfix).toContainText("When Branch starts with “hotfix/”, it sets");
  await expect(org.getByRole("listitem").last()).toContainText("Hotfix branches are maintenance");
  await page.getByRole("button", { name: "Move Hotfix branches are maintenance up" }).click();
  await expect(org.getByRole("listitem").nth(4)).toContainText("Hotfix branches are maintenance");

  await page.getByLabel("When prompts name several tickets").selectOption({ label: "An equal split between the tickets" });
  await page.reload();
  await expect(page.getByLabel("When prompts name several tickets")).toHaveValue("equal_split");
  await expect(org.getByRole("listitem").nth(4)).toContainText("Hotfix branches are maintenance");
});

test("an agent manager reads the organization's rules and keeps their own", async ({ page }) => {
  await signIn(page, "mara");
  await openFromMenu(page, "Rules");
  await expect(page.getByRole("button", { name: "New organization rule" })).toHaveCount(0);
  await expect(page.getByRole("button", { name: /^Move .* up$/ }).first()).toBeVisible();
  // Only her own list has buttons: one rule, so one Remove.
  await expect(page.getByRole("button", { name: /^Remove / })).toHaveCount(1);

  await page.getByRole("button", { name: "New personal rule" }).click();
  const dialog = page.getByRole("dialog", { name: "New personal rule" });
  await dialog.getByLabel("Name", { exact: true }).fill("Docs repository is documentation");
  await dialog.getByLabel("Signal 1").selectOption({ label: "Repository" });
  await dialog.getByLabel("Test 1").selectOption({ label: "is" });
  await dialog.getByLabel("Text 1").fill("kestrel/docs");
  await dialog.getByLabel("Tag value 1").fill("documentation");
  await dialog.getByRole("button", { name: "Add rule" }).click();
  await expect(dialog).toBeHidden();
  await expect(page.getByRole("listitem").filter({ hasText: "Docs repository is documentation" })).toContainText("When Repository is “kestrel/docs”, it sets");
  await page.getByRole("button", { name: "Remove Spike folder is learning" }).click();
  await expect(page.getByRole("listitem").filter({ hasText: "Spike folder is learning" })).toHaveCount(0);
});
