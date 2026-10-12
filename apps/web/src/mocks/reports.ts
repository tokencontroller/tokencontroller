// Mock answers for token budgets, token forecasts and reports, shaped as the
// service's (apps/server/src/budgets and apps/server/src/reports): the same
// access rules, checks, messages and status codes. Money is kept in
// nano-units and sent as the service's decimal strings.

import { http, HttpResponse, type HttpResponseResolver } from "msw";
import type {
  AcceptInput,
  CostReport,
  CostReportRow,
  Outcome,
  Outlier,
  Outliers,
  Person,
  SavedReport,
  SaveInput,
  Tags,
  TicketActual,
  TokenBudgetDetail,
  TokenBudgetEntry,
  TokenBudgetInput,
  TokenBudgetReport,
  TokenBudgetRow,
  TokenBudgetView,
  TokenForecastSettings,
  TokenForecastView,
} from "@/api/contract";
import { TODAY, type BudgetRecord, type Db, type EntryRecord, type ForecastMatchRecord, type SavedReportRecord } from "./seed";
import { allocate, costKind, counts, cur, dec, matches } from "./store";

type Ctx = { db: Db; me: Person; url: URL; params: Record<string, string>; request: Request };
type Member = (handler: (ctx: Ctx) => Response | Promise<Response>) => HttpResponseResolver;
type Fail = (status: number, code: string, message: string) => Response;

/** The service's reasons for no token forecast (apps/server/src/budgets/forecast.rs). */
export const NO_MODEL =
  "Token forecasts are off on this instance: it has no embeddings model configured. Whoever runs it can set TC_EMBEDDINGS_URL and TC_EMBEDDINGS_MODEL (see Self-hosting > Configuration). Token budgets can still be typed.";
export const SWITCHED_OFF =
  "Token forecasts are switched off for this organization. A controller can switch them on again in the token forecast settings. Token budgets can still be typed.";

const rank: Record<Person["role"], number> = { agent_manager: 1, people_manager: 2, controller: 3 };

/** A text as Rust's `{:?}` writes it in a message: in double quotes. */
const quoted = (text: string) => JSON.stringify(text);

const THIS_MONTH = TODAY.slice(0, 7);

// Exchange rates per US dollar on the last day of each period, for showing a
// report in another currency.
const RATES: Record<string, [number, number]> = { USD: [1, 1], EUR: [92, 100], GBP: [79, 100], CHF: [88, 100] };

function convert(nanos: number, from: string, to: string): number {
  if (from === to) return nanos;
  const [fn, fd] = RATES[from] ?? [1, 1];
  const [tn, td] = RATES[to] ?? [1, 1];
  return Math.round((nanos * fd * tn) / (fn * td));
}

/** The rate from one currency to another as the service writes it, such as `0.92`. */
function rateText(from: string, to: string): string {
  const [fn, fd] = RATES[from] ?? [1, 1];
  const [tn, td] = RATES[to] ?? [1, 1];
  const nanos = Math.round((1e9 * fd * tn) / (fn * td));
  const whole = Math.floor(nanos / 1e9);
  const fraction = String(nanos % 1e9).padStart(9, "0").replace(/0+$/, "");
  return fraction ? `${String(whole)}.${fraction}` : String(whole);
}

function parsePeriod(text: string): string | null {
  return /^\d{4}-(0[1-9]|1[0-2])$/.test(text) ? text : null;
}

const notAPeriod = (text: string) => `${quoted(text)} is not a period: use the year and month, such as 2026-09`;

/** A decimal amount, such as `1500.00`, in nano-units; null when it is not one. */
function parseNanos(text: string): number | null {
  const match = /^([-+]?)(\d+)(?:\.(\d{1,9}))?$/.exec(text.trim());
  if (!match) return null;
  const [, sign, whole = "0", fraction = ""] = match;
  const nanos = Number(BigInt(whole) * 1_000_000_000n + BigInt(fraction.padEnd(9, "0")));
  return sign === "-" ? -nanos : nanos;
}

/** A tag filter in the URL form, such as `client:acme,team:web`, or the service's message. */
function parseFilter(text: string): Tags | string {
  const tags: Tags = {};
  for (const raw of text.split(",")) {
    const part = raw.trim();
    if (!part) continue;
    const at = part.indexOf(":");
    const name = at < 0 ? "" : part.slice(0, at).trim();
    const value = at < 0 ? "" : part.slice(at + 1).trim();
    if (!name || !value) return `${quoted(part)} is not a tag: write it as name:value, such as client:acme`;
    if (Object.hasOwn(tags, name)) return `the filter names ${quoted(name)} twice; a filter has one value per tag name`;
    tags[name] = value;
  }
  return tags;
}

function median(amounts: number[]): number | null {
  const sorted = [...amounts].sort((a, b) => a - b);
  const upper = Math.floor(sorted.length / 2);
  if (!sorted.length) return null;
  if (sorted.length % 2) return sorted[upper] ?? null;
  // The two middle amounts split in two halves, the first one taking the odd nano.
  return allocate((sorted[upper - 1] ?? 0) + (sorted[upper] ?? 0), [1, 1])[0] ?? null;
}

