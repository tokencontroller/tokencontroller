// The mock service's fold of the example organization: the cost of a
// message from the price table, the full tag set of a split, C(F, w), plans,
// shares and deviations. It follows the docs' formulas so the screens show
// what the service computes (docs: How it works, Plans and estimates).

import type { Member, Part, Session, Submitted, Tags } from "@/api/contract";
import { toDecimal } from "@/lib/money";
import type { Db, MockEstimate, MockMessage, MockSession } from "./seed";

export const NANOS = 1e9;

/** Nanos of US dollars as the service's decimal string. */
export function dec(nanos: number): string {
  return toDecimal({ nanos: Math.round(nanos), currency: "USD" });
}

export function parseDec(text: string): number {
  return Math.round(Number(text) * NANOS);
}

export function filterKey(tags: Tags): string {
  return Object.keys(tags)
    .sort()
    .map((k) => `${k}:${tags[k] ?? ""}`)
    .join(",");
}

export function contains(tags: Tags, filter: Tags): boolean {
  return Object.entries(filter).every(([name, value]) => tags[name] === value);
}

const KINDS = ["input", "output", "cache_read", "cache_write"] as const;

/** The price per million in nanos for a kind on a day; an organization row wins over a managed one. */
function priceOf(db: Db, m: MockMessage, kind: string): number | null {
  const day = m.at.slice(0, 10);
  const rows = db.prices
    .filter((r) => r.provider === m.provider && r.model === m.model && r.kind === kind && r.valid_from <= day)
    .sort((a, b) => a.valid_from.localeCompare(b.valid_from) || (a.source === "organization" ? 1 : -1));
  const row = rows.at(-1);
  return row ? parseDec(row.price_per_million) : null;
}

/** C(e): `null` with the reason when there is none, never zero. */
export function messageCost(db: Db, m: MockMessage): { nanos: number | null; label: "priced" | "count_error" | "unknown_model" } {
  if (!m.tokens) return { nanos: null, label: "count_error" };
  let total = 0;
  for (const kind of KINDS) {
    const count = m.tokens[kind];
    if (!count) continue;
    const p = priceOf(db, m, kind);
    if (p === null) return { nanos: null, label: "unknown_model" };
    total += (count * p) / 1_000_000;
  }
  if (!KINDS.some((k) => priceOf(db, m, k) !== null)) return { nanos: null, label: "unknown_model" };
  return { nanos: total, label: "priced" };
}

/** A person's attributes, with their defaults. */
export function attributesOf(db: Db, email: string): Tags {
  const person = db.people.find((p) => p.email === email);
  return closeAttributes(db, person?.attributes ?? {});
}

export function closeAttributes(db: Db, attributes: Tags): Tags {
  const out: Tags = { ...attributes };
  for (let i = 0; i < 4; i += 1) {
    for (const [name, key] of Object.entries(out)) {
      const v = db.attributeValues.find((a) => a.name === name && a.key === key);
      for (const [dn, dk] of Object.entries(v?.defaults ?? {})) if (!(dn in out)) out[dn] = dk;
    }
  }
  return out;
}

/** T*(s): the split's tags closed under defaults, with the person and their attributes. */
export function fullTags(db: Db, tags: Tags, email: string): Tags {
  const out: Tags = { ...tags };
  for (let i = 0; i < 4; i += 1) {
    for (const [name, key] of Object.entries(out)) {
      const v = db.tagValues.find((t) => t.name === name && t.key === key);
      for (const [dn, dk] of Object.entries(v?.defaults ?? {})) if (!(dn in out)) out[dn] = dk;
    }
  }
  return { ...out, person: email, ...attributesOf(db, email) };
}

export function labelOf(tags: Tags): Part["label"] {
  if (tags.feature) return "direct";
  if (tags.project) return "project_overhead";
  return "organization_overhead";
}

/** The split in force on a session: a hand split, or one part on what the rules found. */
export function splitOf(db: Db, s: MockSession): { parts: { bp: number; tags: Tags }[]; by: "hand" | "rule" | "default" } {
  const hand = db.splits.filter((x) => x.session_id === s.session_id).at(-1);
  if (hand) return { parts: hand.parts, by: "hand" };
  const keys = Array.isArray(s.signals.keys) ? (s.signals.keys as string[]) : [];
  if (!s.ruleTags.feature && keys.length > 1) {
    // Several features named and nobody decided: equal fractions.
    const bp = Math.floor(10_000 / keys.length);
    return { parts: keys.map((key, i) => ({ bp: i === 0 ? 10_000 - bp * (keys.length - 1) : bp, tags: { feature: key } })), by: "default" };
  }
  return { parts: [{ bp: 10_000, tags: s.ruleTags }], by: Object.keys(s.ruleTags).length ? "rule" : "default" };
}

/** One line of the fold: a fraction of one message on one tag set. */
export interface Line {
  session: MockSession;
  message: MockMessage;
  bp: number;
  assigned: Tags;
  tags: Tags;
  nanos: number | null;
  costLabel: string;
  day: string;
}

