import { readFile } from "node:fs/promises";
import { expect, test } from "@playwright/test";
import { openFromMenu, signIn } from "./helpers";

// Docs: Exports. Every table downloads as CSV, holding exactly what it shows.
test("a controller downloads token entries and the cost sheet as CSV", async ({ page, isMobile }) => {
  test.skip(isMobile, "Downloads are the same on a phone; once is enough.");
  await signIn(page, "nadia");

  await openFromMenu(page, "Token entries");
  await page.getByLabel("Tag filter").fill("client:acme");
  const rows = page.getByRole("table", { name: "Token entries" }).locator("tbody tr");
  await expect(rows.first()).toBeVisible();
  const shown = await rows.count();
  const entries = page.waitForEvent("download");
  await page.getByRole("button", { name: "Download CSV" }).click();
  const entriesFile = await entries;
  expect(entriesFile.suggestedFilename()).toBe("token-entries-2026-10.csv");
  const entriesCsv = await readFile(await entriesFile.path(), "utf8");
  const lines = entriesCsv.trim().split("\r\n");
  expect(lines[0]).toContain("Summary");
  // Header, one line per row shown, and the total.
  expect(lines).toHaveLength(shown + 2);
  expect(lines.slice(1, -1).every((line) => line.includes("client: acme"))).toBe(true);

  // The cost sheet with team as an extra column.
  await openFromMenu(page, "Close");
  await page.getByRole("link", { name: "Cost sheet" }).first().click();
  await expect(page.getByRole("heading", { name: "Cost sheet, August 2026" })).toBeVisible();
  await page.getByRole("button", { name: "Download CSV" }).first().click();
  const dialog = page.getByRole("dialog", { name: "Download the cost sheet as CSV" });
  await dialog.getByRole("checkbox", { name: "team" }).click();
  const sheet = page.waitForEvent("download");
  await dialog.getByRole("button", { name: "Download CSV" }).click();
  const sheetFile = await sheet;
  expect(sheetFile.suggestedFilename()).toBe("cost-sheet-2026-08.csv");
  const sheetCsv = await readFile(await sheetFile.path(), "utf8");
  expect(sheetCsv.split(/\r?\n/)[0]).toContain("team");
});
