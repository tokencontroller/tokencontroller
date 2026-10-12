import { describe, expect, test } from "vitest";
import BEDROCK from "../../../server/tests/fixtures/bills/bedrock_cur2.csv?raw";
import BILLS from "../../../server/tests/fixtures/bills/bills.csv?raw";
import FOUNDRY from "../../../server/tests/fixtures/bills/foundry_cost_details.csv?raw";
import VERTEX_BIGQUERY from "../../../server/tests/fixtures/bills/vertex_bigquery_export.csv?raw";
import VERTEX_COST_TABLE from "../../../server/tests/fixtures/bills/vertex_cost_table.csv?raw";
import { fromDecimal, nanosOf } from "@/lib/money";
import { CsvBillProblem, parseBills, parseCloudExport } from "./bill-csv";
import { costSheetCsvOf, costSheetOf, periodViewOf, reconciliationOf } from "./close";
import { createDb } from "./seed";

const nanos = (amount: string, currency: string) => nanosOf(fromDecimal(amount, currency));
const added = (amounts: string[], currency: string) => amounts.reduce((total, a) => total + nanos(a, currency), 0n);

// The mocks mirror the service's account of bills and the close, so the
// screens show the figures the service would.
describe("bills and the close", () => {
  test("per bill, token entries plus residual equal the bill's gross", () => {
    const db = createDb();
    for (const period of ["2026-07", "2026-08", "2026-09"]) {
      const recon = reconciliationOf(db, period);
      expect(recon.bills.length).toBeGreaterThan(0);
      for (const check of recon.bills) {
        const c = check.currency;
        expect(nanos(check.token_entries, c) + nanos(check.residual, c)).toBe(nanos(check.gross, c));
        expect(nanos(check.gross, c) - nanos(check.discount, c) - nanos(check.credit, c)).toBe(nanos(check.net, c));
      }
    }
  });

  test("a closed period's cost sheet adds up to its bills and adjustments", () => {
    const sheet = costSheetOf(createDb(), "2026-08");
    const c = sheet.currency;
    expect(sheet.closed).toBe(true);
    for (const block of sheet.blocks) {
      expect(added([...block.lines.map((l) => l.amount), block.residual], c)).toBe(nanos(block.gross, c));
      expect(nanos(block.gross, c) - nanos(block.discount, c) - nanos(block.credit, c)).toBe(nanos(block.net, c));
    }
    const lines = added(sheet.blocks.flatMap((b) => b.lines.map((l) => l.amount)), c);
    expect(added([sheet.kinds.direct, sheet.kinds.project_overhead, sheet.kinds.organization_overhead], c)).toBe(lines);
    expect(sheet.adjustments.length).toBeGreaterThan(0);
    expect(added([...sheet.blocks.map((b) => b.net), ...sheet.adjustments.map((a) => a.amount)], c)).toBe(nanos(sheet.total, c));
  });

  test("the CSV has the extra tag columns and ends with the total", () => {
    const db = createDb();
    const csv = costSheetCsvOf(db, "2026-08", ["team"]);
    const rows = csv.trimEnd().split("\r\n");
    expect(rows[0]).toBe("bill,line,cost_kind,project,ticket,team,amount,note");
    expect(rows.at(-1)).toContain(costSheetOf(db, "2026-08").total);
  });

  test("September cannot close while token entries wait for approval", () => {
    const view = periodViewOf(createDb(), "2026-09");
    expect(view.can_close).toBe(false);
    expect(view.blockers.join(" ")).toMatch(/token entries are not approved yet/);
    expect(periodViewOf(createDb(), "2026-08").blockers[0]).toBe("2026-08 is already closed. Corrections are adjustments.");
  });
});

// The same files the service's own tests read (apps/server/tests/fixtures/bills).
describe("bill CSV files", () => {
  test("our own format reads one bill per row", () => {
    const bills = parseBills(BILLS);
    expect(bills).toHaveLength(3);
    expect(bills[0]).toMatchObject({ period: "2026-09", provider: "openai", cost_source: "api_key", currency: "USD", gross: 1_234_560_000_000n, net: 1_234_560_000_000n, exchange_rate: "0.92" });
  });

  test("a Bedrock cost report counts its own rows in the period", () => {
    const bill = parseCloudExport("bedrock", BEDROCK, "2026-09");
    expect(bill).toMatchObject({ provider: "aws_bedrock", cost_source: "cloud_marketplace", currency: "USD", gross: 54_500_000_000n, discount: 4_675_000_000n, credit: 10_000_000_000n, net: 39_825_000_000n });
    expect(bill.lines).toHaveLength(4);
    expect(bill.lines[0]).toMatchObject({ date: "2026-09-01", workspace: "111122223333", amount: 14_000_000_000n });
  });

  test("Vertex reads the cost table and the BigQuery export", () => {
    expect(parseCloudExport("vertex", VERTEX_COST_TABLE, "2026-09")).toMatchObject({ provider: "gcp_vertex", currency: "EUR", gross: 76_000_000_000n, discount: 1_500_000_000n, credit: 5_000_000_000n, net: 69_500_000_000n });
    const bigquery = parseCloudExport("vertex", VERTEX_BIGQUERY, "2026-09");
    expect(bigquery).toMatchObject({ currency: "USD", gross: 10_000_000_000n, discount: 850_000_000n, credit: 2_000_000_000n, net: 7_150_000_000n });
    expect(bigquery.lines.map((l) => l.date)).toEqual(["2026-09-01", "2026-09-30"]);
  });

  test("Foundry counts models, cognitive services and Claude", () => {
    const bill = parseCloudExport("foundry", FOUNDRY, "2026-09");
    expect(bill).toMatchObject({ provider: "azure_foundry", currency: "EUR", gross: 59_800_000_000n, discount: 0n, credit: 5_000_000_000n, net: 54_800_000_000n });
    expect(bill.lines).toHaveLength(4);
  });

  test("a problem names its row and column", () => {
    expect(() => parseBills("period,cost_source,gross,discount,credit,net,currency\n2026-09,api_key,10,0,0,9,USD\n")).toThrow(CsvBillProblem);
  });

  test("discount and credit are never below zero", () => {
    const problemOf = (text: string) => {
      try {
        parseBills(`period,cost_source,gross,discount,credit,net,currency\n${text}\n`);
      } catch (error) {
        if (error instanceof CsvBillProblem) return `Row ${String(error.row)}, column ${String(error.column)}: ${error.message}`;
      }
      return null;
    };
    expect(problemOf("2026-09,api_key,10,-5,0,15,USD")).toMatch(/^Row 2, column discount: "-5" is below zero\./);
    expect(problemOf("2026-09,api_key,10,0,-1,11,USD")).toMatch(/^Row 2, column credit: "-1" is below zero\./);
  });
});
