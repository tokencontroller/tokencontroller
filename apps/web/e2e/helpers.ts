import { expect, type Locator, type Page } from "@playwright/test";

/** People of the docs' example organization, Kestrel Digital. */
export const people = {
  mara: "m.ortner@kestrel.example", // agent manager, team web
  priya: "p.shah@kestrel.example", // people manager for team: web
  nadia: "n.berger@kestrel.example", // controller
  kai: "k.novak@kestrel.example", // people manager for client: acme
} as const;

/** Signs in the way a person does: email, then the link from the email. */
export async function signIn(page: Page, who: keyof typeof people): Promise<void> {
  await page.goto("/sign-in");
  await page.getByLabel("Work email").fill(people[who]);
  await page.getByRole("button", { name: "Email me a link" }).click();
  await expect(page.getByRole("heading", { name: "Check your email" })).toBeVisible();
  // The mock build has no inbox; it offers the link it would have sent.
  await page.getByRole("button", { name: "Open the sign-in link" }).click();
  // The link lands on the start page, which greets them.
  await expect(page).not.toHaveURL(/sign-in/);
  await expect(page.getByRole("heading", { level: 1, name: /^Hello/ })).toBeVisible();
}

/** The main menu, opened first where it is a drawer (on a phone). */
export async function mainMenu(page: Page): Promise<Locator> {
  // The app shell is on screen once the page has loaded.
  await page.getByRole("main").waitFor();
  const menu = page.getByRole("button", { name: "Menu" });
  if (await menu.isVisible()) await menu.click();
  return page.getByRole("navigation", { name: "Main" });
}

/** Opens a page from the menu, which is a drawer on a phone. */
export async function openFromMenu(page: Page, name: string): Promise<void> {
  await (await mainMenu(page)).getByRole("link", { name, exact: true }).click();
}
