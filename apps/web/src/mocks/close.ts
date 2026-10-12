// Mock answers for bills, bill connections and CSV bills, reconciliation,
// periods and the close, the cost sheet with its links for finance, and
// adjustments. Each mirrors the service's handler (apps/server/src/bills,
// close, token_sheets/late.rs and integrations/bill_*.rs): the same figures
// and rounding, the same messages, status codes and roles.

import { http, HttpResponse, type HttpResponseResolver } from "msw";
import type {
  AdjustmentInput,
  AdjustmentRow,
  BillApi,
  BillBlock,
  BillCheck,
  BillConnectionView,
  BillCostSource,
  BillCsvBill,
  BillInput,
  BillView,
  CostRow,
  CostSheet,
  CostSheetLink,
  KindTotals,
  LateSheet,
  NewBillConnection,
  PeriodSummary,
  PeriodView,
  Person,
  ProjectTotal,
  Reconciliation,
  Role,
  Tags,
  WithoutBill,
} from "@/api/contract";
import { CLOUDS, CsvBillProblem, divideRounded, isPeriod, minorDigits, moneyText, parseBills, parseCloudExport, quoted, type Cloud, type CsvBill } from "./bill-csv";
import { TODAY, type AdjustmentRecord, type BillRecord, type BillRevisionRecord, type Db, type EntryRecord, type PeriodRecord } from "./seed";
import { convert } from "./reports";
import { allocate, lateSheets, matches } from "./store";

type Ctx = { db: Db; me: Person; url: URL; params: Record<string, string>; request: Request };
type Member = (handler: (ctx: Ctx) => Response | Promise<Response>, roles?: Role[]) => HttpResponseResolver;
type Fail = (status: number, code: string, message: string) => Response;

const CONTROLLER: Role[] = ["controller"];

// Money in nano-units as BigInt, rounded the way the core rounds it.

const NANO = 1_000_000_000n;
const minorStep = (currency: string) => 10n ** BigInt(9 - minorDigits(currency));

/** An amount in minor units as the API writes it, such as `-18.40`. */
function minorText(minor: bigint, currency: string): string {
  const digits = minorDigits(currency);
  const magnitude = minor < 0n ? -minor : minor;
  const scale = 10n ** BigInt(digits);
  const units = (magnitude / scale).toString();
  const fraction = (magnitude % scale).toString().padStart(digits, "0");
  return `${minor < 0n ? "-" : ""}${digits ? `${units}.${fraction}` : units}`;
}

/** An amount as the API writes it: rounded to the currency's minor unit. */
export const dec = (nanos: bigint, currency: string) => minorText(divideRounded(nanos, minorStep(currency)), currency);

/** Turns exact running totals into parts that add up to the rounded total. */
function runningRound(values: bigint[], round: (exact: bigint) => bigint): bigint[] {
  let exact = 0n;
  let before = 0n;
  return values.map((value) => {
    exact += value;
    const rounded = round(exact);
    const part = rounded - before;
    before = rounded;
    return part;
  });
}

const roundAllToMinor = (values: bigint[], currency: string) => runningRound(values, (exact) => divideRounded(exact, minorStep(currency)));
const convertAll = (values: bigint[], rate: bigint) => runningRound(values, (exact) => divideRounded(exact * rate, NANO));
const sum = (values: bigint[]) => values.reduce((total, value) => total + value, 0n);

/** A decimal without trailing zeros, such as `0.9214`, `1` or `-14.5`: how the service writes rates and bill lines. */
function plainDecimal(nanos: bigint): string {
  const magnitude = nanos < 0n ? -nanos : nanos;
  const fraction = (magnitude % NANO).toString().padStart(9, "0").replace(/0+$/, "");
  const whole = (magnitude / NANO).toString();
  return `${nanos < 0n ? "-" : ""}${fraction ? `${whole}.${fraction}` : whole}`;
}
const rateText = plainDecimal;

/** A decimal number with up to nine digits after the point, as the core reads amounts. */
function parseNanos(text: string, { plus = true } = {}): bigint | null {
  const match = (plus ? /^([-+]?)(\d+)(?:\.(\d{1,9}))?$/ : /^(-?)(\d+)(?:\.(\d{1,9}))?$/).exec(text.trim());
  if (!match) return null;
  const [, sign, whole = "0", fraction = ""] = match;
  const nanos = BigInt(whole) * NANO + BigInt(fraction.padEnd(9, "0"));
  return sign === "-" ? -nanos : nanos;
}

const isTagName = (name: string) => /^[a-z][a-z0-9_-]{0,39}$/.test(name);
const tagNameProblem = (name: string) => `${quoted(name)} is not a tag name: use up to 40 small letters, digits, - and _, starting with a letter`;
const periodProblem = (text: string) => `${quoted(text)} is not a period: use the year and month, such as 2026-09`;
const compare = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

const BILL_SOURCES: BillCostSource[] = ["usage_at_api_rates", "api_key", "cloud_marketplace", "reseller"];
const KINDS = ["direct", "project_overhead", "organization_overhead"] as const;
type Kind = (typeof KINDS)[number];
const KIND_LABEL: Record<Kind, string> = { direct: "direct", project_overhead: "project overhead", organization_overhead: "organization overhead" };

/** A ticket makes cost direct; a project, or a rung below it such as an epic, project overhead. */
function costKind(tags: Tags, ladder: string[]): Kind {
  if (tags.ticket) return "direct";
  const project = ladder.indexOf("project");
  if (tags.project || (project !== -1 && ladder.slice(0, project).some((rung) => tags[rung]))) return "project_overhead";
  return "organization_overhead";
}

/** The person's name as the service shows it: the email when they gave none. */
function nameOf(db: Db, id: string | null | undefined): string | null {
  const person = db.people.find((p) => p.id === id);
  if (!person) return null;
  return person.name.trim() ? person.name : person.email;
}

function newId(db: Db): string {
  db.nextId += 1;
  return `6b657374-0000-4000-8009-${db.nextId.toString(16).padStart(12, "0")}`;
}

