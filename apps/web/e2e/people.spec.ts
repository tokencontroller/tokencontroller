import { expect, test } from "@playwright/test";
import { fillToMemberLimit, openFromMenu, signIn } from "./helpers";

// Docs: Organization settings > People, and Plans and limits > The 150-member limit.
// A controller invites people by email; everyone invited counts as a member.
test("a controller on the Team plan sees the member count and invites a person", async ({ page }) => {
  await signIn(page, "nadia");
  await openFromMenu(page, "People");
  const count = page.getByText(/^\d+ of 150 members on the Team plan\.$/);
  const before = Number(/^\d+/.exec((await count.textContent()) ?? "")?.[0]);
  expect(before).toBeGreaterThan(1);

  await page.getByRole("button", { name: "Invite a person" }).click();
  const invite = page.getByRole("dialog", { name: "Invite a person" });
  await invite.getByLabel("Name").fill("Lena Graf");
  await invite.getByLabel("Email").fill("l.graf@kestrel.example");
  await invite.getByRole("button", { name: "Send invite" }).click();
  await page.getByRole("dialog", { name: "Invite sent" }).getByRole("button", { name: "Done" }).click();
  await expect(page.getByText(`${String(before + 1)} of 150 members on the Team plan.`)).toBeVisible();
  await expect(page.getByRole("table", { name: "People" }).getByText("Lena Graf").filter({ visible: true }).first()).toBeVisible();
});

test("an Enterprise plan instance shows no plan on People and keeps inviting", async ({ page }) => {
  await signIn(page, "nadia");
  // The mock starts as the open edition; this makes it an Enterprise plan instance, without billing.
  await page.evaluate(() => fetch("/api/v1/__mock/edition", { method: "POST", body: JSON.stringify({ edition: "enterprise" }) }));
  await page.goto("/people");
  await expect(page.getByRole("table", { name: "People" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Invite a person" })).toBeEnabled();
  await expect(page.getByText(/members on the Team plan/)).toHaveCount(0);
  await expect(page.getByText("Solo is for one person")).toHaveCount(0);
});

// The example license allows 250 members. At the limit, People says so and
// no one more can be invited.
test("an Enterprise plan instance at its member limit invites no one more", async ({ page }) => {
  await signIn(page, "nadia");
  await page.evaluate(() => fetch("/api/v1/__mock/edition", { method: "POST", body: JSON.stringify({ edition: "enterprise" }) }));
  await page.goto("/people");
  await expect(page.getByRole("button", { name: "Invite a person" })).toBeEnabled();
  await expect(page.getByText(/allows no more members/)).toHaveCount(0);

  await fillToMemberLimit(page);
  await page.reload();
  await expect(page.getByText("250 of 250 members. This instance allows no more members.")).toBeVisible();
  await expect(page.getByRole("button", { name: "Invite a person" })).toBeDisabled();
  await expect(page.getByText(/members on the Team plan/)).toHaveCount(0);
});