/** Shares of 10,000 basis points by the largest-remainder method, or none for no usage or a negative line. */
function sharePoints(amounts: number[]): (number | null)[] {
  const whole = amounts.reduce((sum, amount) => sum + BigInt(amount), 0n);
  if (!amounts.length || whole <= 0n || amounts.some((amount) => amount < 0)) return amounts.map(() => null);
  const exact = amounts.map((amount) => BigInt(amount) * 10_000n);
  const parts = exact.map((x) => x / whole);
  let rest = 10_000n - parts.reduce((sum, x) => sum + x, 0n);
  const order = exact.map((x, i) => ({ i, rem: x % whole })).sort((a, b) => (b.rem > a.rem ? 1 : b.rem < a.rem ? -1 : a.i - b.i));
  for (const { i } of order) {
    if (rest === 0n) break;
    parts[i] = (parts[i] ?? 0n) + 1n;
    rest -= 1n;
  }
  return parts.map(Number);
}

// Who sees what (docs: Account > Roles): an agent manager their own token
// entries, a people manager also everything within their filters, a
// controller everything.

/** One split of a token entry: a cost line, with its list value in nano-units. */
interface Line {
  entry: EntryRecord;
  index: number;
  tags: Tags;
  value: number | null;
}

function linesOf(entry: EntryRecord): Line[] {
  const values = entry.list === null ? entry.splits.map(() => null) : allocate(entry.list, entry.splits.map((split) => split.share));
  return entry.splits.map((split, index) => ({ entry, index, tags: split.tags, value: values[index] ?? null }));
}

function seesLine(me: Person, line: Line): boolean {
  if (me.role === "controller" || line.entry.agent_manager === me.id) return true;
  return me.role === "people_manager" && me.filters.some((filter) => matches(line.tags, filter));
}

function seesTags(me: Person, tags: Tags): boolean {
  return me.role === "controller" || (me.role === "people_manager" && me.filters.some((filter) => matches(tags, filter)));
}

/** The cost lines of one period a person sees, matching a filter, oldest first. Late token sheets are adjustments, not lines. */
function account(db: Db, me: Person, period: string, approvedOnly: boolean, filter: Tags): Line[] {
  return db.entries
    .filter((entry) => entry.period === period && !entry.late && counts(entry, approvedOnly))
    .sort((a, b) => a.started_at.localeCompare(b.started_at) || a.id.localeCompare(b.id))
    .flatMap(linesOf)
    .filter((line) => matches(line.tags, filter) && seesLine(me, line));
}

interface AdjustmentLine {
  id: string;
  period: string;
  tags: Tags;
  amount: number;
  reason: string;
  late: boolean;
}

/** Adjustments to closed periods a person sees, matching a filter. */
function adjustments(db: Db, me: Person, periods: string[], filter: Tags): AdjustmentLine[] {
  return db.adjustments
    .map((a) => ({ id: a.id, period: a.period, tags: { [a.rung]: a.value }, amount: a.amount, reason: a.reason, late: a.late_sheet }))
    .filter((a) => periods.includes(a.period) && seesTags(me, a.tags) && matches(a.tags, filter));
}

const isClosed = (db: Db, period: string) => db.periods.find((p) => p.id === period)?.status === "closed";

/** Every period from the organization's first token entry up to this month: what an all-time token budget counts. */
function allPeriods(db: Db): string[] {
  const first = db.entries.reduce((min, entry) => (entry.period < min ? entry.period : min), THIS_MONTH);
  const periods: string[] = [];
  for (let period = first; period <= THIS_MONTH; ) {
    periods.push(period);
    const [year = 2026, month = 1] = period.split("-").map(Number);
    period = month === 12 ? `${String(year + 1)}-01` : `${String(year)}-${String(month + 1).padStart(2, "0")}`;
  }
  return periods;
}

// Reports (docs: Report > Reading reports, Agent harness reports).

const AGENT_CUTS = ["agent_harness", "agent_name", "model", "effort", "speed"];

function groupKey(by: string, line: Line, ladder: string[]): string | null {
  const entry = line.entry;
  switch (by) {
    case "agent_harness":
      return entry.agent_harness;
    case "agent_name":
      return entry.agent_name;
    case "model":
      return entry.model;
    case "effort":
      return entry.effort;
    case "speed":
      return null;
    case "day":
      return entry.started_at.slice(0, 10);
    case "cost_kind":
      return costKind(line.tags, ladder);
    default:
      return line.tags[by] ?? null;
  }
}

const isTagGrouping = (by: string) => ![...AGENT_CUTS, "day", "cost_kind"].includes(by);

interface ReportSpec {
  period: string;
  by: string;
  filter: Tags;
  approved: boolean;
  currency: string | null;
  share: boolean;
}

