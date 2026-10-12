import { expect, test } from "@playwright/test";
import { openFromMenu, signIn } from "./helpers";

// Docs: Review and submit. The same five draft token entries as the docs page.
test("an agent manager fixes the exception and submits the token sheet", async ({ page }) => {
  await signIn(page, "mara");
  await openFromMenu(page, "Review and submit");
  await expect(page.getByRole("heading", { name: "Review and submit" })).toBeVisible();

  const drafts = page.getByRole("table", { name: "Draft token entries" }).locator("tbody tr");
  await expect(drafts).toHaveCount(5);
  await expect(page.getByText("$212.40").first()).toBeVisible();
  await expect(page.getByText("67%")).toBeVisible();

  // Number 5 has no ticket and two candidates, tickets the session named: place it on KD-231.
  const ticket = page.getByLabel("Ticket for “Address lookup edge cases”");
  await expect(ticket).toHaveValue("");
  await expect(page.getByRole("button", { name: /^Place “Address lookup edge cases” on / })).toHaveCount(2);
  await page.getByRole("button", { name: "Place “Address lookup edge cases” on KD-231" }).click();
  await expect(ticket).toHaveValue("KD-231");
  await expect(page.getByText("95%")).toBeVisible();

  await page.getByRole("button", { name: "Submit token sheet" }).click();
  await expect(page.getByTestId("submit-result")).toHaveText("submitted token sheet · 5 token entries · $212.40 · 95% on tickets");
  await page.getByRole("button", { name: "Done" }).click();
  await expect(page.getByText("Nothing to review")).toBeVisible();
});