// Bills

const latest = (bill: BillRecord): BillRevisionRecord => {
  const revision = bill.revisions[bill.revisions.length - 1];
  if (!revision) throw new Error(`bill ${bill.id} has no revision`);
  return revision;
};

export function billView(bill: BillRecord, revision: BillRevisionRecord): BillView {
  const money = (nanos: number) => dec(BigInt(nanos), revision.currency);
  return {
    id: bill.id,
    period: bill.period,
    provider: bill.provider,
    cost_source: bill.cost_source,
    revision: revision.revision,
    currency: revision.currency,
    gross: money(revision.gross),
    discount: money(revision.discount),
    credit: money(revision.credit),
    net: money(revision.net),
    exchange_rate: revision.exchange_rate === null ? null : rateText(BigInt(revision.exchange_rate)),
    exchange_rate_given: revision.exchange_rate_given,
    source: revision.source,
    lines: (revision.lines ?? []) as BillView["lines"],
    recorded_at: revision.recorded_at,
    final: revision.final ?? true,
  };
}

/** The latest revision of each bill: newest period first, then provider and cost source. */
function latestBills(db: Db, period?: string): BillView[] {
  return db.bills
    .filter((b) => !period || b.period === period)
    .map((b) => billView(b, latest(b)))
    .sort((a, b) => compare(b.period, a.period) || compare(a.provider, b.provider) || compare(a.cost_source, b.cost_source));
}

/** A bill ready to store, amounts in nano-units of its currency. */
interface NewBill {
  period: string;
  provider: string;
  cost_source: BillCostSource;
  currency: string;
  gross: bigint;
  discount: bigint;
  credit: bigint;
  net: bigint;
  /** In nano-units; null to take the reference rate. */
  rate: bigint | null;
  source: BillRevisionRecord["source"];
  lines: Record<string, string | null>[];
}

/** The service's check of a bill before it is stored, or null when it holds together. */
function billProblem(bill: NewBill): string | null {
  if (bill.discount < 0n || bill.credit < 0n) return "discount and credit are written as positive amounts that reduce the gross";
  if (!bill.provider || bill.provider.length > 40 || !/^[a-z0-9_]+$/.test(bill.provider)) {
    return `${quoted(bill.provider)} is not a provider: use lower-case letters, digits and _, such as anthropic`;
  }
  if (bill.gross - bill.discount - bill.credit !== bill.net) {
    const m = (nanos: bigint) => moneyText(nanos, bill.currency);
    return `gross minus discount minus credit must equal net: ${m(bill.gross)} - ${m(bill.discount)} - ${m(bill.credit)} is not ${m(bill.net)}`;
  }
  return null;
}

/**
 * Stores a bill as a new revision when its figures differ from the latest
 * one, so sending the same bill twice changes nothing. Check it first.
 */
function recordBill(db: Db, bill: NewBill, recordedAt: string): { view: BillView; changed: boolean } {
  const reporting = db.organization.reporting_currency;
  const given = bill.rate !== null;
  const rate = bill.rate ?? (bill.currency === reporting ? null : BigInt(convert(1e9, bill.currency, reporting)));
  let record = db.bills.find((b) => b.period === bill.period && b.provider === bill.provider && b.cost_source === bill.cost_source);
  const before = record ? latest(record) : null;
  const same =
    before !== null &&
    before.currency === bill.currency &&
    BigInt(before.gross) === bill.gross &&
    BigInt(before.discount) === bill.discount &&
    BigInt(before.credit) === bill.credit &&
    BigInt(before.net) === bill.net &&
    JSON.stringify(before.lines ?? []) === JSON.stringify(bill.lines) &&
    (!given || (before.exchange_rate !== null && BigInt(before.exchange_rate) === rate));
  if (record && before && same) return { view: billView(record, before), changed: false };
  if (!record) {
    record = { id: newId(db), period: bill.period, provider: bill.provider, cost_source: bill.cost_source, revisions: [] };
    db.bills.push(record);
  }
  const revision: BillRevisionRecord = {
    revision: (before?.revision ?? 0) + 1,
    currency: bill.currency,
    gross: Number(bill.gross),
    discount: Number(bill.discount),
    credit: Number(bill.credit),
    net: Number(bill.net),
    exchange_rate: rate === null ? null : Number(rate),
    exchange_rate_given: given,
    source: bill.source,
    recorded_at: recordedAt,
    ...(bill.lines.length ? { lines: bill.lines } : {}),
  };
  record.revisions.push(revision);
  return { view: billView(record, revision), changed: true };
}

/** A bill the bills endpoint was sent, read as the service reads it. */
function readBillInput(input: BillInput): NewBill | string {
  const currency = input.currency;
  const invalid = (text: string) => `${quoted(text)} is not an amount: write it as a decimal number, such as 1234.56`;
  if (!/^[A-Z]{3}$/.test(currency)) return invalid(currency);
  const read = (text: string | null | undefined) => (text === null || text === undefined ? 0n : parseNanos(text));
  const net = parseNanos(input.net);
  if (net === null) return invalid(input.net);
  const discount = read(input.discount);
  if (discount === null) return invalid(input.discount ?? "");
  const credit = read(input.credit);
  if (credit === null) return invalid(input.credit ?? "");
  let gross = net + discount + credit;
  if (input.gross !== null && input.gross !== undefined) {
    const given = parseNanos(input.gross);
    if (given === null) return invalid(input.gross);
    gross = given;
  }
  let rate: bigint | null = null;
  if (input.exchange_rate !== null && input.exchange_rate !== undefined) {
    rate = parseNanos(input.exchange_rate, { plus: false });
    if (rate === null || rate <= 0n) return `${quoted(input.exchange_rate)} is not an exchange rate: write it as a decimal number above zero, such as 0.9214`;
  }
  return {
    period: input.period,
    provider: input.provider,
    cost_source: input.cost_source,
    currency,
    gross,
    discount,
    credit,
    net,
    rate,
    source: input.typed_in ? "manual" : "api",
    lines: input.lines ?? [],
  };
}

