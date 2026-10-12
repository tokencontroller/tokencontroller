// The mock service's logic over the example organization: who sees what,
// money per split, reports, reconciliation and the close. It follows the docs
// pages so the screens behave as the real service will.

import type {
  Adjustment,
  Bill,
  BudgetVsActualRow,
  CloseCheck,
  CostKind,
  CostSheet,
  CostSheetLine,
  Filter,
  HarnessCut,
  Money,
  Outlier,
  Person,
  Reconciliation,
  ReportResult,
  Tags,
  Ticket,
  TokenBudget,
  TokenEntry,
  TokenForecast,
  TokenSheet,
} from "@/api/contract";
import { TODAY, type BillRecord, type Db, type EntryRecord } from "./seed";

export const NANOS = 1_000_000_000;

export function m(nanos: number, currency: string): Money {
  return { nanos, currency };
}

export function cur(db: Db): string {
  return db.organization.reporting_currency;
}

/** Splits an amount by shares with the largest-remainder method (ADR 0004). */
export function allocate(total: number, shares: number[]): number[] {
  const weight = shares.reduce((s, x) => s + x, 0) || 1;
  const exact = shares.map((share) => (total * share) / weight);
  const parts = exact.map((x) => Math.trunc(x));
  let rest = total - parts.reduce((s, x) => s + x, 0);
  const order = exact
    .map((x, i) => ({ i, frac: Math.abs(x - Math.trunc(x)) }))
    .sort((a, b) => b.frac - a.frac || a.i - b.i);
  const step = rest > 0 ? 1 : -1;
  for (const { i } of order) {
    if (rest === 0) break;
    parts[i] = (parts[i] ?? 0) + step;
    rest -= step;
  }
  return parts;
}

export function matches(tags: Tags, filter: Filter): boolean {
  return Object.entries(filter).every(([name, value]) => tags[name] === value);
}

export function viewer(db: Db): Person | undefined {
  return db.people.find((p) => p.id === db.session);
}

/** Whether a person may see a token entry: their own, their filters, or everything. */
export function canSee(person: Person, entry: EntryRecord): boolean {
  if (person.role === "controller") return true;
  if (entry.agent_manager === person.id) return true;
  if (person.role === "people_manager") {
    if (entry.agent_manager === null) return true;
    return entry.splits.some((split) => person.filters.some((filter) => matches(split.tags, filter)));
  }
  return false;
}

export function counts(entry: EntryRecord, approvedOnly = false): boolean {
  if (approvedOnly) return entry.status === "approved" || entry.status === "locked";
  return entry.status === "submitted" || entry.status === "approved" || entry.status === "locked";
}

export function entryValue(entry: EntryRecord): number | null {
  return entry.billed ?? entry.list;
}

export function costKind(tags: Tags, ladder: string[]): CostKind {
  if (tags.ticket) return "direct";
  if (tags.project) return "project_overhead";
  // Rungs between ticket and project, such as epic, count as project overhead.
  const projectIndex = ladder.indexOf("project");
  if (ladder.slice(1, projectIndex === -1 ? 1 : projectIndex).some((rung) => tags[rung])) return "project_overhead";
  return "organization_overhead";
}

export function costObject(tags: Tags, ladder: string[]): string {
  for (const rung of ladder) {
    const value = tags[rung];
    if (value) return `${rung}: ${value}`;
  }
  return "organization";
}

export interface SplitRow {
  entry: EntryRecord;
  tags: Tags;
  nanos: number | null;
  share: number;
}

export function splitsOf(entry: EntryRecord): SplitRow[] {
  const value = entryValue(entry);
  const parts = value === null ? entry.splits.map(() => null) : allocate(value, entry.splits.map((s) => s.share));
  return entry.splits.map((split, i) => ({ entry, tags: split.tags, nanos: parts[i] ?? null, share: split.share }));
}

