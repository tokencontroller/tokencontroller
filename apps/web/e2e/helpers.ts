import { execSync } from "node:child_process";
import { expect, type Locator, type Page } from "@playwright/test";

/** People of the docs' example organization, Kestrel Digital. */
export const people = {
  mara: "m.ortner@kestrel.example", // agent manager, team web
  priya: "p.shah@kestrel.example", // people manager for team: web
  nadia: "n.berger@kestrel.example", // controller
  kai: "k.novak@kestrel.example", // people manager for client: acme
} as const;

/**
 * Signs in the way a person does: email, then the link from the email.
 *
 * Against a running service (TC_WEB_URL) there is no inbox, so set
 * TC_SIGN_IN_LINK_CMD to a command that prints the link for an address, with
 * `{email}` where the address goes, for example
 * `docker exec tc tokencontroller sign-in-link {email}`.
 */
export async function signIn(
  page: Page,
  who: keyof typeof people,
): Promise<void> {
  const linkCommand = process.env.TC_SIGN_IN_LINK_CMD;
  if (linkCommand) {
    const link =
      execSync(linkCommand.replace("{email}", people[who]), {
        encoding: "utf8",
      })
        .trim()
        .split("\n")
        .at(-1) ?? "";
    await page.goto(link);
    await page.getByRole("button", { name: "Sign in" }).click();
    await expect(page).not.toHaveURL(/sign-in/);
    await expect(
      page.getByRole("heading", { level: 1, name: /^Hello/ }),
    ).toBeVisible();
    return;
  }
  await page.goto("/sign-in");
  await page.getByLabel("Work email").fill(people[who]);
  await page.getByRole("button", { name: "Email me a link" }).click();
  await expect(
    page.getByRole("heading", { name: "Check your email" }),
  ).toBeVisible();
  // The mock build has no inbox; it offers the link it would have sent.
  await page.getByRole("button", { name: "Open the sign-in link" }).click();
  // The link lands on the start page, which greets them.
  await expect(page).not.toHaveURL(/sign-in/);
  await expect(
    page.getByRole("heading", { level: 1, name: /^Hello/ }),
  ).toBeVisible();
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

/**
 * Invites people through the API until the organization is at its member
 * limit, for screens that show the limit reached.
 */
export async function fillToMemberLimit(page: Page): Promise<void> {
  await page.evaluate(async () => {
    const limit = (await (
      await fetch("/api/v1/organization/member-limit")
    ).json()) as { members: number; member_limit: number | null };
    if (limit.member_limit === null) throw new Error("this instance has no member limit");
    for (let n = limit.members; n < limit.member_limit; n += 1) {
      const invited = await fetch("/api/v1/members", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ email: `member${String(n)}@kestrel.example` }),
      });
      if (!invited.ok)
        throw new Error(
          `invite ${String(n)} answered ${String(invited.status)}`,
        );
    }
  });
}