/** A cloud export's line as the service keeps it on the bill. */
function lineJson(line: CsvBill["lines"][number], currency: string): Record<string, string | null> {
  return { date: line.date, workspace: line.workspace, api_key_id: null, model: null, description: line.description, amount: plainDecimal(line.amount), currency };
}

function shownCsvBill(bill: CsvBill): BillCsvBill {
  const money = (nanos: bigint) => dec(nanos, bill.currency);
  return {
    period: bill.period,
    provider: bill.provider,
    cost_source: bill.cost_source,
    currency: bill.currency,
    gross: money(bill.gross),
    discount: money(bill.discount),
    credit: money(bill.credit),
    net: money(bill.net),
    exchange_rate: bill.exchange_rate,
    lines: bill.lines.length,
  };
}

// One period's account: its cost lines in the reporting currency, its bills,
// adjustments and tag map. An open period reads the latest of each; a closed
// one reads what it closed with.

interface AccountBill {
  record: BillRecord;
  revision: BillRevisionRecord;
  /** From the bill's currency to the reporting currency, in nano-units. */
  rate: bigint;
}

interface Line {
  entry: EntryRecord;
  tags: Tags;
  /** In the reporting currency; null for an unknown model. */
  value: bigint | null;
  /** The bill that charges it, by index; null when its provider has no bill this period. */
  bill: number | null;
}

interface Account {
  period: string;
  currency: string;
  approval: boolean;
  metered: boolean;
  closed: PeriodRecord | null;
  ladder: string[];
  tagMapVersion: number;
  lines: Line[];
  bills: AccountBill[];
  restated: BillView[];
  adjustments: AdjustmentRecord[];
}

const closedRecord = (db: Db, period: string) => db.periods.find((p) => p.id === period && p.status === "closed") ?? null;

function rateOf(db: Db, revision: BillRevisionRecord): bigint {
  const reporting = db.organization.reporting_currency;
  if (revision.currency === reporting) return NANO;
  return BigInt(revision.exchange_rate ?? convert(1e9, revision.currency, reporting));
}

/** Whether a token entry's cost counts on the cost sheet: with approval on, approved ones only. */
function counts(status: EntryRecord["status"], approval: boolean): boolean {
  if (status === "approved" || status === "locked") return true;
  return status === "submitted" && !approval;
}

function account(db: Db, period: string): Account {
  const currency = db.organization.reporting_currency;
  const closed = closedRecord(db, period);
  const forPeriod = db.bills.filter((b) => b.period === period);
  let bills: AccountBill[];
  let restated: BillView[] = [];
  if (closed) {
    bills = (closed.bills ?? []).flatMap(({ bill, revision }) => {
      const record = db.bills.find((b) => b.id === bill);
      const kept = record?.revisions.find((r) => r.revision === revision);
      return record && kept ? [{ record, revision: kept, rate: rateOf(db, kept) }] : [];
    });
    restated = latestBills(db, period).filter((view) => !bills.some((b) => b.record.id === view.id && b.revision.revision === view.revision));
  } else {
    bills = forPeriod.map((record) => ({ record, revision: latest(record), rate: rateOf(db, latest(record)) }));
  }
  // Per provider, the first cost source charges its token entries.
  bills.sort((a, b) => compare(a.record.provider, b.record.provider) || BILL_SOURCES.indexOf(a.record.cost_source) - BILL_SOURCES.indexOf(b.record.cost_source));
  const charging = new Map<string, number>();
  bills.forEach((b, index) => {
    if (!charging.has(b.record.provider)) charging.set(b.record.provider, index);
  });

  // A late token sheet reaches a closed period as an adjustment, not as a line.
  const statuses: EntryRecord["status"][] = closed ? ["locked"] : ["submitted", "approved", "locked"];
  const lines = db.entries
    .filter((e) => e.period === period && statuses.includes(e.status) && !e.late)
    .flatMap((entry) => {
      const values = entry.list === null ? null : allocate(entry.list, entry.splits.map((s) => s.share));
      return entry.splits.map((split, i) => ({ entry, tags: split.tags, value: values ? BigInt(values[i] ?? 0) : null, bill: charging.get(entry.provider) ?? null }));
    });

  return {
    period,
    currency,
    approval: closed ? (closed.approval ?? db.organization.approval) : db.organization.approval,
    metered: !["seat", "subscription"].includes(db.organization.cost_source ?? ""),
    closed,
    ladder: closed?.tag_map?.ladder ?? db.tagMap.ladder,
    tagMapVersion: closed ? (closed.tag_map_version ?? db.tagMap.version) : db.tagMap.version,
    lines,
    bills,
    restated,
    adjustments: closed ? db.adjustments.filter((a) => a.period === period).sort((a, b) => compare(a.posted_at, b.posted_at) || compare(a.id, b.id)) : [],
  };
}

const counted = (acc: Account) => acc.lines.filter((line) => counts(line.entry.status, acc.approval));

// The core's cost sheet: per bill, its lines by cost kind and cost object in
// the reporting currency, plus residual.

interface BlockLine {
  kind: Kind;
  rungs: (string | null)[];
  columns: (string | null)[];
  /** In the reporting currency. */
  amount: bigint;
  unknown: boolean;
}

interface Block {
  bill: AccountBill;
  name: string;
  lines: BlockLine[];
  /** What token entries explain and what they do not, in the bill's currency. */
  explained: bigint;
  unexplained: bigint;
  /** In the reporting currency, to the nano-unit. */
  residual: bigint;
  gross: bigint;
  discount: bigint;
  credit: bigint;
  net: bigint;
}

const compareValues = (a: (string | null)[], b: (string | null)[]) => {
  for (let i = 0; i < Math.max(a.length, b.length); i += 1) {
    const x = a[i] ?? null;
    const y = b[i] ?? null;
    if (x === y) continue;
    if (x === null) return -1;
    if (y === null) return 1;
    return compare(x, y);
  }
  return 0;
};

