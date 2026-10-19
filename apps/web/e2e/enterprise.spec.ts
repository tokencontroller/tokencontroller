import { expect, test } from "@playwright/test";
import { mainMenu, signIn } from "./helpers";

// Docs: Self-hosting > OIDC and SCIM. On an Enterprise plan instance an
// org manager adds the identity provider and a SCIM token under Settings; the
// sign-in page then offers the provider's button next to the magic link.
test("an org manager sets up sign-in with the identity provider and SCIM", async ({ page }) => {
  await signIn(page, "nadia");
  // The mock starts as the open edition; this makes it an Enterprise plan instance.
  await page.evaluate(() => fetch("/api/v1/__mock/edition", { method: "POST", body: JSON.stringify({ edition: "enterprise" }) }));
  await page.goto("/settings");
  await expect(page.getByText(/Your Enterprise plan license ends on 30 November 2026/)).toBeVisible();
  await expect(page.getByRole("heading", { name: "Enterprise plan" })).toBeVisible();

  const signInSection = page.locator("section").filter({ has: page.getByRole("heading", { name: "Sign-in", exact: true }) });
  await expect(signInSection.getByLabel("Redirect URI")).toHaveValue(/\/api\/v1\/sign-in\/providers\/oidc\/callback$/);
  await signInSection.getByLabel("Issuer URL").fill("https://login.microsoftonline.com/kestrel/v2.0");
  await signInSection.getByLabel("Client ID").fill("4b1d-tc");
  await signInSection.getByLabel("Client secret").fill("s3cret");
  await signInSection.getByRole("button", { name: "Add sign-in" }).click();
  await expect(page.getByText("Sign-in saved")).toBeVisible();
  await expect(signInSection.getByRole("button", { name: "Save sign-in" })).toBeVisible();

  await page.getByRole("button", { name: "New SCIM token" }).click();
  const token = page.getByRole("dialog", { name: "Copy your SCIM token now" });
  await expect(token.getByTestId("scim-token")).toHaveText(/^tc_scim_/);
  await token.getByRole("button", { name: "Done" }).click();
  await expect(page.getByRole("table", { name: "SCIM tokens" }).locator("tbody tr")).toHaveCount(1);

  const log = page.getByRole("table", { name: "Audit log" });
  await expect(log.getByText("Created a SCIM token")).toBeVisible();
  await expect(log.getByText("Changed sign-in with the identity provider")).toBeVisible();

  await mainMenu(page);
  await page.getByRole("button", { name: "Sign out" }).click();
  await expect(page.getByRole("link", { name: "Sign in with your company account" })).toBeVisible();
});

test("the open edition shows no Enterprise plan settings", async ({ page }) => {
  await signIn(page, "nadia");
  await page.goto("/settings");
  await expect(page.getByRole("heading", { name: "Raw export" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Enterprise plan" })).toHaveCount(0);
});
