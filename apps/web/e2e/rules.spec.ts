import { expect, test } from "@playwright/test";
import { openFromMenu, signIn } from "./helpers";

// Docs: Rules. An organization rule runs back over unassigned sessions since
// the cutoff, so Tom's session in kestrel/tools lands on a project.
test("a people manager adds an organization rule", async ({ page }) => {
  await signIn(page, "nadia");
  await openFromMenu(page, "Rules");
  await expect(page.getByRole("table", { name: "Organization rules" }).getByText("feature key in branch").filter({ visible: true })).toBeVisible();
  await page.getByRole("button", { name: "Add a rule" }).first().click();
  await page.getByLabel("Rule name").fill("repository kestrel/returns is Shop");
  await page.getByLabel("Signal").fill("repository");
  await page.getByRole("dialog").getByLabel("Pattern").fill("^kestrel/returns$");
  await page.getByLabel("Sets tag name").selectOption("project");
  await page.getByLabel("Tag value").fill("shop");
  await page.getByRole("button", { name: "Add the rule" }).click();
  await expect(page.getByRole("table", { name: "Organization rules" }).getByText("repository kestrel/returns is Shop").filter({ visible: true })).toBeVisible();
});

// Docs: Rules. A rule may read any signal and set a contract; contract rules
// run back before the cutoff too.
test("a people manager remembers which contract an agent runs under", async ({ page }) => {
  await signIn(page, "nadia");
  await openFromMenu(page, "Rules");
  await expect(page.getByRole("table", { name: "Organization rules" }).getByText("contract: claude-team").filter({ visible: true })).toBeVisible();
  await page.getByRole("button", { name: "Add a rule" }).first().click();
  await page.getByLabel("Rule name").fill("Codex is on the OpenAI API");
  await page.getByLabel("Signal").fill("agent_harness");
  await page.getByRole("dialog").getByLabel("Pattern").fill("^codex$");
  await page.getByLabel("Sets tag name").selectOption("contract");
  await page.getByLabel("Tag value").fill("openai-api");
  await page.getByRole("button", { name: "Add the rule" }).click();
  await expect(page.getByRole("table", { name: "Organization rules" }).getByText("Codex is on the OpenAI API").filter({ visible: true })).toBeVisible();
});
