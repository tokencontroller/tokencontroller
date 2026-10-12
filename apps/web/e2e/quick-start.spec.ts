import { expect, test } from "@playwright/test";
import { openFromMenu } from "./helpers";

// Docs: Quick start, "I manage people": create the organization, connect the
// bill, invite people.
test("a people manager creates an organization and puts in the first bill", async ({ page }) => {
  await page.goto("/signup");
  await page.getByLabel("Organization", { exact: true }).fill("Harbor Labs");
  await page.getByLabel("Work email").fill("ada@harbor.example");
  await page.getByLabel("I manage people").check();
  await page.getByLabel("Reporting currency").selectOption("EUR");
  await page.getByRole("button", { name: "Create organization" }).click();

  await expect(page.getByRole("heading", { name: "Check your email" })).toBeVisible();
  await page.getByRole("button", { name: "Open the sign-in link" }).click();
  await expect(page.getByRole("heading", { name: "Hello", exact: true })).toBeVisible();
  // The name is optional at sign-up; left out, the start page asks once.
  await page.getByLabel("Your name").fill("Ada Brandt");
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByRole("heading", { name: "Hello, Ada" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "First steps" })).toBeVisible();
  await expect(page.getByRole("main").getByText(/Harbor Labs/)).toBeVisible();

  // The bill: here a provider with no API, typed in.
  await openFromMenu(page, "Bills");
  await page.getByRole("button", { name: "Type in" }).click();
  const dialog = page.getByRole("dialog", { name: "Type in a bill" });
  await dialog.getByLabel("Provider").selectOption("cursor");
  await dialog.getByLabel("Net").fill("1,240.50");
  await dialog.getByRole("button", { name: "Add bill" }).click();
  await expect(dialog).toBeHidden();
  await expect(page.getByRole("table", { name: /Bills/ }).getByText("Cursor")).toBeVisible();
  await expect(page.getByRole("table", { name: /Bills/ }).getByText("€1,240.50").filter({ visible: true }).first()).toBeVisible();

  // Inviting people takes the Team plan; Solo is for one person.
  await openFromMenu(page, "People");
  await expect(page.getByText("Solo is for one person")).toBeVisible();
  await expect(page.getByRole("button", { name: "Invite a person" })).toBeDisabled();
});