export function toEntry(db: Db, entry: EntryRecord): TokenEntry {
  const currency = cur(db);
  const { ladder, cost_center } = db.tagMap;
  return {
    id: entry.id,
    period: entry.period,
    agent_harness: entry.agent_harness,
    agent_name: entry.agent_name,
    model: entry.model,
    started_at: entry.started_at,
    ended_at: entry.ended_at,
    tokens: entry.tokens,
    list_value: entry.list === null ? null : m(entry.list, currency),
    billed_value: entry.billed === null ? null : m(entry.billed, currency),
    value_label: entry.list === null && entry.billed === null ? "unknown_model" : entry.billed !== null ? "billed" : "list_estimate",
    summary: entry.summary,
    agent_manager: entry.agent_manager,
    automatic: entry.automatic,
    status: entry.status,
    reject_reason: entry.reject_reason,
    placed_by: entry.placed_by,
    candidates: entry.candidates,
    splits: splitsOf(entry).map((row) => ({
      share: row.share,
      tags: row.tags,
      value: row.nanos === null ? null : m(row.nanos, currency),
      cost_kind: costKind(row.tags, ladder),
      cost_center: row.tags[cost_center] ?? row.tags.person ?? "nobody",
      cost_object: costObject(row.tags, ladder),
    })),
    late: entry.late,
  };
}

const RATES: Record<string, [number, number]> = { USD: [1, 1], EUR: [92, 100], GBP: [79, 100], CHF: [88, 100] };

/** Converts from the reporting currency at the rate of the period's last day. */
export function convert(nanos: number, from: string, to: string): number {
  if (from === to) return nanos;
  const [fn, fd] = RATES[from] ?? [1, 1];
  const [tn, td] = RATES[to] ?? [1, 1];
  return Math.round((nanos * fd * tn) / (fn * td));
}

function sumNanos(rows: { nanos: number | null }[]): number {
  return rows.reduce((s, r) => s + (r.nanos ?? 0), 0);
}

export function visibleSplits(db: Db, person: Person, period: string, filter: Filter, approvedOnly = false): SplitRow[] {
  return db.entries
    .filter((e) => e.period === period && counts(e, approvedOnly) && canSee(person, e))
    .flatMap(splitsOf)
    .filter((row) => matches(row.tags, filter))
    .filter((row) => person.role !== "people_manager" || row.entry.agent_manager === person.id || row.entry.agent_manager === null || person.filters.some((f) => matches(row.tags, f)));
}

export function costReport(
  db: Db,
  person: Person,
  query: { period: string; filter: Filter; group_by: string; currency?: string | undefined; approved_only?: boolean },
): ReportResult {
  const from = cur(db);
  const to = query.currency || from;
  const rows = visibleSplits(db, person, query.period, query.filter, query.approved_only);
  const groups = new Map<string | null, number>();
  for (const row of rows) {
    const key = row.tags[query.group_by] ?? null;
    groups.set(key, (groups.get(key) ?? 0) + (row.nanos ?? 0));
  }
  const total = sumNanos(rows);
  const list = [...groups.entries()]
    .map(([value, nanos]) => ({ value, nanos }))
    .sort((a, b) => (a.value === null ? 1 : b.value === null ? -1 : b.nanos - a.nanos));
  return {
    period: query.period,
    currency: to,
    group_by: query.group_by,
    rows: list.map(({ value, nanos }) => ({
      value,
      amount: m(convert(nanos, from, to), to),
      share: total ? Math.round((nanos * 10000) / total) : 0,
      label: "list_estimate",
    })),
    total: m(convert(total, from, to), to),
    estimate: db.organization.cost_source === "seat_plan",
  };
}

export function harnessReport(db: Db, person: Person, period: string, by: HarnessCut): ReportResult {
  const currency = cur(db);
  const rows = visibleSplits(db, person, period, {});
  const groups = new Map<string | null, number>();
  for (const row of rows) {
    const key = by === "agent_harness" ? row.entry.agent_harness : by === "agent_name" ? row.entry.agent_name : by === "model" ? row.entry.model : row.entry.effort;
    groups.set(key, (groups.get(key) ?? 0) + (row.nanos ?? 0));
  }
  const total = sumNanos(rows);
  return {
    period,
    currency,
    group_by: by,
    rows: [...groups.entries()]
      .sort((a, b) => (a[0] === null ? 1 : b[0] === null ? -1 : b[1] - a[1]))
      .map(([value, nanos]) => ({ value, amount: m(nanos, currency), share: total ? Math.round((nanos * 10000) / total) : 0, label: "list_estimate" })),
    total: m(total, currency),
    estimate: db.organization.cost_source === "seat_plan",
  };
}

