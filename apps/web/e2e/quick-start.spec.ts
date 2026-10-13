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

  // Then the team: the founder invites people from People.
  await openFromMenu(page, "People");
  await expect(page.getByRole("button", { name: "Invite a person" })).toBeEnabled();
});

// Jan's report: someone without an account asks for a sign-in link. The one
// mail they get creates the organization; no second mail.
test("someone without an account creates an organization with the one link they get", async ({ page }) => {
  await page.goto("/sign-in");
  await page.getByLabel("Work email").fill("ada@harbor.example");
  await page.getByRole("button", { name: "Email me a link" }).click();
  await expect(page.getByRole("heading", { name: "Check your email" })).toBeVisible();
  await expect(page.getByText("We sent a link to ada@harbor.example")).toBeVisible();
  // The mock build has no inbox; it offers the "no account" mail's link.
  await page.getByRole("button", { name: "Open the sign-in link" }).click();

  await expect(page.getByRole("heading", { name: "Create your organization" })).toBeVisible();
  await expect(page.getByLabel("Work email")).toHaveValue("ada@harbor.example");
  await page.getByLabel("Organization", { exact: true }).fill("Harbor Labs");
  await page.getByLabel("Your name").fill("Ada Brandt");
  await page.getByLabel("I manage agents").check();
  await page.getByRole("button", { name: "Create organization" }).click();

  // Signed in at once, with the first steps for agents.
  await expect(page.getByRole("heading", { name: "Hello, Ada" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "First steps" })).toBeVisible();
  await expect(page.getByText("Submit a first token sheet")).toBeVisible();
  await expect(page).not.toHaveURL(/first_steps/);

  // Signed in, the sign-in and sign-up pages lead to the start page.
  await page.goto("/sign-in");
  await expect(page.getByRole("heading", { name: "Hello, Ada" })).toBeVisible();
  // The link worked once.
  await page.goto("/signup?token=used");
  await expect(page.getByRole("heading", { name: "Hello, Ada" })).toBeVisible();
});