function buildBlocks(acc: Account, wanted: string[]): { rungs: string[]; columns: string[]; blocks: Block[] } {
  const rungs = [...acc.ladder].reverse();
  const columns = wanted.filter((c, i) => !rungs.includes(c) && wanted.indexOf(c) === i);
  const lines = counted(acc);
  const blocks = acc.bills.map((bill, index): Block => {
    const currency = bill.revision.currency;
    const groups: BlockLine[] = [];
    for (const line of lines.filter((l) => l.bill === index)) {
      const kind = costKind(line.tags, acc.ladder);
      const rungValues = rungs.map((r) => line.tags[r] ?? null);
      const columnValues = columns.map((c) => line.tags[c] ?? null);
      const list = line.value === null ? 0n : BigInt(convert(Number(line.value), acc.currency, currency));
      const group = groups.find((g) => g.kind === kind && compareValues(g.rungs, rungValues) === 0 && compareValues(g.columns, columnValues) === 0);
      if (group) {
        group.amount += list;
        group.unknown ||= line.value === null;
      } else groups.push({ kind, rungs: rungValues, columns: columnValues, amount: list, unknown: line.value === null });
    }
    groups.sort((a, b) => KINDS.indexOf(a.kind) - KINDS.indexOf(b.kind) || compareValues(a.rungs, b.rungs) || compareValues(a.columns, b.columns));
    const explained = sum(groups.map((g) => g.amount));
    const unexplained = BigInt(bill.revision.gross) - explained;
    const converted = convertAll([...groups.map((g) => g.amount), unexplained], bill.rate);
    const shown = groups.map((g, i) => ({ ...g, amount: converted[i] ?? 0n }));
    const gross = sum(converted);
    const discount = divideRounded(BigInt(bill.revision.discount) * bill.rate, NANO);
    const credit = divideRounded(BigInt(bill.revision.credit) * bill.rate, NANO);
    return {
      bill,
      name: `${bill.record.provider}-${bill.record.cost_source}-${acc.period}`,
      lines: shown,
      explained,
      unexplained,
      residual: converted[converted.length - 1] ?? 0n,
      gross,
      discount,
      credit,
      net: gross - discount - credit,
    };
  });
  return { rungs, columns, blocks };
}

/** `part` as a whole percent of `whole`, or null when there is nothing to share. */
function percent(part: bigint, whole: bigint): number | null {
  if (whole <= 0n) return null;
  const share = ((part > 0n ? part : 0n) * 100n) / whole;
  return Number(share > 100n ? 100n : share);
}

function reconcile(acc: Account, blocks: Block[]): Reconciliation {
  const onBills = new Map<number, { count: number; unknown: number }>();
  const without = new Map<string, { amount: bigint; count: number; unknown: number }>();
  for (const line of counted(acc)) {
    const unknown = line.value === null ? 1 : 0;
    if (line.bill !== null) {
      const entry = onBills.get(line.bill) ?? { count: 0, unknown: 0 };
      onBills.set(line.bill, { count: entry.count + 1, unknown: entry.unknown + unknown });
    } else {
      const entry = without.get(line.entry.provider) ?? { amount: 0n, count: 0, unknown: 0 };
      without.set(line.entry.provider, { amount: entry.amount + (line.value ?? 0n), count: entry.count + 1, unknown: entry.unknown + unknown });
    }
  }
  const bills = blocks.map((block, index): BillCheck => {
    const { record, revision } = block.bill;
    const c = revision.currency;
    const { count, unknown } = onBills.get(index) ?? { count: 0, unknown: 0 };
    return {
      bill: billView(record, revision),
      currency: c,
      gross: dec(BigInt(revision.gross), c),
      discount: dec(BigInt(revision.discount), c),
      credit: dec(BigInt(revision.credit), c),
      net: dec(BigInt(revision.net), c),
      token_entries: dec(block.explained, c),
      residual: dec(block.unexplained, c),
      token_entry_count: count,
      unknown_model_count: unknown,
      attribution_percent: percent(block.explained, BigInt(revision.gross)),
    };
  });
  const gross = sum(blocks.map((b) => b.gross));
  const residual = sum(blocks.map((b) => b.residual));
  const withoutBill = [...without.entries()]
    .sort(([a], [b]) => compare(a, b))
    .map(([provider, w]): WithoutBill => ({ provider, token_entries: dec(w.amount, acc.currency), token_entry_count: w.count, unknown_model_count: w.unknown }));
  return {
    period: acc.period,
    closed: acc.closed !== null,
    reporting_currency: acc.currency,
    metered: acc.metered,
    bills,
    without_bill: withoutBill,
    totals: {
      token_entries: dec(gross - residual, acc.currency),
      residual: dec(residual, acc.currency),
      discount: dec(sum(blocks.map((b) => b.discount)), acc.currency),
      credit: dec(sum(blocks.map((b) => b.credit)), acc.currency),
      net: dec(sum(blocks.map((b) => b.net)), acc.currency),
      estimates: dec(sum([...without.values()].map((w) => w.amount)), acc.currency),
    },
    restated_after_close: acc.restated,
  };
}

export function reconciliationOf(db: Db, period: string): Reconciliation {
  const acc = account(db, period);
  return reconcile(acc, buildBlocks(acc, []).blocks);
}