export function lines(db: Db): Line[] {
  const out: Line[] = [];
  for (const s of db.sessions) {
    const split = splitOf(db, s);
    for (const m of s.messages) {
      const cost = messageCost(db, m);
      for (const part of split.parts) {
        out.push({
          session: s,
          message: m,
          bp: part.bp,
          assigned: part.tags,
          tags: fullTags(db, part.tags, s.person),
          nanos: cost.nanos === null ? null : (cost.nanos * part.bp) / 10_000,
          costLabel: cost.label,
          day: m.at.slice(0, 10),
        });
      }
    }
  }
  return out;
}

/** The filters a person views: their role's and their grants of view. */
export function viewFilters(db: Db, me: Member): Tags[] {
  if (me.role === "org_manager") return [{}];
  const own = [{ person: me.email }];
  const granted = db.grants.filter((g) => g.person === me.email && g.capability === "view").map((g) => g.filter);
  return me.role === "people_manager" ? [...own, ...me.filters.map((f) => f), ...granted] : [...own, ...granted];
}

export function canSee(db: Db, me: Member, tags: Tags): boolean {
  return viewFilters(db, me).some((f) => contains(tags, f));
}

/** Where a person's session stands with submit and approval. */
export function submittedOf(db: Db, s: MockSession): Submitted {
  const day = s.started_at.slice(0, 10);
  const sub = db.submissions.find((x) => x.person === s.person && x.from <= day && day <= x.to);
  if (!sub) return "open";
  return sub.status === "approved" ? "approved" : sub.status === "rejected" ? "rejected" : "submitted";
}

export interface Window {
  from: string;
  to: string;
}

export function visibleLines(db: Db, me: Member, filter: Tags, w: Window, approved = false): Line[] {
  return lines(db).filter((l) => l.day >= w.from && l.day <= w.to && contains(l.tags, filter) && canSee(db, me, l.tags) && (!approved || submittedOf(db, l.session) === "approved"));
}

/** C(F, w) over lines with a cost. */
export function costOf(ls: Line[]): number {
  return ls.reduce((sum, l) => sum + (l.nanos ?? 0), 0);
}

const DAY = 86_400_000;
export function days(w: Window): number {
  return Math.round((Date.parse(w.to) - Date.parse(w.from)) / DAY) + 1;
}
function overlap(a: Window, b: Window): number {
  const from = a.from > b.from ? a.from : b.from;
  const to = a.to < b.to ? a.to : b.to;
  return to < from ? 0 : days({ from, to });
}

export function acceptedFor(db: Db, tags: Tags): MockEstimate | undefined {
  const id = db.accepted[filterKey(tags)];
  return db.estimates.find((e) => e.id === id);
}

/** P(F, w): the accepted estimate spread by days, or a person's shares. */
export function planOf(db: Db, filter: Tags, w: Window): number | null {
  const names = Object.keys(filter);
  if (names.length === 1 && filter.person) {
    const shares = db.shares.filter((s) => s.person === filter.person);
    if (!shares.length) return null;
    let total = 0;
    for (const share of shares) {
      const est = acceptedFor(db, { project: share.project });
      if (!est) continue;
      const covered = { from: share.from > w.from ? share.from : w.from, to: share.to && share.to < w.to ? share.to : w.to };
      if (covered.to < covered.from) continue;
      total += Number(share.fraction) * spread(est, covered);
    }
    return total;
  }
  const est = acceptedFor(db, filter);
  return est ? spread(est, w) : null;
}

function spread(est: MockEstimate, w: Window): number {
  return (est.median * overlap(est, w)) / days(est);
}

export function sessionView(db: Db, s: MockSession): Session {
  const split = splitOf(db, s);
  const costs = s.messages.map((m) => messageCost(db, m));
  const total = costs.reduce((sum, c) => sum + (c.nanos ?? 0), 0);
  return {
    session_id: s.session_id,
    person: s.person,
    agent_harness: s.agent_harness,
    agent_name: s.agent_name,
    started_at: s.started_at,
    ended_at: s.ended_at,
    summary: s.summary,
    facts: s.facts as Session["facts"],
    signals: s.signals as Session["signals"],
    messages: s.messages.length,
    cost: dec(total),
    count_errors: costs.filter((c) => c.label === "count_error").length,
    unknown_models: [...new Set(s.messages.filter((_, i) => costs[i]?.label === "unknown_model").map((m) => `${m.provider}/${m.model}`))],
    assigned_by: split.by,
    submitted: submittedOf(db, s),
    parts: split.parts.map((p) => {
      const tags = fullTags(db, p.tags, s.person);
      return { fraction: String(p.bp / 10_000), assigned: p.tags, tags: tags, label: labelOf(tags), amount: dec((total * p.bp) / 10_000) };
    }),
  };
}
