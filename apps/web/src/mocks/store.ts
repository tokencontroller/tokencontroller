// The mock service's logic over the example organization: who sees what,
// money per split and token sheets. It follows the docs pages so the screens
// behave as the real service will. Bills, reconciliation and the close are in
// `close.ts`, reports in `reports.ts`.

import type {
  CostKind,
  Filter,
  Money,
  Person,
  Tags,
  Ticket,
} from "@/api/contract";
import { toDecimal } from "@/lib/money";
import { TODAY, type Db, type EntryRecord } from "./seed";

export const NANOS = 1_000_000_000;

export function m(nanos: number, currency: string): Money {
  return { nanos, currency };
}

/** Nano-units as the service's decimal string, rounded to the currency's minor unit. */
export function dec(nanos: number, currency: string): string {
  return toDecimal(m(Math.round(nanos), currency));
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

export function toTicket(t: Db["tickets"][number]): Ticket {
  const ticket: Ticket = { id: t.id, key: t.key, source: t.source, tags: t.tags, updated_at: t.updated_at };
  for (const field of ["title", "description", "parent_id", "assignee_id", "project_id"] as const) {
    if (t[field] !== undefined) ticket[field] = t[field];
  }
  return ticket;
}
