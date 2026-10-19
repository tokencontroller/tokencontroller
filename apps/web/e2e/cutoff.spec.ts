import { expect, test } from "@playwright/test";
import { openFromMenu, signIn } from "./helpers";

// Docs: Cutoff. Before moving it to the end of March: the bill's
// reconciliation, organization overhead, submissions not yet approved.
test("an org manager moves the cutoff to the end of the bill's range", async ({ page }) => {
  await signIn(page, "nadia");
  await openFromMenu(page, "Cutoff");
  await expect(page.getByText("28 Feb 2026")).toBeVisible();
  await expect(page.getByText(/what no message explains \$50\.00/)).toBeVisible();
  await expect(page.getByText(/Submissions not approved yet: Mara Ortner/)).toBeVisible();
  await expect(page.getByLabel("New cutoff")).toHaveValue("2026-03-31");
  await page.getByRole("button", { name: "Move the cutoff" }).click();
  await expect(page.getByText("Cutoff moved")).toBeVisible();
  await expect(page.getByText("31 Mar 2026").first()).toBeVisible();
});
