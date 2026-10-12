import { expect, type Page } from "@playwright/test";

/** People of the docs' example organization, Kestrel Digital. */
export const people = {
  mara: "m.ortner@kestrel.example", // agent manager, team web
  priya: "p.nair@kestrel.example", // people manager for team: web
  dana: "d.fischer@kestrel.example", // controller
} as const;

/** Signs in the way a person does: email, then the link from the email. */
export async function signIn(page: Page, who: keyof typeof people): Promise<void> {
  await page.goto("/sign-in");
  await page.getByLabel("Work email").fill(people[who]);
  await page.getByRole("button", { name: "Email me a link" }).click();
  await expect(page.getByRole("heading", { name: "Check your email" })).toBeVisible();
  // The mock build has no inbox; it shows the link it would have sent.
  await page.getByRole("link", { name: "Open the sign-in link" }).click();
  await expect(page.getByRole("heading", { level: 1 }).first()).toBeVisible();
  await expect(page).not.toHaveURL(/sign-in/);
}

/** Opens a page from the menu, which is a drawer on a phone. */
export async function openFromMenu(page: Page, name: string): Promise<void> {
  const menu = page.getByRole("button", { name: "Menu" });
  if (await menu.isVisible()) await menu.click();
  await page.getByRole("navigation").getByRole("link", { name, exact: true }).click();
}
