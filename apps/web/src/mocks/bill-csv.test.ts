import { expect, test } from "vitest";
import { describeProblem, parseBills, parseCloudExport, readBillCsv } from "./bill-csv";

// Mirrors apps/server/src/integrations/bills/csv: the same reading, the same
// words for the first problem found.

test("reads bills in the bill schema's columns, commas or semicolons", () => {
  const commas = parseBills("provider,from,to,amount,currency,exchange_rate\nanthropic,2026-03-01,2026-03-31,1000.00,usd,\nbedrock,2026-03-01,2026-03-31,\"1,234.56\",USD,0.9214\n");
  expect(commas.problem).toBeNull();
  expect(commas.bills).toEqual([
    { provider: "anthropic", from: "2026-03-01", to: "2026-03-31", amount: 1000e9, currency: "USD", exchange_rate: null, lines: 0 },
    { provider: "bedrock", from: "2026-03-01", to: "2026-03-31", amount: 1234.56e9, currency: "USD", exchange_rate: "0.9214", lines: 0 },
  ]);
  const semicolons = parseBills("\uFEFFprovider;from;to;amount;currency\nopenai;2026-03-01;2026-03-31;1.234,50;EUR\n");
  expect(semicolons.bills[0]?.amount).toBe(1234.5e9);
  expect(semicolons.bills[0]?.currency).toBe("EUR");
});

test("names the first problem with its row and column", () => {
  const bad = parseBills("provider,from,to,amount,currency\nanthropic,2026-03-01,2026-03-31,abc,USD\n");
  expect(bad.problem).toEqual({ row: 2, column: "amount", message: '"abc" is not an amount. Write a plain number such as 1234.56.' });
  expect(bad.problem && describeProblem(bad.problem)).toBe('Row 2, column amount: "abc" is not an amount. Write a plain number such as 1234.56.');
  expect(parseBills("").problem?.message).toMatch(/^The file is empty/);
  expect(parseBills("provider,from,to,amount,currency\n").problem?.message).toMatch(/header but no bills/);
  expect(parseBills("provider,from,amount,currency\na,2026-03-01,1,USD\n").problem?.message).toBe("The header has no to column. A bill needs the columns provider, from, to, amount and currency, and optionally exchange_rate.");
  expect(parseBills("provider,from,to,amount,currency,note\n").problem?.message).toMatch(/not part of the bill schema/);
  expect(parseBills("provider,from,to,amount,currency\na,2026-03-31,2026-03-01,1,USD\n").problem).toEqual({ row: 2, column: "to", message: "The bill ends on 2026-03-01, before it starts on 2026-03-31." });
  const twice = parseBills("provider,from,to,amount,currency\na,2026-03-01,2026-03-31,1,USD\na,2026-03-01,2026-03-31,2,USD\n");
  expect(twice.problem).toEqual({ row: 3, column: null, message: "This is a second bill for a from 2026-03-01 to 2026-03-31; row 2 already is one. Give each bill once per provider and range." });
});

test("sums a cloud's export into one month's bill", () => {
  const report = [
    "line_item_usage_start_date,line_item_line_item_type,line_item_product_code,line_item_unblended_cost,line_item_currency_code",
    "2026-03-02T00:00:00Z,Usage,AmazonBedrock,100.25,USD",
    "2026-03-15T00:00:00Z,Usage,AmazonBedrock,399.75,USD",
    "2026-03-31T00:00:00Z,Tax,AmazonBedrock,50.00,USD",
    "2026-04-01T00:00:00Z,Usage,AmazonBedrock,10.00,USD",
  ].join("\n");
  const read = parseCloudExport("bedrock", report, "2026-03");
  expect(read.problem).toBeNull();
  expect(read.bills).toEqual([{ provider: "aws_bedrock", from: "2026-03-01", to: "2026-03-31", amount: 500e9, currency: "USD", exchange_rate: null, lines: 2 }]);
  expect(parseCloudExport("bedrock", report, "2026-05").problem?.message).toMatch(/^No row of this export is Amazon Bedrock cost in 2026-05/);
  expect(parseCloudExport("vertex", report, "2026-03").problem?.message).toMatch(/^This is not a Google Cloud billing export for Vertex AI/);
  expect(readBillCsv(report, "bedrock", null).problem?.message).toBe("Give the month the export is the bill for, such as 2026-03.");
});
