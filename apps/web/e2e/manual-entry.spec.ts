import { expect, test } from "@playwright/test";
import { openFromMenu, signIn } from "./helpers";

// Docs: Collect > Manual entry and CSV import. A token entry by hand, then a
// CSV file: the import shows the problem in each row before anything is
// saved, and saves nothing until every row can be read.
test("an agent manager adds a token entry by hand and imports a CSV file", async ({ page }) => {
  await signIn(page, "mara");
  await openFromMenu(page, "Token entries");

  await page.getByRole("button", { name: "Add", exact: true }).click();
  const add = page.getByRole("dialog", { name: "Add a token entry" });
  await add.getByLabel("Model").fill("claude-sonnet-5");
  await add.getByLabel("Input").fill("41200");
  await add.getByLabel("Output").fill("6100");
  await add.getByLabel("Summary").fill("Paired on the release checklist");
  await add.getByLabel("Ticket").selectOption("KD-214");
  await add.getByRole("button", { name: "Add token entry" }).click();
  await expect(add).toBeHidden();
  await expect(page.getByText("Token entry added")).toBeVisible();

  await page.getByRole("button", { name: "Import CSV" }).click();
  const dialog = page.getByRole("dialog", { name: "Import CSV" });
  const header = "agent_harness,model,started_at,ended_at,tokens.input,tokens.output,reported_cost,summary,ticket,tags.team";
  const good = "claude-code,claude-sonnet-5,2026-10-08T09:12:00Z,2026-10-08T09:40:00Z,41200,6100,0.42,Fixed the cart total,KD-214,web";
  const bad = "claude-code,claude-sonnet-5,yesterday,2026-10-08T11:00:00Z,900,100,,Looked at logs,,web";
  await dialog.getByLabel("CSV text").fill(`${header}\n${good}\n${bad}\n`);
  await dialog.getByRole("button", { name: "Check the file" }).click();
  await expect(dialog).toContainText("1 row is ready, 1 cannot be read.");
  await expect(dialog).toContainText("Line 3: started_at and ended_at must be RFC 3339 times");
  await expect(dialog.getByRole("button", { name: /^Import / })).toHaveCount(0);

  await dialog.getByLabel("CSV text").fill(`${header}\n${good}\n`);
  await dialog.getByRole("button", { name: "Check the file" }).click();
  await dialog.getByRole("button", { name: "Import 1 token entry" }).click();
  await expect(dialog).toBeHidden();
  await expect(page.getByText("1 token entry imported")).toBeVisible();
});