export function costSheetOf(db: Db, period: string, wanted: string[] = []): CostSheet {
  const acc = account(db, period);
  const { rungs, columns, blocks } = buildBlocks(acc, wanted);
  const c = acc.currency;
  const kinds = new Map<Kind, bigint>();
  const projects = new Map<string, { direct: bigint; overhead: bigint }>();
  let total = 0n;
  const named = (names: string[], values: (string | null)[]) =>
    Object.fromEntries(
      names.flatMap((name, i): [string, string][] => {
        const value = values[i];
        return value ? [[name, value]] : [];
      }),
    );
  const blockViews = blocks.map((block): BillBlock => {
    const shown = roundAllToMinor([...block.lines.map((l) => l.amount), block.residual], c);
    const lines = block.lines.map((line, i): CostRow => {
      const minor = shown[i] ?? 0n;
      kinds.set(line.kind, (kinds.get(line.kind) ?? 0n) + minor);
      const project = line.rungs[rungs.indexOf("project")];
      if (rungs.includes("project") && project) {
        const p = projects.get(project) ?? { direct: 0n, overhead: 0n };
        projects.set(project, line.kind === "direct" ? { ...p, direct: p.direct + minor } : { ...p, overhead: p.overhead + minor });
      }
      return { cost_kind: line.kind, rungs: named(rungs, line.rungs), columns: named(columns, line.columns), amount: minorText(minor, c), figure: line.unknown ? "unknown_model" : "list_price" };
    });
    const gross = sum(shown);
    const discount = divideRounded(block.discount, minorStep(c));
    const credit = divideRounded(block.credit, minorStep(c));
    const net = gross - discount - credit;
    total += net;
    return {
      bill: block.name,
      provider: block.bill.record.provider,
      revision: block.bill.revision.revision,
      exchange_rate: rateText(block.bill.rate),
      lines,
      residual: minorText(shown[shown.length - 1] ?? 0n, c),
      gross: minorText(gross, c),
      discount: minorText(discount, c),
      credit: minorText(credit, c),
      net: minorText(net, c),
    };
  });
  const shownAdjustments = roundAllToMinor(acc.adjustments.map((a) => BigInt(a.amount)), c);
  const adjustments = acc.adjustments.map((a, i): AdjustmentRow => {
    const minor = shownAdjustments[i] ?? 0n;
    total += minor;
    return { id: a.id, amount: minorText(minor, c), cost_kind: costKind(a.tags, acc.ladder), tags: a.tags, reason: a.reason, late: a.late, posted_by: a.posted_by ? nameOf(db, a.posted_by) : null, posted_at: a.posted_at };
  });
  const all = sum([...kinds.values()]);
  const kind = (k: Kind) => kinds.get(k) ?? 0n;
  const kindTotals: KindTotals = {
    direct: minorText(kind("direct"), c),
    project_overhead: minorText(kind("project_overhead"), c),
    organization_overhead: minorText(kind("organization_overhead"), c),
    direct_percent: percent(kind("direct"), all),
    project_overhead_percent: percent(kind("project_overhead"), all),
    organization_overhead_percent: percent(kind("organization_overhead"), all),
  };
  return {
    period,
    closed: acc.closed !== null,
    closed_at: acc.closed?.closed_at ?? null,
    currency: c,
    approval: acc.approval,
    tag_map_version: acc.tagMapVersion,
    rungs,
    columns,
    blocks: blockViews,
    projects: [...projects.entries()]
      .sort(([a], [b]) => compare(a, b))
      .map(([project, p]): ProjectTotal => ({ project, direct: minorText(p.direct, c), project_overhead: minorText(p.overhead, c), total: minorText(p.direct + p.overhead, c) })),
    kinds: kindTotals,
    adjustments,
    reconciliation: reconcile(acc, blocks),
    total: minorText(total, c),
  };
}

