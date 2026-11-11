import { describe, expect, test } from "vitest";
import { costOf, dec, lines, planOf, sessionView, visibleLines } from "./fold";
import { createDb, emails } from "./seed";

// The mocks answer the worked month of the docs (How it works, A worked
// month), so the screens and screenshots show its numbers.
describe("the worked month in the mocks", () => {
  const db = createDb();
  const nadia = db.people.find((p) => p.email === emails.nadia);
  const march = { from: "2026-03-01", to: "2026-03-31" };
  const cost = (filter: Record<string, string>) => dec(costOf(lines(db).filter((l) => l.day >= march.from && l.day <= march.to && Object.entries(filter).every(([k, v]) => l.tags[k] === v))));

  test("messages cost 950, sliced every way the docs slice them", () => {
    expect(cost({})).toBe("950.00");
    expect(cost({ person: emails.mara })).toBe("700.00");
    expect(cost({ person: emails.tom })).toBe("250.00");
    expect(cost({ feature: "KD-1" })).toBe("580.00");
    expect(cost({ feature: "KD-2" })).toBe("120.00");
    expect(cost({ project: "shop" })).toBe("800.00");
    expect(cost({ client: "acme" })).toBe("800.00");
    expect(cost({ team: "web" })).toBe("950.00");
    expect(cost({ department: "engineering" })).toBe("950.00");
  });

  test("an org manager views all of it", () => {
    if (!nadia) throw new Error("no Nadia in the seed");
    expect(dec(costOf(visibleLines(db, nadia, {}, march)))).toBe("950.00");
  });

  test("plans and shares", () => {
    expect(dec(planOf(db, { project: "shop" }, march) ?? 0)).toBe("232.50");
    expect(dec(planOf(db, { feature: "KD-1" }, march) ?? 0)).toBe("500.00");
    expect(dec(planOf(db, { person: emails.mara }, march) ?? 0)).toBe("139.50");
  });

  test("each session's contract, as the docs find it", () => {
    const byContract = (key: string | null) => dec(costOf(lines(db).filter((l) => l.day <= march.to && (l.contract?.key ?? null) === key)));
    expect(byContract("claude-team")).toBe("400.00");
    expect(byContract("openai-api")).toBe("300.00");
    expect(byContract("anthropic-api")).toBe("250.00");
    const how = Object.fromEntries(lines(db).filter((l) => l.day <= march.to).map((l) => [l.session.session_id, l.contract?.by]));
    expect(how).toEqual({ "seed-s1": "rule", "seed-s2": "fallback", "seed-s3": "rule", "seed-s4": "hand" });
  });

  test("open April sessions on Anthropic have no contract yet", () => {
    const april = db.sessions.filter((s) => s.started_at >= "2026-04-01").map((s) => sessionView(db, s));
    expect(april.every((s) => s.contracts.length === 0)).toBe(true);
    expect(april.map((s) => s.no_contract)).toEqual([2, 1, 1]);
  });
});
