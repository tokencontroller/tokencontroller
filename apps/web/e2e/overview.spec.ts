import { expect, test } from "@playwright/test";
import { signIn } from "./helpers";

// Docs: How it works. The overview shows the cost of what the person may
// view over the bill's range, the reconciliation line and open deviations.
test("the org manager's overview is the worked month", async ({ page }) => {
  await signIn(page, "nadia");
  await expect(page.getByText("$950.00").first()).toBeVisible();
  await expect(page.getByText("$1,000.00")).toBeVisible();
  await expect(page.getByText("−$567.50").first()).toBeVisible();
});

test("an agent manager's overview is her own cost", async ({ page }) => {
  await signIn(page, "mara");
  await expect(page.getByText("$700.00").first()).toBeVisible();
  await expect(page.getByRole("heading", { name: "Your sessions to check" })).toBeVisible();
});