/** A leading minus is a formula unless the cell is a plain number. */
function csvCell(cell: string): string {
  const formula = /^[=+@\t\r]/.test(cell) || (cell.startsWith("-") && (cell.length === 1 || !/^[\d.]+$/.test(cell.slice(1))));
  const defused = formula ? `'${cell}` : cell;
  return /[,"\r\n]/.test(defused) ? `"${defused.replace(/"/g, '""')}"` : defused;
}

/** The cost sheet as the service's CSV download: one row per line, amounts in the minor unit. */
export function costSheetCsvOf(db: Db, period: string, wanted: string[] = []): string {
  const acc = account(db, period);
  const { rungs, columns, blocks } = buildBlocks(acc, wanted);
  const c = acc.currency;
  const rows: string[][] = [["bill", "line", "cost_kind", ...rungs, ...columns, "amount", "note"]];
  const empty = () => [...rungs, ...columns].map(() => "");
  let total = 0n;
  for (const block of blocks) {
    const shown = roundAllToMinor([...block.lines.map((l) => l.amount), block.residual], c);
    block.lines.forEach((line, i) => {
      rows.push([block.name, "cost", KIND_LABEL[line.kind], ...[...line.rungs, ...line.columns].map((v) => v ?? ""), minorText(shown[i] ?? 0n, c), line.unknown ? "unknown model" : ""]);
    });
    const gross = sum(shown);
    const discountMinor = divideRounded(block.discount, minorStep(c));
    const creditMinor = divideRounded(block.credit, minorStep(c));
    const net = gross - discountMinor - creditMinor;
    const summary = (line: string, minor: bigint) => rows.push([block.name, line, "", ...empty(), minorText(minor, c), ""]);
    summary("residual", shown[shown.length - 1] ?? 0n);
    summary("gross", gross);
    summary("discount", discountMinor);
    summary("credit", creditMinor);
    summary("net", net);
    total += net;
  }
  const shownAdjustments = roundAllToMinor(acc.adjustments.map((a) => BigInt(a.amount)), c);
  acc.adjustments.forEach((a, i) => {
    const minor = shownAdjustments[i] ?? 0n;
    rows.push(["", a.late ? "late adjustment" : "adjustment", KIND_LABEL[costKind(a.tags, acc.ladder)], ...[...rungs, ...columns].map((name) => a.tags[name] ?? ""), minorText(minor, c), a.reason]);
    total += minor;
  });
  rows.push(["", "total", "", ...empty(), minorText(total, c), ""]);
  return rows.map((row) => `${row.map(csvCell).join(",")}\r\n`).join("");
}

// Periods and the close

const thisMonth = () => TODAY.slice(0, 7);
const ended = (period: string) => period < thisMonth();

function nextMonth(period: string): string {
  const [year = 0, month = 1] = period.split("-").map(Number);
  return month === 12 ? `${String(year + 1)}-01` : `${String(year)}-${String(month + 1).padStart(2, "0")}`;
}

/** Every period from the first with a token entry or a bill up to this month, newest first. */
export function periodsOf(db: Db): PeriodSummary[] {
  const firsts = [...db.entries.map((e) => e.started_at.slice(0, 7)), ...db.bills.map((b) => b.period)].sort();
  const current = thisMonth();
  const periods: PeriodSummary[] = [];
  for (let period = firsts[0] ?? current; period <= current; period = nextMonth(period)) {
    const closed = closedRecord(db, period);
    periods.push({
      period,
      ended: ended(period),
      closed: closed !== null,
      closed_at: closed?.closed_at ?? null,
      closed_by: closed ? (db.people.find((p) => p.id === closed.closed_by)?.name ?? null) : null,
    });
  }
  return periods.reverse();
}

export function periodViewOf(db: Db, period: string): PeriodView {
  const acc = account(db, period);
  const entries = db.entries.filter((e) => e.period === period && e.status !== "draft");
  const unapproved = acc.approval ? entries.filter((e) => e.status === "submitted" || e.status === "rejected").length : 0;
  const lines = counted(acc);
  const overhead = lines.filter((l) => costKind(l.tags, acc.ladder) === "organization_overhead");
  const without = new Map<string, number>();
  for (const line of lines.filter((l) => l.bill === null)) without.set(line.entry.provider, (without.get(line.entry.provider) ?? 0) + 1);
  const notFinal = acc.bills.filter((b) => !(b.revision.final ?? true)).map((b) => b.record.provider);
  const blockers: string[] = [];
  if (acc.closed) blockers.push(`${period} is already closed. Corrections are adjustments.`);
  else {
    if (!ended(period)) blockers.push(`${period} is not over yet.`);
    if (unapproved) blockers.push(`${String(unapproved)} token entries are not approved yet. With approval on, every token entry must be approved before the close.`);
    for (const provider of notFinal) blockers.push(`The bill from ${provider} for ${period} is not final yet. The close waits until ${provider} stops changing its figures.`);
    if (acc.metered) {
      for (const [provider, count] of [...without.entries()].sort(([a], [b]) => compare(a, b))) {
        blockers.push(`There is no bill from ${provider} for ${period} yet, so ${String(count)} token entries are on no bill. Send the bill from the API, as CSV or typed in.`);
      }
    }
  }
  return {
    period,
    ended: ended(period),
    closed: acc.closed !== null,
    closed_at: acc.closed?.closed_at ?? null,
    closed_by: acc.closed ? nameOf(db, acc.closed.closed_by) : null,
    approval: acc.approval,
    open_items: {
      bills: acc.bills.length,
      providers_without_bill: [...without.keys()].sort(compare),
      bills_not_final: notFinal,
      organization_overhead: dec(sum(overhead.map((l) => l.value ?? 0n)), acc.currency),
      currency: acc.currency,
      organization_overhead_entries: overhead.length,
      late_token_sheets: lateSheets(db, period).length,
      unapproved,
      rejected: entries.filter((e) => e.status === "rejected").length,
      without_agent_manager: entries.filter((e) => e.agent_manager === null).length,
      unknown_model: lines.filter((l) => l.value === null).length,
    },
    can_close: blockers.length === 0,
    blockers,
  };
}

/** Closes a period: locks its token entries and keeps the tag map, approval and bills it closed with. */
function closePeriod(db: Db, period: string, controller: string, at: string): void {
  const bills = account(db, period).bills.map((b) => ({ bill: b.record.id, revision: b.revision.revision }));
  const closed: PeriodRecord = {
    id: period,
    status: "closed",
    closed_at: at,
    closed_by: controller,
    tag_map: { cost_center: db.tagMap.cost_center, ladder: [...db.tagMap.ladder] },
    tag_map_version: db.tagMap.version,
    approval: db.organization.approval,
    bills,
  };
  const index = db.periods.findIndex((p) => p.id === period);
  if (index === -1) {
    db.periods.push(closed);
    db.periods.sort((a, b) => compare(a.id, b.id));
  } else db.periods[index] = closed;
  for (const entry of db.entries) {
    if (entry.period === period && (entry.status === "submitted" || entry.status === "approved")) entry.status = "locked";
  }
}

/** Late token sheets as the service lists them: oldest first, then by name. */
function lateSheetsOf(db: Db, me: Person, period: string | null): LateSheet[] {
  const people = new Map(db.people.map((p) => [p.id, p]));
  return lateSheets(db, period ?? undefined)
    .flatMap((sheet): LateSheet[] => {
      const person = people.get(sheet.person);
      if (!person || person.status === "removed") return [];
      return [{ person_id: person.id, person: person.name, email: person.email, period_start: sheet.starts, period_end: sheet.ends }];
    })
    .filter((sheet) => me.role === "controller" || sheet.person_id === me.id || me.filters.some((filter) => matches(people.get(sheet.person_id)?.tags ?? {}, filter)))
    .sort((a, b) => compare(a.period_start, b.period_start) || compare(a.person, b.person));
}

// Links for finance

function secret(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return [...bytes].map((b) => b.toString(16).padStart(2, "0")).join("");
}

const BILL_API_NAME: Record<BillApi, string> = { anthropic_enterprise: "Anthropic Enterprise", anthropic_console: "Anthropic Console", openai: "OpenAI" };

export function closeHandlers({ API, member, fail, load, now }: { API: string; member: Member; fail: Fail; load: () => Db; now: () => string }) {
  const notFound = () => fail(404, "not_found", "not found");
  const badRequest = (message: string) => fail(400, "bad_request", message);
  const conflict = (message: string) => fail(409, "conflict", message);
  /** What axum answers when a query or body does not deserialize: plain text, no JSON. */
  const rejected = (status: number, message: string) => new HttpResponse(message, { status, headers: { "content-type": "text/plain; charset=utf-8" } });

  /** The cost sheet as JSON, or as CSV with `format=csv`; `columns` adds tag columns. */
  /** A link's cost sheet also names the organization, since finance is not signed in. */
  const sheetResponse = (db: Db, period: string, url: URL, status = 200, shared = false) => {
    const columns = (url.searchParams.get("columns") ?? "").split(",").map((c) => c.trim()).filter(Boolean);
    const wrong = columns.find((c) => !isTagName(c));
    if (wrong) return badRequest(tagNameProblem(wrong));
    if (url.searchParams.get("format") === "csv") {
      return new HttpResponse(costSheetCsvOf(db, period, columns), {
        headers: { "content-type": "text/csv; charset=utf-8", "content-disposition": `attachment; filename="cost-sheet-${period}.csv"` },
      });
    }
    const sheet = costSheetOf(db, period, columns);
    return HttpResponse.json(shared ? { organization: db.organization.name, ...sheet } : sheet, { status });
  };

  /** The `?period=` of a list: absent is fine, malformed is axum's 400. */
  const queryPeriod = (url: URL): string | null | Response => {
    const period = url.searchParams.get("period");
    if (period === null) return null;
    return isPeriod(period) ? period : rejected(400, `Failed to deserialize query string: ${periodProblem(period)}`);
  };

  /** Reads a CSV upload: our own format, or one cloud's export as one period's bill. */
  const readCsv = async (url: URL, request: Request): Promise<CsvBill[] | CsvBillProblem | Response> => {
    const cloud = url.searchParams.get("cloud");
    const period = queryPeriod(url);
    if (period instanceof Response) return period;
    if (cloud !== null && !CLOUDS.includes(cloud as Cloud)) {
      return rejected(400, `Failed to deserialize query string: cloud: unknown variant \`${cloud}\`, expected one of \`bedrock\`, \`vertex\`, \`foundry\``);
    }
    const text = await request.text();
    try {
      if (cloud === null) return parseBills(text);
      if (period === null) return new CsvBillProblem(null, null, "Give the period the export is the bill for, such as 2026-09.");
      return [parseCloudExport(cloud as Cloud, text, period)];
    } catch (error) {
      if (error instanceof CsvBillProblem) return error;
      throw error;
    }
  };

  return [
    // Bills
    http.get(`${API}/bills`, member(({ db, url }) => {
      const period = queryPeriod(url);
      if (period instanceof Response) return period;
      return HttpResponse.json(latestBills(db, period ?? undefined));
    }, CONTROLLER)),
    http.post(`${API}/bills`, member(async ({ db, request }) => {
      const input = (await request.json()) as BillInput;
      if (!isPeriod(input.period)) return rejected(422, `Failed to deserialize the JSON body into the target type: period: ${periodProblem(input.period)}`);
      if (!BILL_SOURCES.includes(input.cost_source)) {
        return rejected(422, `Failed to deserialize the JSON body into the target type: cost_source: unknown variant \`${input.cost_source}\`, expected one of \`usage_at_api_rates\`, \`api_key\`, \`cloud_marketplace\`, \`reseller\``);
      }
      const bill = readBillInput(input);
      if (typeof bill === "string") return badRequest(bill);
      const problem = billProblem(bill);
      if (problem) return badRequest(problem);
      const { view, changed } = recordBill(db, bill, now());
      return HttpResponse.json(view, { status: changed ? 201 : 200 });
    }, CONTROLLER)),
    http.post(`${API}/bills/csv/preview`, member(async ({ url, request }) => {
      const read = await readCsv(url, request);
      if (read instanceof Response) return read;
      if (read instanceof CsvBillProblem) return HttpResponse.json({ bills: [], problem: { row: read.row, column: read.column, message: read.message } });
      return HttpResponse.json({ bills: read.map(shownCsvBill), problem: null });
    }, CONTROLLER)),
    http.post(`${API}/bills/csv`, member(async ({ db, url, request }) => {
      const read = await readCsv(url, request);
      if (read instanceof Response) return read;
      if (read instanceof CsvBillProblem) return badRequest(`${read.text} Nothing was saved.`);
      const bills: NewBill[] = [];
      for (const bill of read) {
        let rate: bigint | null = null;
        if (bill.exchange_rate !== null) {
          rate = parseNanos(bill.exchange_rate, { plus: false });
          if (rate === null) return badRequest(`${quoted(bill.exchange_rate)} is not an exchange rate: write it as a decimal number above zero, such as 0.9214 Nothing was saved.`);
        }
        bills.push({ ...bill, rate, source: "csv", lines: bill.lines.map((line) => lineJson(line, bill.currency)) });
      }
      // The service stores bill by bill; the mock checks them all first.
      const problem = bills.map(billProblem).find(Boolean);
      if (problem) return badRequest(problem);
      const stored = bills.map((bill) => recordBill(db, bill, now()));
      return HttpResponse.json({ bills: stored.map((s) => s.view), changed: stored.filter((s) => s.changed).length }, { status: 201 });
    }, CONTROLLER)),
    http.get(`${API}/bills/:id`, member(({ db, params }) => {
      const bill = db.bills.find((b) => b.id === params.id);
      return bill?.revisions.length ? HttpResponse.json(bill.revisions.map((r) => billView(bill, r))) : notFound();
    }, CONTROLLER)),

    // Bill connections: a provider's admin API, pulled once a day.
    http.get(`${API}/bill-connections`, member(({ db }) => HttpResponse.json(db.connections), CONTROLLER)),
    http.post(`${API}/bill-connections`, member(async ({ db, request }) => {
      const body = (await request.json()) as NewBillConnection;
      if (!(body.provider in BILL_API_NAME)) {
        return rejected(422, `Failed to deserialize the JSON body into the target type: provider: unknown variant \`${body.provider}\`, expected one of \`anthropic_enterprise\`, \`anthropic_console\`, \`openai\``);
      }
      const key = body.key.trim();
      if (!key) return badRequest(`Add an admin key from ${BILL_API_NAME[body.provider]}.`);
      const connection: BillConnectionView = { id: newId(db), provider: body.provider, key_hint: `…${Array.from(key).slice(-4).join("")}`, last_pull_at: null, status: "ok", message: null };
      db.connections.push(connection);
      return HttpResponse.json(connection, { status: 201 });
    }, CONTROLLER)),
    http.delete(`${API}/bill-connections/:id`, member(({ db, params }) => {
      const before = db.connections.length;
      db.connections = db.connections.filter((c) => c.id !== params.id);
      return db.connections.length < before ? new HttpResponse(null, { status: 204 }) : notFound();
    }, CONTROLLER)),
    http.post(`${API}/bill-connections/:id/pull`, member(({ db, params }) => {
      const connection = db.connections.find((c) => c.id === params.id);
      // The service's job pulls within a minute; the mock has nothing to pull.
      return connection ? HttpResponse.json(connection, { status: 202 }) : notFound();
    }, CONTROLLER)),

    http.get(`${API}/reconciliation`, member(({ db, url }) => {
      const period = url.searchParams.get("period") ?? "";
      if (!isPeriod(period)) return rejected(400, `Failed to deserialize query string: ${period ? periodProblem(period) : "missing field `period`"}`);
      return HttpResponse.json(reconciliationOf(db, period));
    }, CONTROLLER)),

    // Late token sheets: the controller sees everyone's, others their own and their filters'.
    http.get(`${API}/token-sheets/late`, member(({ db, me, url }) => {
      const period = queryPeriod(url);
      if (period instanceof Response) return period;
      return HttpResponse.json(lateSheetsOf(db, me, period));
    })),

    // Periods, the close and the cost sheet
    // Everyone can list the periods, to pick one.
    http.get(`${API}/periods`, member(({ db }) => HttpResponse.json(periodsOf(db)))),
    http.get(`${API}/periods/:period`, member(({ db, params }) => {
      const period = params.period ?? "";
      return isPeriod(period) ? HttpResponse.json(periodViewOf(db, period)) : badRequest(periodProblem(period));
    }, CONTROLLER)),
    http.post(`${API}/periods/:period/close`, member(({ db, me, params, url }) => {
      const period = params.period ?? "";
      if (!isPeriod(period)) return badRequest(periodProblem(period));
      const blocker = periodViewOf(db, period).blockers[0];
      if (blocker) return conflict(blocker);
      closePeriod(db, period, me.id, now());
      return sheetResponse(db, period, new URL(url.pathname, url.origin));
    }, CONTROLLER)),
    http.get(`${API}/periods/:period/cost-sheet`, member(({ db, params, url }) => {
      const period = params.period ?? "";
      return isPeriod(period) ? sheetResponse(db, period, url) : badRequest(periodProblem(period));
    }, CONTROLLER)),
    http.post(`${API}/periods/:period/adjustments`, member(async ({ db, me, params, request, url }) => {
      const period = params.period ?? "";
      if (!isPeriod(period)) return badRequest(periodProblem(period));
      if (!closedRecord(db, period)) return conflict(`${period} is open: change the token entries themselves. Adjustments correct closed periods.`);
      const input = (await request.json()) as AdjustmentInput;
      const amount = parseNanos(input.amount);
      if (amount === null) return badRequest(`${quoted(input.amount)} is not an amount: write it as a decimal number, such as 1234.56`);
      if (amount === 0n) return badRequest("an adjustment needs an amount other than zero");
      const reason = input.reason.trim();
      if (!reason) return badRequest("an adjustment needs a reason");
      const tags = input.tags ?? {};
      if (Object.entries(tags).some(([name, value]) => !name.trim() || !value.trim())) return badRequest("every tag needs a name and a value");
      db.adjustments.push({ id: newId(db), period, amount: Number(amount), tags, reason, late: false, posted_by: me.id, posted_at: now() });
      return sheetResponse(db, period, new URL(url.pathname, url.origin), 201);
    }, CONTROLLER)),

    // Links for finance, who need no login
    http.post(`${API}/periods/:period/cost-sheet/links`, member(({ db, me, params, url }) => {
      const period = params.period ?? "";
      if (!isPeriod(period)) return badRequest(periodProblem(period));
      if (!closedRecord(db, period)) return conflict(`${period} is not closed yet; only a closed period has a cost sheet to share.`);
      const link = { id: newId(db), period, secret: secret(), created_by: me.id, created_at: now(), revoked_at: null };
      db.links.push(link);
      const view: CostSheetLink = { id: link.id, period, url: `${url.origin}/shared/cost-sheet/${link.secret}`, created_by: me.name, created_at: link.created_at, revoked_at: null };
      return HttpResponse.json(view, { status: 201 });
    }, CONTROLLER)),
    http.get(`${API}/periods/:period/cost-sheet/links`, member(({ db, params }) => {
      const period = params.period ?? "";
      if (!isPeriod(period)) return badRequest(periodProblem(period));
      const links = db.links
        .filter((l) => l.period === period)
        .sort((a, b) => compare(b.created_at, a.created_at))
        .map((l): CostSheetLink => ({ id: l.id, period, url: null, created_by: db.people.find((p) => p.id === l.created_by)?.name ?? "", created_at: l.created_at, revoked_at: l.revoked_at }));
      return HttpResponse.json(links);
    }, CONTROLLER)),
    http.delete(`${API}/cost-sheet-links/:id`, member(({ db, params }) => {
      const link = db.links.find((l) => l.id === params.id);
      if (!link) return notFound();
      link.revoked_at ??= now();
      return new HttpResponse(null, { status: 204 });
    }, CONTROLLER)),
    http.get(`${API}/shared/cost-sheets/:secret`, ({ params, request }) => {
      const key = String(params.secret);
      if (!/^[0-9a-fA-F]{64}$/.test(key)) return notFound();
      const db = load();
      const link = db.links.find((l) => l.secret === key && l.revoked_at === null);
      return link ? sheetResponse(db, link.period, new URL(request.url), 200, true) : notFound();
    }),
  ];
}
