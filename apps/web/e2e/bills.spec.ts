import { expect, test } from "@playwright/test";
import { openFromMenu, signIn } from "./helpers";

// Docs: Bills and reconciliation. The March bill of 1,000: messages 950,
// 50 that no message explains. A bill below its messages is flagged.
test("an org manager reads the reconciliation and adds a bill by hand", async ({ page }) => {
  await signIn(page, "nadia");
  await openFromMenu(page, "Bills");
  const table = page.getByRole("table", { name: "Reconciliation" });
  const march = table.locator("tr").filter({ hasText: "anthropic" }).first();
  await expect(march.getByText("$1,000.00").filter({ visible: true })).toBeVisible();
  await expect(march.getByText("$950.00").filter({ visible: true })).toBeVisible();
  await expect(march.getByText("$50.00").filter({ visible: true })).toBeVisible();

  await page.getByLabel("Bill provider").fill("anthropic");
  await page.getByLabel("First day").fill("2026-03-01");
  await page.getByLabel("Last day").fill("2026-03-31");
  await page.getByLabel("Amount").fill("900");
  await page.getByRole("button", { name: "Add the bill" }).click();
  await expect(page.getByText("a message is wrong").first()).toBeVisible();
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
  await dialog.getByLabel("CSV file").setInputFiles(file("bills.csv", "provider,from,to,amount,currency\nbedrock,2026-03-01,2026-03-31,500.00,USD\nanthropic,2026-03-01,2026-03-31,1000.00,USD\n"));
  const holds = dialog.getByRole("table", { name: "What the file holds" });
  await expect(holds.getByText("$500.00").filter({ visible: true })).toBeVisible();
  await expect(holds.getByText("new bill").filter({ visible: true })).toBeVisible();
  await expect(holds.getByText("same figures, nothing changes").filter({ visible: true })).toBeVisible();
  await dialog.getByRole("button", { name: "Upload 2 bills" }).click();

  // The bill is in the reconciliation: no bedrock message explains it.
  const table = page.getByRole("table", { name: "Reconciliation" });
  const bedrock = table.locator("tr").filter({ hasText: "bedrock" }).first();
  await expect(bedrock.getByText("$500.00").filter({ visible: true }).first()).toBeVisible();
  await expect(bedrock.getByText("$0.00").filter({ visible: true })).toBeVisible();
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
  await expect(page.getByRole("table", { name: "Reconciliation" }).locator("tr").filter({ hasText: "aws_bedrock" }).first()).toBeVisible();
});
