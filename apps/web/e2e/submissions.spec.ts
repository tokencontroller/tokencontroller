import { expect, test } from "@playwright/test";
import { openFromMenu, signIn } from "./helpers";

// Docs: Submit and approval. With approval on, Priya, people manager on
// team: web, approves Mara's March and rejects nothing of her own.
test("a people manager approves a submission", async ({ page }) => {
  await signIn(page, "priya");
  await openFromMenu(page, "Submissions");
  const table = page.getByRole("table", { name: "Submissions" });
  const mara = table.locator("tr").filter({ hasText: "Mara Ortner" });
  await expect(mara.getByText("$700.00").filter({ visible: true })).toBeVisible();
  await page.getByRole("button", { name: "Approve Mara Ortner" }).click();
  await expect(mara.getByText("approved").filter({ visible: true })).toBeVisible();
});

test("a rejected submission carries its reason", async ({ page }) => {
  await signIn(page, "priya");
  await openFromMenu(page, "Submissions");
  await page.getByRole("button", { name: "Reject Mara Ortner" }).click();
  const dialog = page.getByRole("dialog", { name: "Reject the submission" });
  await dialog.getByLabel("Reason").fill("S4 belongs on Shop.");
  await dialog.getByRole("button", { name: "Reject" }).click();
  await expect(page.getByRole("table", { name: "Submissions" }).getByText("S4 belongs on Shop.").filter({ visible: true })).toBeVisible();
});