/** Runs a report the way the service's `reports::cost::run` does. */
function runReport(db: Db, me: Person, spec: ReportSpec, fail: Fail): CostReport | Response {
  const reporting = cur(db);
  const record = db.periods.find((p) => p.id === spec.period);
  const ladder = record?.tag_map?.ladder ?? db.tagMap.ladder;
  const groups = new Map<string | null, { nanos: number; entries: number; unknown: number }>();
  for (const line of account(db, me, spec.period, spec.approved, spec.filter)) {
    const key = groupKey(spec.by, line, ladder);
    const group = groups.get(key) ?? { nanos: 0, entries: 0, unknown: 0 };
    group.entries += 1;
    if (line.value === null) group.unknown += 1;
    else group.nanos += line.value;
    groups.set(key, group);
  }
  const rows = [...groups.entries()]
    .map(([value, group]) => ({ value, nanos: group.nanos, entries: group.entries, unknown: group.unknown, adjustment: false }))
    .sort((a, b) => Number(a.value === null) - Number(b.value === null) || b.nanos - a.nanos || (a.value ?? "").localeCompare(b.value ?? ""));
  const posted = new Map<string | null, number>();
  for (const a of adjustments(db, me, [spec.period], spec.filter)) {
    const key = isTagGrouping(spec.by) ? (a.tags[spec.by] ?? null) : null;
    posted.set(key, (posted.get(key) ?? 0) + a.amount);
  }
  const adjustmentRows = [...posted.entries()]
    .sort(([a], [b]) => (a === null ? -1 : b === null ? 1 : a.localeCompare(b)))
    .map(([value, nanos]) => ({ value, nanos, entries: 0, unknown: 0, adjustment: true }));
  const all = [...rows, ...adjustmentRows];
  const total = all.reduce((sum, row) => sum + row.nanos, 0);

  let target = reporting;
  if (spec.currency !== null) {
    target = spec.currency.trim();
    if (!/^[A-Z]{3}$/.test(target)) return fail(400, "bad_request", `${quoted(target)} is not a currency code: use three capital letters, such as EUR`);
  }
  if (!RATES[target] || !RATES[reporting]) {
    const day = `${spec.period}-${String(new Date(Date.UTC(Number(spec.period.slice(0, 4)), Number(spec.period.slice(5, 7)), 0)).getUTCDate())}`;
    return fail(409, "conflict", `there is no exchange rate from ${reporting} to ${target} for ${day < TODAY ? day : TODAY} yet: it arrives with the daily rate refresh, or a controller can enter it`);
  }
  const amounts = all.map((row) => convert(row.nanos, reporting, target));
  const usage = sharePoints(amounts.filter((_, i) => !all[i]?.adjustment));
  let next = 0;
  const shares = all.map((row) => (row.adjustment ? null : (usage[next++] ?? null)));
  return {
    period: spec.period,
    closed: isClosed(db, spec.period),
    by: spec.by,
    filter: spec.filter,
    status: spec.approved ? "approved" : "all",
    view: spec.share ? "share" : "money",
    currency: target,
    exchange_rate: target === reporting ? null : rateText(reporting, target),
    label: spec.share ? "share of the period" : "list-price estimate",
    rows: all.map(
      (row, i): CostReportRow => ({
        value: row.value,
        amount: dec(amounts[i] ?? 0, target),
        share: spec.share ? (shares[i] ?? null) : null,
        token_entries: row.entries,
        unknown_model: row.unknown,
        adjustment: row.adjustment,
      }),
    ),
    total: dec(target === reporting ? total : amounts.reduce((sum, amount) => sum + amount, 0), target),
  };
}

/** The period a report is asked for, read as the service reads its query. */
function readPeriod(url: URL, fail: Fail): string | Response {
  const text = url.searchParams.get("period");
  if (text === null) return fail(400, "bad_request", "Failed to deserialize query string: missing field `period`");
  return parsePeriod(text) ?? fail(400, "bad_request", `Failed to deserialize query string: ${notAPeriod(text)}`);
}

/** The controls every report takes, read as the service reads its query. */
function readControls(url: URL, fail: Fail): Omit<ReportSpec, "by" | "filter"> | Response {
  const period = readPeriod(url, fail);
  if (period instanceof Response) return period;
  const status = url.searchParams.get("status") ?? "all";
  if (status !== "all" && status !== "approved") return fail(400, "bad_request", `Failed to deserialize query string: status: unknown variant \`${status}\`, expected \`all\` or \`approved\``);
  const view = url.searchParams.get("view") ?? "money";
  if (view !== "money" && view !== "share") return fail(400, "bad_request", `Failed to deserialize query string: view: unknown variant \`${view}\`, expected \`money\` or \`share\``);
  return { period, approved: status === "approved", currency: url.searchParams.get("currency"), share: view === "share" };
}

function reportSpec(url: URL, defaultBy: string, fail: Fail): ReportSpec | Response {
  const controls = readControls(url, fail);
  if (controls instanceof Response) return controls;
  const filter = parseFilter(url.searchParams.get("filter") ?? "");
  if (typeof filter === "string") return fail(400, "bad_request", filter);
  return { ...controls, by: (url.searchParams.get("by") ?? defaultBy).trim(), filter };
}

// Token budgets (docs: Account > Token budgets and forecasts).

