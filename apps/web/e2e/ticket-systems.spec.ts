import { expect, test } from "@playwright/test";
import { openFromMenu, signIn } from "./helpers";

// Docs: Integrations > Jira Cloud. A controller adds an API token with the
// email of the Atlassian account it belongs to, then maps each Jira project
// to a Token Controller project. Disconnecting keeps the tickets.
test("a controller connects Jira Cloud, maps its projects and disconnects it again", async ({ page }) => {
  await signIn(page, "nadia");
  await openFromMenu(page, "Settings");
  await page.getByRole("button", { name: "Connect a ticket system" }).click();

  const connect = page.getByRole("dialog", { name: "Connect a ticket system" });
  // Only Jira Cloud asks for an email; GitHub Issues starts with its API address.
  await connect.getByLabel("System").selectOption({ label: "GitHub Issues" });
  await expect(connect.getByLabel("API address")).toHaveValue("https://api.github.com");
  await expect(connect.getByLabel("Email of the Atlassian account")).toHaveCount(0);
  await connect.getByLabel("System").selectOption({ label: "Jira Cloud" });
  await expect(connect.getByLabel("Jira Cloud site")).toHaveValue("");
  await connect.getByLabel("Jira Cloud site").fill("kestrel-labs.atlassian.net");
  await connect.getByLabel("Email of the Atlassian account").fill("n.berger@kestrel.example");
  await connect.getByLabel("API token").fill("ATATT3x-labs-9fQe");
  await connect.getByRole("button", { name: "Connect" }).click();

  // Mapping opens right away: nothing syncs until a project is mapped.
  const map = page.getByRole("dialog", { name: "Map Jira Cloud projects" });
  await map.getByLabel("Project for Intranet").selectOption("intranet");
  await map.getByLabel("Project for Storefront").selectOption("storefront");
  await map.getByRole("button", { name: "Save mapping" }).click();
  await expect(map).toBeHidden();

  const labs = page.getByRole("listitem").filter({ hasText: "https://kestrel-labs.atlassian.net" });
  await expect(labs).toContainText("Storefront → storefront, Intranet → intranet");
  await expect(labs).toContainText("token …9fQe");

  await labs.getByRole("button", { name: "Disconnect" }).click();
  const leave = page.getByRole("dialog", { name: "Disconnect Jira Cloud?" });
  await expect(leave).toContainText("Its tickets stay");
  await leave.getByRole("button", { name: "Disconnect" }).click();
  await expect(labs).toHaveCount(0);
  // The first Jira site is still there.
  await expect(page.getByRole("listitem").filter({ hasText: "https://kestrel.atlassian.net" })).toBeVisible();
});
