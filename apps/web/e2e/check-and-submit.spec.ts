import { expect, test } from "@playwright/test";
import { openFromMenu, signIn } from "./helpers";

// Docs: Check and submit. Mara sees her sessions since her last submit and
// how each was split, puts the one no rule matched on a feature, remembers
// it as her rule, and submits.
test("an agent manager checks her sessions, fixes one and submits", async ({ page }) => {
  await signIn(page, "mara");
  await openFromMenu(page, "Check and submit");
  const table = page.getByRole("table", { name: "Sessions" });
  const label = table.locator("tr").filter({ hasText: "Returns portal: printable label" });
  await expect(label.getByText("no project").filter({ visible: true })).toBeVisible();
  await expect(label.getByText("unknown model").filter({ visible: true })).toBeVisible();
  // The worked month's S2: 60 % KD-1, 40 % KD-2, submitted.
  const s2 = table.locator("tr").filter({ hasText: "Shared payment component" });
  await expect(s2.getByText("60 %").filter({ visible: true })).toBeVisible();
  await expect(s2.getByText("$300.00").filter({ visible: true })).toBeVisible();

  await page.getByRole("button", { name: "Split Returns portal: printable label" }).click();
  const dialog = page.getByRole("dialog", { name: "Split the session" });
  await dialog.getByLabel("Tag value 1").fill("KD-2");
  await dialog.getByRole("checkbox", { name: "Remember as my rule" }).click();
  await dialog.getByRole("button", { name: "Save split" }).click();
  await expect(page.getByText("Split saved")).toBeVisible();
  await expect(label.getByText("Direct").filter({ visible: true })).toBeVisible();

  await page.getByRole("button", { name: /^Submit 2 sessions/ }).click();
  await expect(page.getByText("Submitted", { exact: true })).toBeVisible();

  await openFromMenu(page, "Rules");
  await expect(page.getByRole("table", { name: "Your rules" }).getByText("branch returns-label is feature KD-2").filter({ visible: true })).toBeVisible();
});