/** A filter with the tags its ticket, project and person carry added; its own tags win. */
function expand(db: Db, filter: Tags): Tags {
  const tags: Tags = { ...filter };
  const add = (more: Tags) => {
    for (const [name, value] of Object.entries(more)) if (!Object.hasOwn(tags, name)) tags[name] = value;
  };
  const ticket = filter.ticket ? db.tickets.find((t) => t.key === filter.ticket) : undefined;
  if (ticket) {
    add(ticket.tags);
    const project = db.projects.find((p) => p.id === ticket.project_id);
    if (project && !Object.hasOwn(tags, "project")) tags.project = project.name;
  }
  const project = tags.project ? db.projects.find((p) => p.name === tags.project) : undefined;
  if (project) add(project.tags);
  const person = filter.person ? db.people.find((p) => p.email === filter.person) : undefined;
  if (person) add(person.tags);
  return tags;
}

/** Whether a person may act on token entries matching a filter (with its inherited tags): a controller anywhere, a people manager within their filters. */
function covers(me: Person, expanded: Tags): boolean {
  return me.role === "controller" || (me.role === "people_manager" && me.filters.some((mine) => matches(expanded, mine)));
}

const maySet = (me: Person, expanded: Tags) => rank[me.role] >= rank.people_manager && covers(me, expanded);

function seesBudget(db: Db, me: Person, filter: Tags): boolean {
  return me.role === "controller" || filter.person === me.email || maySet(me, expand(db, filter));
}

function appliesText(b: BudgetRecord): string {
  if (b.period === null) return "all time";
  return b.every_month ? `every month from ${b.period}` : b.period;
}

const inForce = (b: BudgetRecord, period: string) => (b.period === null ? true : b.every_month ? b.period <= period : b.period === period);

function budgetView(db: Db, b: BudgetRecord): TokenBudgetView {
  return {
    id: b.id,
    name: b.name,
    filter: b.filter,
    period: b.period,
    every_month: b.every_month,
    applies: appliesText(b),
    amount: dec(b.amount, cur(db)),
    currency: cur(db),
    token_forecast_id: b.token_forecast_id,
    set_by: db.people.find((p) => p.id === b.set_by)?.name ?? null,
    set_at: b.set_at,
  };
}

function visibleBudgets(db: Db, me: Person): BudgetRecord[] {
  return db.budgets.filter((b) => seesBudget(db, me, b.filter)).sort((a, b) => b.set_at.localeCompare(a.set_at) || a.id.localeCompare(b.id));
}

type Draft = Pick<BudgetRecord, "name" | "filter" | "period" | "every_month" | "amount">;

/** The service's check of a token budget input, or what to fix. */
function check(input: Partial<TokenBudgetInput>): Draft | string {
  const filter: Tags = {};
  for (const [rawName, rawValue] of Object.entries(input.filter ?? {}).sort(([a], [b]) => a.localeCompare(b))) {
    const name = rawName.trim();
    const value = rawValue.trim();
    if (!name || !value) return "every tag of the filter needs a name and a value, such as team: web";
    if (Object.hasOwn(filter, name)) return `the filter names ${quoted(name)} twice; a filter has one value per tag name`;
    filter[name] = value;
  }
  const text = input.period?.trim() ?? "";
  const period = text ? parsePeriod(text) : null;
  if (text && !period) return notAPeriod(text);
  const everyMonth = input.every_month ?? false;
  if (!period && everyMonth) return "a token budget for every month needs the period it starts in, such as 2026-09";
  const amountText = input.amount ?? "";
  const amount = parseNanos(amountText);
  if (amount === null) return `${quoted(amountText)} is not an amount: write it as a decimal number, such as 1234.56`;
  if (amount <= 0) return "a token budget needs an amount above zero";
  const name = input.name?.trim() || null;
  return { name, filter, period, every_month: everyMonth, amount };
}

// Token budget vs actual (docs: Report > Token budget vs actual).

interface Actual {
  value: number;
  entries: Set<string>;
  unknown: Set<string>;
  lines: Line[];
  adjustments: AdjustmentLine[];
}

/** What was spent on a filter in some periods, as a person sees it. */
function actualOf(db: Db, me: Person, filter: Tags, periods: string[], approved: boolean): Actual {
  const lines = periods.flatMap((period) => account(db, me, period, approved, filter));
  const posted = adjustments(db, me, periods, filter);
  return {
    value: lines.reduce((sum, line) => sum + (line.value ?? 0), 0) + posted.reduce((sum, a) => sum + a.amount, 0),
    entries: new Set(lines.map((line) => line.entry.id)),
    unknown: new Set(lines.filter((line) => line.value === null).map((line) => line.entry.id)),
    lines,
    adjustments: posted,
  };
}

function budgetRow(db: Db, me: Person, b: BudgetRecord, period: string, approved: boolean): { row: TokenBudgetRow; actual: Actual; difference: number } {
  const actualPeriod = b.period === null ? null : period;
  const actual = actualOf(db, me, b.filter, actualPeriod === null ? allPeriods(db) : [actualPeriod], approved);
  const difference = actual.value - b.amount;
  // Token entries on an unknown model leave an underrun open, never an overrun.
  const outcome: Outcome | null = difference > 0 ? "overrun" : actual.unknown.size ? null : difference === 0 ? "at_token_budget" : "underrun";
  const currency = cur(db);
  return {
    actual,
    difference,
    row: {
      token_budget: budgetView(db, b),
      actual_period: actualPeriod,
      amount: dec(b.amount, currency),
      actual: dec(actual.value, currency),
      difference: dec(difference, currency),
      outcome,
      token_entries: actual.entries.size,
      unknown_model_entries: actual.unknown.size,
    },
  };
}