const PROVIDER_OF_BILL: Record<string, string> = {
  Anthropic: "anthropic",
  OpenAI: "openai",
  "Google Gemini": "google",
  Cursor: "cursor",
  "GitHub Copilot": "copilot",
};

export function providerKey(bill: BillRecord): string {
  return PROVIDER_OF_BILL[bill.label] ?? bill.label.toLowerCase();
}

export function toBill(db: Db, bill: BillRecord): Bill {
  const currency = cur(db);
  return {
    id: bill.id,
    provider: bill.provider,
    label: bill.label,
    period: bill.period,
    gross: m(bill.gross, currency),
    discount: m(bill.discount, currency),
    credit: m(bill.credit, currency),
    net: m(bill.net, currency),
    net_reporting: m(bill.net, currency),
    exchange_rate: null,
    source: bill.source,
    revision: bill.revision,
    received_at: bill.received_at,
    revisions: bill.revisions.map((r) => ({ ...r, net: m(r.net, currency) })),
  };
}

export function reconciliation(db: Db, period: string): Reconciliation {
  const currency = cur(db);
  const approvedOnly = db.periods.find((p) => p.id === period)?.status === "closed" && db.organization.approval;
  const bills = db.bills.filter((b) => b.period === period);
  const rows = bills.map((bill) => {
    const entries = db.entries
      .filter((e) => e.period === period && counts(e, approvedOnly) && e.provider === providerKey(bill))
      .reduce((s, e) => s + (entryValue(e) ?? 0), 0);
    return { bill: bill.id, provider: bill.provider, label: bill.label, bill_net: m(bill.net, currency), token_entries: m(entries, currency), residual: m(bill.net - entries, currency) };
  });
  const billTotal = bills.reduce((s, b) => s + b.net, 0);
  const entryTotal = rows.reduce((s, r) => s + Number(r.token_entries.nanos), 0);
  return {
    period,
    bills: rows,
    total: { bill_net: m(billTotal, currency), token_entries: m(entryTotal, currency), residual: m(billTotal - entryTotal, currency) },
    attribution: billTotal ? Math.round((entryTotal * 10000) / billTotal) : 0,
  };
}

/** Monday-to-Sunday cadence periods; the sheet is late once its period is over. */
export function cadencePeriod(iso: string): { starts: string; ends: string } {
  const date = new Date(`${iso.slice(0, 10)}T00:00:00Z`);
  const weekday = (date.getUTCDay() + 6) % 7;
  const start = new Date(date.getTime() - weekday * 86_400_000);
  const end = new Date(start.getTime() + 6 * 86_400_000);
  return { starts: start.toISOString().slice(0, 10), ends: end.toISOString().slice(0, 10) };
}

export function lateSheets(db: Db, period?: string): { person: string; starts: string; ends: string }[] {
  const seen = new Map<string, { person: string; starts: string; ends: string }>();
  for (const entry of db.entries) {
    if (entry.status !== "draft" || !entry.agent_manager) continue;
    if (period && entry.period !== period) continue;
    const cadence = cadencePeriod(entry.started_at);
    if (cadence.ends >= TODAY) continue;
    seen.set(`${entry.agent_manager}:${cadence.starts}`, { person: entry.agent_manager, ...cadence });
  }
  return [...seen.values()];
}

export function onTickets(rows: SplitRow[]): number {
  const total = sumNanos(rows);
  const direct = sumNanos(rows.filter((r) => r.tags.ticket));
  return total ? Math.round((direct * 10000) / total) : 0;
}

export function tokenSheet(db: Db, person: Person): TokenSheet {
  const currency = cur(db);
  const mine = db.entries.filter((e) => e.agent_manager === person.id && (e.status === "draft" || e.status === "rejected"));
  const drafts = mine.filter((e) => e.status === "draft");
  const rows = drafts.flatMap(splitsOf);
  const cadence = cadencePeriod(drafts[0]?.started_at ?? `${TODAY}T00:00:00Z`);
  return {
    person: person.id,
    cadence: db.cadence ?? { every: "weekly", days: null },
    starts: cadence.starts,
    ends: cadence.ends,
    late: lateSheets(db).some((s) => s.person === person.id),
    entries: mine.sort((a, b) => a.started_at.localeCompare(b.started_at)).map((e) => toEntry(db, e)),
    total: m(sumNanos(rows), currency),
    on_tickets: onTickets(rows),
  };
}

