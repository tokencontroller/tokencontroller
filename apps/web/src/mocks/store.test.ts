import { describe, expect, test } from "vitest";
import { createDb } from "./seed";
import { allocate, closeCheck, costSheet, reconciliation } from "./store";

// The mocks stand in for the service until its API lands, so they must keep
// the rules the docs promise; otherwise the screens would show wrong numbers.
describe("example organization", () => {
  test("a split always adds up to the whole amount", () => {
    expect(allocate(1000, [3333, 3333, 3334])).toEqual([333, 333, 334]);
    const parts = allocate(41_870_000_001, [6000, 4000]);
    expect(parts.reduce((a, b) => a + b, 0)).toBe(41_870_000_001);
  });

  test("per bill, token entries plus residual equal the bill", () => {
    const db = createDb();
    for (const period of ["2026-07", "2026-08", "2026-09"]) {
      const recon = reconciliation(db, period);
      for (const line of recon.bills) {
        expect(Number(line.token_entries.nanos) + Number(line.residual.nanos)).toBe(Number(line.bill_net.nanos));
      }
      expect(Number(recon.total.token_entries.nanos) + Number(recon.total.residual.nanos)).toBe(Number(recon.total.bill_net.nanos));
    }
  });

  test("a closed period's cost sheet adds up to its bills", () => {
    const db = createDb();
    const sheet = costSheet(db, "2026-08");
    if (!sheet) throw new Error("August is closed in the seed");
    const kinds = Object.values(sheet.by_kind).reduce((total, amount) => total + Number(amount.nanos), 0);
    expect(kinds).toBe(Number(sheet.reconciliation.total.token_entries.nanos));
    expect(costSheet(db, "2026-10")).toBeNull();
  });

  test("September cannot close while token entries wait for approval", () => {
    const check = closeCheck(createDb(), "2026-09");
    expect(check.can_close).toBe(false);
    expect(check.blocked_by.join(" ")).toMatch(/not approved/);
  });
});