function entryViews(db: Db, lines: Line[]): TokenBudgetEntry[] {
  const byEntry = new Map<string, Line[]>();
  for (const line of lines) byEntry.set(line.entry.id, [...(byEntry.get(line.entry.id) ?? []), line]);
  const currency = cur(db);
  return [...byEntry.values()].flatMap((splits) => {
    const first = splits[0];
    if (!first) return [];
    const entry = first.entry;
    const known = splits.every((line) => line.value !== null);
    return [
      {
        id: entry.id,
        agent_harness: entry.agent_harness,
        agent_name: entry.agent_name,
        model: entry.model,
        provider: entry.provider,
        started_at: entry.started_at,
        status: entry.status,
        automatic: entry.automatic,
        summary: entry.summary,
        agent_manager: db.people.find((p) => p.id === entry.agent_manager)?.name ?? null,
        list_value: known ? dec(splits.reduce((sum, line) => sum + (line.value ?? 0), 0), currency) : null,
        currency,
        label: known ? "list-price estimate" : "unknown model",
        splits: splits.map((line) => ({ id: `${entry.id}-${String(line.index)}`, list_value: line.value === null ? null : dec(line.value, currency), tags: line.tags, placed_by: entry.placed_by, unknown_ticket: null })),
        rejected_reason: entry.reject_reason,
      },
    ];
  });
}

function perTicket(db: Db, actual: Actual): TicketActual[] {
  const buckets = new Map<string | null, { nanos: number; entries: Set<string>; unknown: Set<string> }>();
  const bucket = (key: string | null) => {
    const found = buckets.get(key) ?? { nanos: 0, entries: new Set<string>(), unknown: new Set<string>() };
    buckets.set(key, found);
    return found;
  };
  for (const line of actual.lines) {
    const b = bucket(line.tags.ticket ?? null);
    b.entries.add(line.entry.id);
    if (line.value === null) b.unknown.add(line.entry.id);
    else b.nanos += line.value;
  }
  for (const a of actual.adjustments) bucket(a.tags.ticket ?? null).nanos += a.amount;
  const currency = cur(db);
  return [...buckets.entries()]
    .sort(([a, x], [b, y]) => Number(a === null) - Number(b === null) || y.nanos - x.nanos || (a ?? "").localeCompare(b ?? ""))
    .map(([ticket, b]) => ({
      ticket,
      title: ticket ? (db.tickets.find((t) => t.key === ticket)?.title ?? null) : null,
      actual: dec(b.nanos, currency),
      token_entries: b.entries.size,
      unknown_model_entries: b.unknown.size,
    }));
}

// Token forecasts. The mock has no embeddings model: it compares words, and
// work in the same project counts as a little alike.

function stems(text: string): Set<string> {
  return new Set(
    text
      .toLowerCase()
      .split(/[^a-z]+/)
      .filter((word) => word.length > 3)
      .map((word) => word.slice(0, 5)),
  );
}

function confidence(target: Set<string>, summary: string, sameProject: boolean): number {
  const shared = [...stems(summary)].filter((stem) => target.has(stem)).length;
  let hash = 0;
  for (const char of summary) hash = (hash * 31 + char.charCodeAt(0)) >>> 0;
  return Math.min(94, 22 + 21 * shared + (sameProject ? 9 : 0) + (hash % 7));
}

function settingsOf(db: Db): TokenForecastSettings {
  const { enabled, model } = db.forecasts;
  return { enabled, available: model !== null, model, reason: model === null ? NO_MODEL : enabled ? null : SWITCHED_OFF };
}

function forecastView(db: Db, f: Db["forecasts"]["made"][number]): TokenForecastView {
  return {
    id: f.id,
    ticket: f.ticket,
    title: db.tickets.find((t) => t.key === f.ticket)?.title ?? null,
    currency: f.currency,
    proposed: f.proposed === null ? null : dec(f.proposed, f.currency),
    matches: f.matches.map((match) => ({ ticket: match.ticket, title: match.title, confidence: match.confidence, actual: dec(match.actual, f.currency), token_entries: match.token_entries, unknown_model_entries: match.unknown_model_entries })),
    model: f.model,
    created_at: f.created_at,
  };
}

const knownCosts = (found: ForecastMatchRecord[]) => found.filter((match) => match.unknown_model_entries === 0).map((match) => match.actual);

// Outliers (docs: Report > Outliers).

const MIN_SIMILAR = 3;

