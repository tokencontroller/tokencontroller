import { describe, expect, test } from "vitest";
import type { Person } from "@/api/contract";
import { createDb, MARA, type Db } from "./seed";
import { submitSheet, tokenSheetOf } from "./token-entries";

function person(db: Db, first: string): Person {
  const found = db.people.find((p) => p.name.startsWith(`${first} `));
  if (!found) throw new Error(`no ${first} in the seed`);
  return found;
}

// The mock token sheet answers as GET /token-sheets/current does, with the
// example the Review and submit docs page shows.
describe("token sheets in the example organization", () => {
  test("Mara's token sheet matches the Review and submit docs page", () => {
    const db = createDb();
    const sheet = tokenSheetOf(db, person(db, "Mara"));
    const drafts = sheet.token_entries.filter((e) => e.status === "draft");
    expect(drafts).toHaveLength(5);
    expect(drafts.map((e) => e.list_value)).toEqual(["41.87", "12.10", "88.30", "9.75", "60.38"]);
    expect(sheet.list_value).toBe("212.40");
    expect(sheet.currency).toBe("USD");
    // 142.27 of 212.40 is on tickets: 66.98 percent, in basis points rounded down.
    expect(sheet.share_on_tickets).toBe(6698);
    expect(sheet.cadence).toBe("weekly");
    expect([sheet.period_start, sheet.period_end, sheet.due_at]).toEqual(["2026-10-12", "2026-10-18", "2026-10-19T00:00:00Z"]);
    expect(sheet.late).toBe(false);
    // The unknown ticket waits for her too, though it is submitted.
    const unknown = sheet.token_entries.filter((e) => e.splits.some((s) => s.unknown_ticket));
    expect(unknown.map((e) => e.status)).toEqual(["submitted"]);
    expect(sheet.token_sheets[0]?.period_start).toBe("2026-10-05");
  });

  test("Jonas has not submitted since September: his token sheet is late", () => {
    const db = createDb();
    const jonas = person(db, "Jonas");
    expect(tokenSheetOf(db, jonas).late).toBe(true);
    const submitted = submitSheet(db, jonas, "2026-10-12T17:00:00Z");
    expect(submitted.token_sheets.map((p) => p.starts_on)).toEqual(["2026-09-28", "2026-10-05", "2026-10-12"]);
    expect(submitted.late).toBe(0);
    expect(tokenSheetOf(db, jonas).late).toBe(false);
  });

  test("a submit hands in the drafts with their list value and share on tickets", () => {
    const db = createDb();
    const mara = person(db, "Mara");
    const result = submitSheet(db, mara, "2026-10-12T17:00:00Z");
    expect(result).toMatchObject({ submitted: 5, list_value: "212.40", currency: "USD", share_on_tickets_percent: 66, late: 0 });
    expect(result.token_sheets).toEqual([{ starts_on: "2026-10-12", ends_on: "2026-10-18" }]);
    const after = tokenSheetOf(db, mara);
    expect(after.token_entries.filter((e) => e.status === "draft")).toHaveLength(0);
    expect(after.token_sheets[0]).toMatchObject({ period_start: "2026-10-12", token_entries: 5, last_submitted_at: "2026-10-12T17:00:00Z" });
    expect(db.entries.filter((e) => e.agent_manager === MARA.id && e.status === "draft")).toHaveLength(0);
  });
});
