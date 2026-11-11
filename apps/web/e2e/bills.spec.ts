import { expect, test } from "@playwright/test";
import { openFromMenu, signIn } from "./helpers";

// Docs: Bills and reconciliation. Each March bill is checked under its
// contract: anthropic-api 270 against messages 250, 20 that no message
// explains; claude-team a fee of 200 and extra usage of 50, the plan covered
// 350. A bill below its messages is flagged.
test("an org manager reads the checks and adds a bill by hand", async ({ page }) => {
  await signIn(page, "nadia");
  await openFromMenu(page, "Bills");
  const api = page.getByRole("group", { name: /^Anthropic API/ });
  await expect(api.getByText("$270.00")).toBeVisible();
  await expect(api.getByText("$250.00")).toBeVisible();
  await expect(api.getByText("$20.00")).toBeVisible();
  const team = page.getByRole("group", { name: /^Claude Team plan/ });
  await expect(team.getByText("on a plan")).toBeVisible();
  await expect(team.getByText("$350.00")).toBeVisible();
  await expect(page.getByRole("group", { name: /^openai-api/ }).getByText("Contract only from the provider")).toBeVisible();

  await page.getByRole("button", { name: "Add a bill" }).click();
  const dialog = page.getByRole("dialog", { name: "Add a bill" });
  await dialog.getByLabel("Contract").selectOption("anthropic-api");
  await dialog.getByLabel("First day").fill("2026-03-01");
  await dialog.getByLabel("Last day").fill("2026-03-31");
  await dialog.getByLabel("Amount").fill("200");
  await dialog.getByRole("button", { name: "Add the bill" }).click();
  await expect(api.getByText("a message is wrong")).toBeVisible();
});

// Docs: Bills and reconciliation > CSV. The upload shows what it read before
// anything is saved; a file with a problem saves nothing.
test("an org manager uploads a CSV bill and sees what it holds first", async ({ page }) => {
  await signIn(page, "nadia");
  await openFromMenu(page, "Bills");
  await page.getByRole("button", { name: "Upload CSV" }).click();
  const dialog = page.getByRole("dialog", { name: "Upload a CSV bill" });
  const file = (name: string, text: string) => ({ name, mimeType: "text/csv", buffer: Buffer.from(text) });

  // A problem names its row and column, and the upload button stays off.
  await dialog.getByLabel("CSV file").setInputFiles(file("bad.csv", "provider,from,to,amount,currency\nbedrock,2026-03-01,2026-03-31,abc,USD\n"));
  await expect(dialog.getByRole("alert")).toContainText("Row 2, column amount");
  await expect(dialog.getByRole("alert")).toContainText("Nothing was saved");
  await expect(dialog.getByRole("button", { name: /^Upload the bill|^Upload \d+ bills/ })).toBeDisabled();

  // A good file: one new bill, one that is already stored with the same figures.
  await dialog.getByLabel("CSV file").setInputFiles(file("bills.csv", "provider,from,to,amount,currency\nbedrock,2026-03-01,2026-03-31,500.00,USD\nanthropic,2026-03-01,2026-03-31,270.00,USD\n"));
  const holds = dialog.getByRole("table", { name: "What the file holds" });
  await expect(holds.getByText("$500.00").filter({ visible: true })).toBeVisible();
  await expect(holds.getByText("new bill").filter({ visible: true })).toBeVisible();
  await expect(holds.getByText("same figures, nothing changes").filter({ visible: true })).toBeVisible();
  await dialog.getByRole("button", { name: "Upload 2 bills" }).click();

  // The bill is checked under a new contract: no bedrock message explains it.
  const bedrock = page.getByRole("group", { name: /^bedrock/ });
  await expect(bedrock.getByText("$500.00").first()).toBeVisible();
  await expect(bedrock.getByText("$0.00")).toBeVisible();
});

test("a cloud's own cost export is one month's bill", async ({ page }) => {
  await signIn(page, "nadia");
  await openFromMenu(page, "Bills");
  await page.getByRole("button", { name: "Upload CSV" }).click();
  const dialog = page.getByRole("dialog", { name: "Upload a CSV bill" });
  await dialog.getByLabel("The file is").selectOption("bedrock");
  await dialog.getByLabel("Month the export bills").fill("2026-03");
  const report = [
    "line_item_usage_start_date,line_item_line_item_type,line_item_product_code,line_item_unblended_cost,line_item_currency_code",
    "2026-03-02T00:00:00Z,Usage,AmazonBedrock,100.25,USD",
    "2026-03-15T00:00:00Z,Usage,AmazonBedrock,399.75,USD",
    "2026-03-31T00:00:00Z,Tax,AmazonBedrock,50.00,USD",
  ].join("\n");
  await dialog.getByLabel("CSV file").setInputFiles({ name: "cur.csv", mimeType: "text/csv", buffer: Buffer.from(report) });
  const holds = dialog.getByRole("table", { name: "What the file holds" });
  await expect(holds.getByText("aws_bedrock").filter({ visible: true })).toBeVisible();
  await expect(holds.getByText("$500.00").filter({ visible: true })).toBeVisible();
  await dialog.getByRole("button", { name: "Upload the bill" }).click();
  await expect(page.getByRole("group", { name: /^aws_bedrock/ })).toBeVisible();
});