function outliersOf(db: Db, me: Person, period: string, filter: Tags, limit: number): Outliers {
  const currency = cur(db);
  interface Ticket {
    project: string | null;
    nanos: number;
    entries: Set<string>;
    unknown: Set<string>;
  }
  const add = (map: Map<string, Ticket>, lines: Line[]) => {
    for (const line of lines) {
      const key = line.tags.ticket;
      if (!key) continue;
      const ticket = map.get(key) ?? { project: null, nanos: 0, entries: new Set<string>(), unknown: new Set<string>() };
      ticket.project ??= line.tags.project ?? null;
      ticket.entries.add(line.entry.id);
      if (line.value === null) ticket.unknown.add(line.entry.id);
      else ticket.nanos += line.value;
      map.set(key, ticket);
    }
  };
  // The twelve months before the period, for the whole organization.
  const [year = 2026, month = 1] = period.split("-").map(Number);
  const months = Array.from({ length: 12 }, (_, i) => {
    const index = year * 12 + (month - 1) - (i + 1);
    return `${String(Math.floor(index / 12))}-${String((index % 12) + 1).padStart(2, "0")}`;
  });
  const past = db.entries.filter((entry) => months.includes(entry.period) && counts(entry)).flatMap(linesOf);
  const tickets = new Map<string, Ticket>();
  add(tickets, account(db, me, period, false, filter));
  add(tickets, past.filter((line) => seesLine(me, line) && matches(line.tags, filter) && tickets.has(line.tags.ticket ?? "")));
  const earlier = new Map<string, Ticket>();
  add(earlier, past);
  for (const key of tickets.keys()) earlier.delete(key);
  const costs = [...earlier.values()].filter((t) => t.nanos > 0);
  const baseline = (amounts: number[]) => (amounts.length >= MIN_SIMILAR ? { amount: median(amounts) ?? 0, tickets: amounts.length } : null);
  const organization = baseline(costs.map((t) => t.nanos));
  let withoutBaseline = 0;
  const found: { far: number; outlier: Outlier }[] = [];
  for (const [key, ticket] of tickets) {
    if (ticket.nanos <= 0) continue;
    const ofProject = ticket.project ? baseline(costs.filter((t) => t.project === ticket.project).map((t) => t.nanos)) : null;
    const compared = ofProject ? { ...ofProject, with: "project" as const } : organization ? { ...organization, with: "organization" as const } : null;
    if (!compared || compared.amount <= 0) {
      withoutBaseline += 1;
      continue;
    }
    const tenths = Math.floor((ticket.nanos * 20 + compared.amount) / (compared.amount * 2));
    found.push({
      // Twice and half are equally far.
      far: Math.max(ticket.nanos, compared.amount) / Math.min(ticket.nanos, compared.amount),
      outlier: {
        ticket: key,
        project: ticket.project,
        amount: dec(ticket.nanos, currency),
        baseline: dec(compared.amount, currency),
        ratio: `${String(Math.floor(tenths / 10))}.${String(tenths % 10)}`,
        direction: ticket.nanos >= compared.amount ? "high" : "low",
        compared_with: compared.with,
        similar_tickets: compared.tickets,
        token_entries: ticket.entries.size,
        unknown_model: ticket.unknown.size,
      },
    });
  }
  found.sort((a, b) => b.far - a.far || a.outlier.ticket.localeCompare(b.outlier.ticket));
  return { period, closed: isClosed(db, period), currency, label: "list-price estimate", tickets: found.slice(0, limit).map((f) => f.outlier), without_baseline: withoutBaseline };
}

function savedView(db: Db, r: SavedReportRecord): SavedReport {
  return { id: r.id, name: r.name, filter: r.filter, by: r.by, created_by: db.people.find((p) => p.id === r.created_by)?.name ?? null, created_at: r.created_at };
}