export function closeCheck(db: Db, period: string): CloseCheck {
  const currency = cur(db);
  const bills = db.bills.filter((b) => b.period === period);
  const expected = ["Anthropic", "OpenAI", "Google Gemini", "Cursor", "GitHub Copilot"];
  const billRows = expected.map((label) => {
    const bill = bills.find((b) => b.label === label);
    return { provider: bill?.provider ?? (label === "Anthropic" ? "anthropic_console" : label === "OpenAI" ? "openai" : "other"), label, received: Boolean(bill), final: bill?.final ?? false } as const;
  });
  const inPeriod = db.entries.filter((e) => e.period === period);
  const overhead = inPeriod.filter((e) => counts(e)).flatMap(splitsOf).filter((r) => costKind(r.tags, db.tagMap.ladder) === "organization_overhead");
  const unapproved = inPeriod.filter((e) => e.status === "submitted").length;
  const blocked: string[] = [];
  const missing = billRows.filter((b) => !b.received).map((b) => b.label);
  if (missing.length) blocked.push(`No bill yet from ${missing.join(", ")}.`);
  if (db.organization.approval && unapproved) blocked.push(`${unapproved} token ${unapproved === 1 ? "entry is" : "entries are"} not approved yet.`);
  const today = TODAY.slice(0, 7);
  if (period >= today) blocked.push("The period is not over yet.");
  return {
    period,
    bills: billRows,
    organization_overhead: { amount: m(sumNanos(overhead), currency), entries: new Set(overhead.map((r) => r.entry.id)).size },
    late_sheets: lateSheets(db, period),
    unapproved,
    approval: db.organization.approval,
    reconciliation: reconciliation(db, period),
    can_close: blocked.length === 0,
    blocked_by: blocked,
  };
}

interface Node {
  rung: string;
  value: string;
  own: number;
  children: Map<string, Node>;
}

function lineOf(node: Node, db: Db, currency: string): CostSheetLine {
  const children = [...node.children.values()].map((child) => lineOf(child, db, currency));
  const total = node.own + children.reduce((s, c) => s + Number(c.amount.nanos), 0);
  if (children.length && node.own) {
    children.push({ rung: node.rung, value: node.value, label: `${node.rung[0]?.toUpperCase() ?? ""}${node.rung.slice(1)} overhead`, amount: m(node.own, currency), children: [] });
  }
  children.sort((a, b) => (a.label?.endsWith("overhead") ? 1 : 0) - (b.label?.endsWith("overhead") ? 1 : 0) || a.value.localeCompare(b.value));
  return { rung: node.rung, value: node.value, label: labelOf(db, node.rung, node.value), amount: m(total, currency), children };
}

function labelOf(db: Db, rung: string, value: string): string | null {
  if (rung === "ticket") return db.tickets.find((t) => t.key === value)?.title ?? null;
  if (rung === "project") return db.projects.find((p) => p.key === value)?.name ?? null;
  return null;
}

export function costSheet(db: Db, period: string): CostSheet | null {
  const record = db.periods.find((p) => p.id === period);
  if (record?.status !== "closed" || !record.closed_at || !record.closed_by) return null;
  const currency = cur(db);
  const tagMap = record.tag_map ?? db.tagMap;
  const rows = db.entries.filter((e) => e.period === period && counts(e, db.organization.approval)).flatMap(splitsOf);
  const root: Node = { rung: "organization", value: "organization", own: 0, children: new Map() };
  const byKind: Record<CostKind, number> = { direct: 0, project_overhead: 0, organization_overhead: 0 };
  for (const row of rows) {
    const nanos = row.nanos ?? 0;
    byKind[costKind(row.tags, tagMap.ladder)] += nanos;
    let node = root;
    for (const rung of [...tagMap.ladder].reverse()) {
      const value = row.tags[rung];
      if (!value) continue;
      let child = node.children.get(`${rung}:${value}`);
      if (!child) {
        child = { rung, value, own: 0, children: new Map() };
        node.children.set(`${rung}:${value}`, child);
      }
      node = child;
    }
    node.own += nanos;
  }
  const lines = [...root.children.values()].map((n) => lineOf(n, db, currency)).sort((a, b) => a.value.localeCompare(b.value));
  if (root.own) lines.push({ rung: "organization", value: "organization", label: "Organization overhead", amount: m(root.own, currency), children: [] });
  return {
    period,
    closed_at: record.closed_at,
    closed_by: record.closed_by,
    tag_map: tagMap,
    lines,
    by_kind: { direct: m(byKind.direct, currency), project_overhead: m(byKind.project_overhead, currency), organization_overhead: m(byKind.organization_overhead, currency) },
    adjustments: db.adjustments.filter((a) => a.period === period).map((a) => toAdjustment(db, a)),
    reconciliation: reconciliation(db, period),
    approval: db.organization.approval,
  };
}

