import { expect, test } from "@playwright/test";
import { openFromMenu, signIn } from "./helpers";

// Docs: Organization settings > People, and Plans and limits > The 150-member limit.
// A controller invites people by email; everyone invited counts as a member.
test("a controller invites a person by email", async ({ page }) => {
  await signIn(page, "nadia");
  await openFromMenu(page, "People");
  await expect(page.getByText(/allows no more members/)).toHaveCount(0);

  await page.getByRole("button", { name: "Invite a person" }).click();
  const invite = page.getByRole("dialog", { name: "Invite a person" });
  await invite.getByLabel("Name").fill("Lena Graf");
  await invite.getByLabel("Email").fill("l.graf@kestrel.example");
  await invite.getByRole("button", { name: "Send invite" }).click();
  await page.getByRole("dialog", { name: "Invite sent" }).getByRole("button", { name: "Done" }).click();
  await expect(page.getByRole("table", { name: "People" }).getByText("Lena Graf").filter({ visible: true }).first()).toBeVisible();
});

// An open instance has no member limit.
test("an open instance has no member limit", async ({ page }) => {
  await signIn(page, "nadia");
  const limit = await page.evaluate(async () => (await fetch("/api/v1/organization/member-limit")).json() as Promise<{ member_limit: number | null }>);
  expect(limit.member_limit).toBeNull();
  await page.goto("/people");
  await expect(page.getByRole("button", { name: "Invite a person" })).toBeEnabled();
  await expect(page.getByText(/members\. /)).toHaveCount(0);
});

// The Enterprise plan has no member limit either.
test("an Enterprise plan instance has no member limit", async ({ page }) => {
  await signIn(page, "nadia");
  // The mock starts as the open edition; this makes it an Enterprise plan instance.
  await page.evaluate(() => fetch("/api/v1/__mock/edition", { method: "POST", body: JSON.stringify({ edition: "enterprise" }) }));
  const limit = await page.evaluate(async () => (await fetch("/api/v1/organization/member-limit")).json() as Promise<{ member_limit: number | null }>);
  expect(limit.member_limit).toBeNull();
  await page.goto("/people");
  await expect(page.getByRole("button", { name: "Invite a person" })).toBeEnabled();
  await expect(page.getByText(/allows no more members/)).toHaveCount(0);
});