export function reportsHandlers({ API, member, fail, now }: { API: string; member: Member; fail: Fail; now: () => string }) {
  const forbidden = () => fail(403, "forbidden", "your role cannot do this");
  const notFound = () => fail(404, "not_found", "not found");
  const bad = (message: string) => fail(400, "bad_request", message);
  const conflict = (message: string) => fail(409, "conflict", message);
  const atLeast = (me: Person, role: Person["role"]) => rank[me.role] >= rank[role];

  /** A token budget the person sees, or the 404 the service answers. */
  const findBudget = (db: Db, me: Person, id: string) => {
    const found = db.budgets.find((b) => b.id === id);
    return found && seesBudget(db, me, found.filter) ? found : null;
  };

  /** The period asked for, or this month. */
  const periodOrNow = (url: URL): string | Response => {
    const text = url.searchParams.get("period")?.trim();
    if (!text) return THIS_MONTH;
    return parsePeriod(text) ?? bad(notAPeriod(text));
  };

  return [
    // Token budgets
    http.get(`${API}/token-budgets`, member(({ db, me }) => HttpResponse.json(visibleBudgets(db, me).map((b) => budgetView(db, b))))),
    http.post(`${API}/token-budgets`, member(async ({ db, me, request }) => {
      if (!atLeast(me, "people_manager")) return forbidden();
      const draft = check((await request.json()) as Partial<TokenBudgetInput>);
      if (typeof draft === "string") return bad(draft);
      if (!maySet(me, expand(db, draft.filter))) return forbidden();
      const budget: BudgetRecord = { id: crypto.randomUUID(), ...draft, token_forecast_id: null, set_by: me.id, set_at: now() };
      db.budgets.push(budget);
      return HttpResponse.json(budgetView(db, budget), { status: 201 });
    })),
    http.put(`${API}/token-budgets/:id`, member(async ({ db, me, params, request }) => {
      if (!atLeast(me, "people_manager")) return forbidden();
      const budget = findBudget(db, me, params.id ?? "");
      if (!budget) return notFound();
      if (!maySet(me, expand(db, budget.filter))) return forbidden();
      const draft = check((await request.json()) as Partial<TokenBudgetInput>);
      if (typeof draft === "string") return bad(draft);
      if (!maySet(me, expand(db, draft.filter))) return forbidden();
      Object.assign(budget, draft, { set_by: me.id, set_at: now() });
      return HttpResponse.json(budgetView(db, budget));
    })),
    http.delete(`${API}/token-budgets/:id`, member(({ db, me, params }) => {
      if (!atLeast(me, "people_manager")) return forbidden();
      const budget = findBudget(db, me, params.id ?? "");
      if (!budget) return notFound();
      if (!maySet(me, expand(db, budget.filter))) return forbidden();
      db.budgets = db.budgets.filter((b) => b !== budget);
      return new HttpResponse(null, { status: 204 });
    })),

    // Token budget vs actual
    http.get(`${API}/reports/token-budgets`, member(({ db, me, url }) => {
      const period = periodOrNow(url);
      if (period instanceof Response) return period;
      const approved = url.searchParams.get("status") === "approved";
      const rows = visibleBudgets(db, me)
        .filter((b) => inForce(b, period))
        .map((b) => budgetRow(db, me, b, period, approved))
        .sort((a, b) => b.difference - a.difference || a.row.token_budget.id.localeCompare(b.row.token_budget.id))
        .map((r) => r.row);
      const report: TokenBudgetReport = { period, currency: cur(db), status: approved ? "approved" : "all", rows };
      return HttpResponse.json(report);
    })),
    http.get(`${API}/reports/token-budgets/:id`, member(({ db, me, url, params }) => {
      const period = periodOrNow(url);
      if (period instanceof Response) return period;
      const budget = findBudget(db, me, params.id ?? "");
      if (!budget) return notFound();
      if (!inForce(budget, period)) return conflict(`this token budget applies to ${appliesText(budget)}, not to ${period}`);
      const { row, actual } = budgetRow(db, me, budget, period, url.searchParams.get("status") === "approved");
      const detail: TokenBudgetDetail = {
        period,
        currency: cur(db),
        row,
        tickets: perTicket(db, actual),
        token_entries: entryViews(db, actual.lines),
        adjustments: actual.adjustments.map((a) => ({ id: a.id, amount: dec(a.amount, cur(db)), tags: a.tags, reason: a.reason, late: a.late })),
      };
      return HttpResponse.json(detail);
    })),

    // Token forecasts
    http.get(`${API}/token-forecasts/settings`, member(({ db, me }) => (atLeast(me, "people_manager") ? HttpResponse.json(settingsOf(db)) : forbidden()))),
    http.put(`${API}/token-forecasts/settings`, member(async ({ db, me, request }) => {
      if (!atLeast(me, "controller")) return forbidden();
      const { enabled } = (await request.json()) as { enabled: boolean };
      db.forecasts.enabled = enabled;
      return HttpResponse.json(settingsOf(db));
    })),
    http.post(`${API}/tickets/:key/token-forecast`, member(({ db, me, params }) => {
      if (!atLeast(me, "people_manager")) return forbidden();
      const model = db.forecasts.model;
      if (model === null) return conflict(NO_MODEL);
      if (!db.forecasts.enabled) return conflict(SWITCHED_OFF);
      const ticket = db.tickets.find((t) => t.key === (params.key ?? "").trim());
      if (!ticket) return notFound();
      if (!covers(me, expand(db, { ticket: ticket.key }))) return forbidden();
      const text = [ticket.title, ticket.description].map((part) => part?.trim() ?? "").filter(Boolean).join("\n\n");
      if (!text) return conflict(`${ticket.key} has no title or description to compare with past work: add one, or type a token budget`);
      const target = stems(text);
      const project = db.projects.find((p) => p.id === ticket.project_id)?.name ?? null;
      const periods = allPeriods(db);
      // Past work is the summaries of locked token entries: those of closed periods.
      const best = new Map<string, number>();
      for (const line of periods.flatMap((period) => account(db, me, period, false, {}))) {
        const key = line.tags.ticket;
        if (!key || key === ticket.key || line.entry.status !== "locked" || !line.entry.summary.trim()) continue;
        const alike = confidence(target, line.entry.summary, project !== null && line.tags.project === project);
        if (alike >= 30 && alike > (best.get(key) ?? 0)) best.set(key, alike);
      }
      const found: ForecastMatchRecord[] = [...best.entries()]
        .sort(([a, x], [b, y]) => y - x || a.localeCompare(b))
        .slice(0, 5)
        .map(([key, alike]) => {
          const actual = actualOf(db, me, { ticket: key }, periods, false);
          return { ticket: key, title: db.tickets.find((t) => t.key === key)?.title ?? null, confidence: alike, actual: actual.value, token_entries: actual.entries.size, unknown_model_entries: actual.unknown.size };
        });
      const record = { id: crypto.randomUUID(), ticket: ticket.key, currency: cur(db), proposed: median(knownCosts(found)), matches: found, model, created_by: me.id, created_at: now() };
      db.forecasts.made.push(record);
      return HttpResponse.json(forecastView(db, record), { status: 201 });
    })),
    http.post(`${API}/token-forecasts/:id/accept`, member(async ({ db, me, params, request }) => {
      if (!atLeast(me, "people_manager")) return forbidden();
      const forecast = db.forecasts.made.find((f) => f.id === params.id);
      if (!forecast) return notFound();
      if (!covers(me, expand(db, { ticket: forecast.ticket }))) return forbidden();
      const input = (await request.json()) as AcceptInput;
      const typed = input.amount?.trim();
      let amount: number;
      if (typed) {
        const parsed = parseNanos(typed);
        if (parsed === null) return bad(`${quoted(typed)} is not an amount: write it as a decimal number, such as 1234.56`);
        amount = parsed;
      } else if (input.tickets) {
        const keys = input.tickets;
        const stranger = keys.find((key) => !forecast.matches.some((match) => match.ticket === key));
        if (stranger !== undefined) return bad(`${stranger} is not one of this token forecast's matches`);
        const picked = median(knownCosts(forecast.matches.filter((match) => keys.includes(match.ticket))));
        if (picked === null) return conflict("none of the picked matches has a known cost: pick others, or type an amount");
        amount = picked;
      } else {
        if (forecast.proposed === null) return conflict("this token forecast proposes no amount: no match has a known cost. Type an amount instead");
        amount = forecast.proposed;
      }
      if (amount <= 0) return conflict("a token budget needs an amount above zero: type one");
      if (db.budgets.some((b) => b.token_forecast_id === forecast.id)) return conflict("this token forecast is already accepted as a token budget");
      const budget: BudgetRecord = { id: crypto.randomUUID(), name: null, filter: { ticket: forecast.ticket }, period: null, every_month: false, amount, token_forecast_id: forecast.id, set_by: me.id, set_at: now() };
      db.budgets.push(budget);
      return HttpResponse.json(budgetView(db, budget), { status: 201 });
    })),

    // Reports
    http.get(`${API}/reports/cost`, member(({ db, me, url }) => {
      const spec = reportSpec(url, "project", fail);
      if (spec instanceof Response) return spec;
      const report = runReport(db, me, spec, fail);
      return report instanceof Response ? report : HttpResponse.json(report);
    })),
    http.get(`${API}/reports/agent-harness`, member(({ db, me, url }) => {
      const spec = reportSpec(url, "agent_harness", fail);
      if (spec instanceof Response) return spec;
      if (!AGENT_CUTS.includes(spec.by)) return bad("group by agent_harness, agent_name, model, effort or speed");
      const report = runReport(db, me, spec, fail);
      return report instanceof Response ? report : HttpResponse.json(report);
    })),
    http.get(`${API}/reports/outliers`, member(({ db, me, url }) => {
      // Outliers take a period, a filter and a limit; no status, currency or share view.
      const period = readPeriod(url, fail);
      if (period instanceof Response) return period;
      const filter = parseFilter(url.searchParams.get("filter") ?? "");
      if (typeof filter === "string") return bad(filter);
      return HttpResponse.json(outliersOf(db, me, period, filter, Number(url.searchParams.get("limit") ?? 20)));
    })),
    http.get(`${API}/reports/saved`, member(({ db }) =>
      HttpResponse.json([...db.reports].sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase()) || a.created_at.localeCompare(b.created_at)).map((r) => savedView(db, r))),
    )),
    http.post(`${API}/reports/saved`, member(async ({ db, me, request }) => {
      const input = (await request.json()) as Partial<SaveInput>;
      const name = input.name?.trim() ?? "";
      if (!name) return bad("give the report a name");
      const by = input.by?.trim() ?? "";
      if (!by) return bad("say what the report groups by, such as team");
      const filter = parseFilter(input.filter ?? "");
      if (typeof filter === "string") return bad(filter);
      const record: SavedReportRecord = { id: crypto.randomUUID(), name, filter, by, created_by: me.id, created_at: now() };
      db.reports.push(record);
      return HttpResponse.json(savedView(db, record), { status: 201 });
    })),
    http.get(`${API}/reports/saved/:id`, member(({ db, me, url, params }) => {
      const saved = db.reports.find((r) => r.id === params.id);
      if (!saved) return notFound();
      const controls = readControls(url, fail);
      if (controls instanceof Response) return controls;
      const report = runReport(db, me, { ...controls, by: saved.by, filter: saved.filter }, fail);
      return report instanceof Response ? report : HttpResponse.json(report);
    })),
    http.delete(`${API}/reports/saved/:id`, member(({ db, me, params }) => {
      const saved = db.reports.find((r) => r.id === params.id && (r.created_by === me.id || me.role === "controller"));
      if (!saved) return notFound();
      db.reports = db.reports.filter((r) => r !== saved);
      return new HttpResponse(null, { status: 204 });
    })),
  ];
}