export function toAdjustment(db: Db, a: Db["adjustments"][number]): Adjustment {
  return { ...a, amount: m(a.amount, cur(db)) };
}

function money2(nanos: number): string {
  const cents = Math.round(nanos / 10_000_000);
  const sign = cents < 0 ? "-" : "";
  const abs = Math.abs(cents);
  return `${sign}${Math.floor(abs / 100)}.${String(abs % 100).padStart(2, "0")}`;
}

export function costSheetCsv(db: Db, period: string, columns: string[]): string {
  const record = db.periods.find((p) => p.id === period);
  const ladder = (record?.tag_map ?? db.tagMap).ladder;
  const rows = db.entries.filter((e) => e.period === period && counts(e, db.organization.approval)).flatMap(splitsOf);
  const groups = new Map<string, { cells: string[]; kind: CostKind; nanos: number }>();
  for (const row of rows) {
    const cells = [...[...ladder].reverse().map((rung) => row.tags[rung] ?? ""), ...columns.map((c) => row.tags[c] ?? "")];
    const kind = costKind(row.tags, ladder);
    const key = [...cells, kind].join("\u0000");
    const group = groups.get(key) ?? { cells, kind, nanos: 0 };
    group.nanos += row.nanos ?? 0;
    groups.set(key, group);
  }
  const header = [...[...ladder].reverse(), ...columns, "cost_kind", "amount", "currency"];
  const lines = [header.join(",")];
  for (const g of [...groups.values()].sort((a, b) => a.cells.join().localeCompare(b.cells.join()))) {
    lines.push([...g.cells, g.kind, money2(g.nanos), cur(db)].join(","));
  }
  for (const a of db.adjustments.filter((x) => x.period === period)) {
    lines.push([...[...ladder].reverse().map((rung) => (rung === a.rung ? a.value : "")), ...columns.map(() => ""), a.late_sheet ? "adjustment (late)" : "adjustment", money2(a.amount), cur(db)].join(","));
  }
  for (const r of reconciliation(db, period).bills) {
    lines.push([...ladder.map(() => ""), ...columns.map(() => ""), `residual ${r.label}`, money2(Number(r.residual.nanos)), cur(db)].join(","));
  }
  return lines.join("\r\n") + "\r\n";
}

export function ticketCost(db: Db, key: string): number {
  const keys = new Set([key, ...db.tickets.filter((t) => t.parent === key).map((t) => t.key)]);
  return db.entries
    .filter((e) => counts(e))
    .flatMap(splitsOf)
    .filter((r) => r.tags.ticket && keys.has(r.tags.ticket))
    .reduce((s, r) => s + (r.nanos ?? 0), 0);
}

export function toTicket(db: Db, t: Db["tickets"][number]): Ticket {
  return { key: t.key, title: t.title, description: t.description, parent: t.parent, assignee: t.assignee, project: t.project, source: t.source, cost: m(ticketCost(db, t.key), cur(db)) };
}

export function budgetActual(db: Db, filter: Filter, period: string): number {
  return db.entries
    .filter((e) => e.period === period && counts(e))
    .flatMap(splitsOf)
    .filter((r) => matches(r.tags, filter))
    .reduce((s, r) => s + (r.nanos ?? 0), 0);
}

export function visibleBudgets(db: Db, person: Person, period: string): TokenBudget[] {
  const currency = cur(db);
  return db.budgets
    .filter((b) => b.period === period)
    .filter((b) => person.role === "controller" || matches(b.filter, { person: person.id }) || person.filters.some((f) => Object.entries(f).every(([k, v]) => b.filter[k] === v) || overlaps(db, b.filter, f)))
    .map((b) => ({ id: b.id, filter: b.filter, period: b.period, amount: m(b.amount, currency), actual: m(budgetActual(db, b.filter, period), currency), from_forecast: b.from_forecast, created_by: b.created_by }));
}

function overlaps(db: Db, budget: Filter, filter: Filter): boolean {
  return db.entries.some((e) => e.splits.some((s) => matches(s.tags, budget) && matches(s.tags, filter)));
}

export function budgetVsActual(db: Db, person: Person, period: string): BudgetVsActualRow[] {
  return visibleBudgets(db, person, period).map((b) => ({
    budget: b.id,
    filter: b.filter,
    amount: b.amount,
    actual: b.actual,
    difference: m(Number(b.actual.nanos) - Number(b.amount.nanos), b.amount.currency),
  }));
}

function words(text: string): Set<string> {
  return new Set(text.toLowerCase().split(/[^a-z]+/).filter((w) => w.length > 3));
}

export function forecast(db: Db, key: string): TokenForecast | null {
  const target = db.tickets.find((t) => t.key === key);
  if (!target) return null;
  const want = words(`${target.title ?? ""} ${target.description ?? ""}`);
  const closed = new Set(db.periods.filter((p) => p.status === "closed").map((p) => p.id));
  const candidates = db.tickets
    .filter((t) => t.key !== key)
    .map((t) => {
      const summaryText = db.entries.filter((e) => closed.has(e.period) && e.splits.some((s) => s.tags.ticket === t.key)).map((e) => e.summary).join(" ");
      const have = words(`${t.title ?? ""} ${summaryText}`);
      const shared = [...want].filter((w) => have.has(w)).length;
      const cost = db.entries.filter((e) => closed.has(e.period)).flatMap(splitsOf).filter((r) => r.tags.ticket === t.key).reduce((s, r) => s + (r.nanos ?? 0), 0);
      const sameProject = t.project === target.project ? 1 : 0;
      return { t, cost, confidence: Math.min(92, 38 + shared * 18 + sameProject * 12) };
    })
    .filter((c) => c.cost > 0)
    .sort((a, b) => b.confidence - a.confidence || b.cost - a.cost)
    .slice(0, 4);
  if (!candidates.length) return null;
  const costs = candidates.map((c) => c.cost).sort((a, b) => a - b);
  const mid = Math.floor(costs.length / 2);
  const median = costs.length % 2 ? (costs[mid] ?? 0) : Math.round(((costs[mid - 1] ?? 0) + (costs[mid] ?? 0)) / 2);
  const currency = cur(db);
  return {
    ticket: key,
    matches: candidates.map((c) => ({ ticket: c.t.key, title: c.t.title, cost: m(c.cost, currency), confidence: c.confidence })),
    proposal: m(Math.round(median / 1e9) * 1e9, currency),
  };
}

export function outliers(db: Db, person: Person, period: string): Outlier[] {
  const currency = cur(db);
  const perTicket = (p: string) => {
    const map = new Map<string, number>();
    for (const row of visibleSplits(db, person, p, {})) {
      if (row.tags.ticket) map.set(row.tags.ticket, (map.get(row.tags.ticket) ?? 0) + (row.nanos ?? 0));
    }
    return map;
  };
  const past = db.periods.filter((p) => p.status === "closed" && p.id < period).flatMap((p) => [...perTicket(p.id).values()]).sort((a, b) => a - b);
  const baseline = past.length ? (past[Math.floor(past.length / 2)] ?? 0) : 0;
  if (!baseline) return [];
  return [...perTicket(period).entries()]
    .map(([ticket, cost]) => ({ ticket, title: db.tickets.find((t) => t.key === ticket)?.title ?? null, cost: m(cost, currency), baseline: m(baseline, currency), ratio: Math.round((cost * 100) / baseline) }))
    .filter((o) => o.ratio >= 160 || o.ratio <= 50)
    .sort((a, b) => Math.abs(Math.log(b.ratio / 100)) - Math.abs(Math.log(a.ratio / 100)));
}
