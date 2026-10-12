// Mock answers for token entries as the service has them: the token entries
// table, the token sheet with its review list, fixing tags and splits by
// hand, submit, asking everyone to submit, approval and reject. They follow
// apps/server/src/token_sheets and reports/{entries,table}.rs: the same
// access rules, checks, messages and status codes. The token entries stay in
// the mock db, so the close, bills and reports mocks see every change.

import { http, HttpResponse, type HttpResponseResolver } from "msw";
import type {
  ApproveInput,
  AskedToSubmit,
  CadencePeriod,
  CadenceView,
  Decided,
  EntryView,
  Filter,
  Person,
  Reminded,
  Role,
  ShareInput,
  Submitted,
  Tags,
  TokenEntriesPage,
  TokenEntryStatus,
  TokenSheet,
} from "@/api/contract";
import { toCsv } from "@/lib/csv";
import { TODAY, type Db, type EntryRecord } from "./seed";
import { allocate, cur, dec, matches } from "./store";

type Ctx = { db: Db; me: Person; url: URL; params: Record<string, string>; request: Request };
type Member = (handler: (ctx: Ctx) => Response | Promise<Response>) => HttpResponseResolver;
type Fail = (status: number, code: string, message: string) => Response;

/** The most splits a token entry can be split into by hand. */
export const MAX_SPLITS = 20;
const DAY = 86_400_000;
const rank: Record<Role, number> = { agent_manager: 1, people_manager: 2, controller: 3 };

// Cost lines: one split of one token entry, the shape every view reads.

interface Line {
  entry: EntryRecord;
  index: number;
  /** The split's tags, with `person` always set to the agent manager's email. */
  tags: Tags;
  /** List value in nano-units; null for an unknown model. */
  list: number | null;
}

function linesOf(db: Db, entry: EntryRecord): Line[] {
  const values = entry.list === null ? entry.splits.map(() => null) : allocate(entry.list, entry.splits.map((s) => s.share));
  const email = db.people.find((p) => p.id === entry.agent_manager)?.email;
  return entry.splits.map((split, index) => ({ entry, index, tags: email && !split.tags.person ? { ...split.tags, person: email } : split.tags, list: values[index] ?? null }));
}

/** Every cost line, oldest first. `period` null is all time. */
function allLines(db: Db, statuses: TokenEntryStatus[], period: string | null): Line[] {
  return db.entries
    .filter((e) => statuses.includes(e.status) && (period === null || e.started_at.slice(0, 7) === period))
    .sort((a, b) => a.started_at.localeCompare(b.started_at) || a.id.localeCompare(b.id))
    .flatMap((e) => linesOf(db, e));
}

/** Whether a member sees a cost line (docs: Roles): their own, their filters, or everything. */
function sees(me: Person, line: Line): boolean {
  if (me.role === "controller" || line.entry.agent_manager === me.id) return true;
  return me.role === "people_manager" && me.filters.some((filter) => matches(line.tags, filter));
}

/** Whether a member may approve or reject a cost line: not their own unless a controller, and within a filter of theirs. */
function mayDecide(me: Person, line: Line): boolean {
  if (me.role === "controller") return true;
  return line.entry.agent_manager !== me.id && me.role === "people_manager" && me.filters.some((filter) => matches(line.tags, filter));
}

/** An RFC 3339 time as the service writes it, without milliseconds. */
const rfc3339 = (iso: string) => iso.replace(/\.\d{3}Z$/, "Z");

/** Groups cost lines into token entries, in the order they came. */
function views(db: Db, lines: Line[]): EntryView[] {
  const groups = new Map<string, Line[]>();
  for (const line of lines) groups.set(line.entry.id, [...(groups.get(line.entry.id) ?? []), line]);
  const currency = cur(db);
  return [...groups.values()].flatMap((group) => {
    const first = group[0];
    if (!first) return [];
    const { entry } = first;
    const known = group.every((line) => line.list !== null);
    const total = group.reduce((sum, line) => sum + (line.list ?? 0), 0);
    const view: EntryView = {
      id: entry.id,
      agent_harness: entry.agent_harness,
      agent_name: entry.agent_name,
      model: entry.model,
      provider: entry.provider,
      started_at: rfc3339(entry.started_at),
      ended_at: rfc3339(entry.ended_at),
      tokens: entry.tokens,
      status: entry.status,
      automatic: entry.automatic,
      summary: entry.summary,
      agent_manager: db.people.find((p) => p.id === entry.agent_manager)?.name ?? null,
      agent_manager_id: entry.agent_manager,
      list_value: known ? dec(total, currency) : null,
      currency,
      label: known ? "list-price estimate" : "unknown model",
      rejected_reason: entry.status === "rejected" ? entry.reject_reason : null,
      candidates: entry.candidates,
      splits: group.map((line) => ({
        id: `${entry.id}-${String(line.index + 1)}`,
        list_value: line.list === null ? null : dec(line.list, currency),
        tags: line.tags,
        placed_by: entry.placed_by,
        // Whole percents when split by hand; a whole token entry has none.
        share_percent: entry.placed_by === "by hand" && entry.splits.length > 1 ? Math.round((entry.splits[line.index]?.share ?? 0) / 100) : null,
        unknown_ticket: entry.splits[line.index]?.unknown_ticket ?? null,
      })),
    };
    return [view];
  });
}

/** Token entries as CSV: one row per split, with a column per tag name. */
function entriesCsv(entries: EntryView[]): string {
  const tagNames = [...new Set(entries.flatMap((e) => e.splits.flatMap((s) => Object.keys(s.tags))))].sort();
  const header = ["token_entry", "started_at", "agent_harness", "agent_name", "model", "provider", "agent_manager", "status", "automatic", "summary", ...tagNames, "list_value", "currency", "label"];
  const rows = entries.flatMap((e) =>
    e.splits.map((s) => [
      e.id,
      e.started_at,
      e.agent_harness,
      e.agent_name ?? "",
      e.model,
      e.provider,
      e.agent_manager ?? "",
      e.status,
      e.automatic ? "yes" : "no",
      e.summary,
      ...tagNames.map((name) => s.tags[name] ?? ""),
      s.list_value ?? "",
      e.currency,
      e.label,
    ]),
  );
  return toCsv(header, rows);
}

function csvFile(csv: string, name: string): Response {
  return new HttpResponse(csv, { headers: { "content-type": "text/csv; charset=utf-8", "content-disposition": `attachment; filename="${name}"` } });
}

/** A tag filter in the URL form, such as `client:acme,team:web`, or the service's message. */
function parseFilter(text: string | null | undefined): Filter | string {
  const filter: Filter = {};
  for (const part of (text ?? "").split(",").map((p) => p.trim()).filter(Boolean)) {
    const at = part.indexOf(":");
    const name = at > 0 ? part.slice(0, at).trim() : "";
    const value = at > 0 ? part.slice(at + 1).trim() : "";
    if (!name || !value) return `${JSON.stringify(part)} is not a tag: write it as name:value, such as client:acme`;
    if (name in filter) return `the filter names ${JSON.stringify(name)} twice; a filter has one value per tag name`;
    filter[name] = value;
  }
  return filter;
}

/** The statuses of the token entries table: those that count unless named. Drafts never show there. */
function tableStatuses(text: string | null): TokenEntryStatus[] | string {
  const clean = text?.trim();
  if (!clean) return ["submitted", "approved", "locked"];
  const named: TokenEntryStatus[] = [];
  for (const part of clean.split(",").map((p) => p.trim())) {
    if (part !== "submitted" && part !== "approved" && part !== "rejected" && part !== "locked") return `${JSON.stringify(part)} is not a status: use submitted, approved, rejected or locked`;
    named.push(part);
  }
  return named;
}

/** The submitted token entries a member may decide on, as cost lines: whole token entries where any split is theirs to decide. */
function decidable(db: Db, me: Person, statuses: TokenEntryStatus[], period: string | null, filter: Filter): Line[] {
  const lines = allLines(db, statuses, period).filter((line) => !line.entry.late && matches(line.tags, filter) && sees(me, line));
  const allowed = new Set(lines.filter((line) => mayDecide(me, line)).map((line) => line.entry.id));
  return lines.filter((line) => allowed.has(line.entry.id));
}

// Cadence and token sheets.

const addDays = (day: string, days: number) => new Date(Date.parse(`${day}T00:00:00Z`) + days * DAY).toISOString().slice(0, 10);

function lastDayOf(month: string): string {
  const [year = 1970, number = 1] = month.split("-").map(Number);
  return new Date(Date.UTC(year, number, 0)).toISOString().slice(0, 10);
}

function cadenceDays(cadence: CadenceView): number | null {
  if (cadence.kind === "weekly") return 7;
  if (cadence.kind === "every_two_weeks") return 14;
  if (cadence.kind === "custom") return Math.max(1, cadence.every_days ?? 7);
  return null;
}

/** The cadence period a day falls in; without a cadence, token sheets are calendar months. */
function periodOf(cadence: CadenceView | null, day: string): CadencePeriod {
  const days = cadence ? cadenceDays(cadence) : null;
  if (cadence && days) {
    const steps = Math.floor((Date.parse(`${day}T00:00:00Z`) - Date.parse(`${cadence.starts_on}T00:00:00Z`)) / DAY / days);
    const starts_on = addDays(cadence.starts_on, steps * days);
    return { starts_on, ends_on: addDays(starts_on, days - 1) };
  }
  return { starts_on: `${day.slice(0, 7)}-01`, ends_on: lastDayOf(day.slice(0, 7)) };
}

/** On a token sheet: reviewed and submitted by a person, never automatic. */
const onSheet = (e: EntryRecord) => !e.automatic && e.status !== "draft";

/** The cadence for a person: the shortest of the organization's and those of projects they put on a token sheet in the last 90 days. */
function cadenceFor(db: Db, person: Person): CadenceView | null {
  const since = addDays(TODAY, -90);
  const projects = new Set(
    db.entries.filter((e) => e.agent_manager === person.id && onSheet(e) && e.started_at.slice(0, 10) >= since).flatMap((e) => e.splits.map((s) => s.tags.project ?? "")),
  );
  const length = (c: CadenceView) => cadenceDays(c) ?? 31;
  return db.cadences.filter((c) => !c.project || projects.has(c.project)).sort((a, b) => length(a) - length(b))[0] ?? null;
}

interface SheetRecord extends CadencePeriod {
  person: string;
  last_submitted_at: string;
  entries: string[];
}

/** Token sheets and requests to submit, kept with the mock db. */
interface SheetState {
  organization: string;
  sheets: SheetRecord[];
  requests: { period: string; asked_by: string; asked_at: string }[];
}

/**
 * The example organization's past token sheets: one per cadence period from
 * each person's first token entry on a token sheet to their last, empty ones
 * saying there was nothing to report.
 */
function seedSheets(db: Db): SheetRecord[] {
  const sheets: SheetRecord[] = [];
  for (const person of db.people) {
    const mine = db.entries.filter((e) => e.agent_manager === person.id && onSheet(e)).sort((a, b) => a.started_at.localeCompare(b.started_at));
    const first = mine[0];
    const last = mine[mine.length - 1];
    if (!first || !last) continue;
    const cadence = cadenceFor(db, person);
    const end = periodOf(cadence, last.started_at.slice(0, 10)).starts_on;
    for (let period = periodOf(cadence, first.started_at.slice(0, 10)); period.starts_on <= end; period = periodOf(cadence, addDays(period.ends_on, 1))) {
      const { starts_on, ends_on } = period;
      const entries = mine.filter((e) => e.started_at.slice(0, 10) >= starts_on && e.started_at.slice(0, 10) <= ends_on);
      const lastEnd = entries.map((e) => e.ended_at).sort().at(-1);
      sheets.push({ person: person.id, starts_on, ends_on, last_submitted_at: rfc3339(lastEnd ?? `${ends_on}T16:00:00.000Z`), entries: entries.map((e) => e.id) });
    }
  }
  return sheets;
}

function sheetState(db: Db): SheetState {
  const holder = db as Db & { tokenSheets?: SheetState };
  if (holder.tokenSheets?.organization !== db.organization.id) holder.tokenSheets = { organization: db.organization.id, sheets: seedSheets(db), requests: [] };
  return holder.tokenSheets;
}

/** A person's late token sheets: cadence periods that are over and that no token sheet covers. With a month, those overlapping it. */
function lateSheets(db: Db, person: Person, month?: string): CadencePeriod[] {
  const cadence = cadenceFor(db, person);
  const sheets = sheetState(db).sheets.filter((s) => s.person === person.id);
  const first = sheets.map((s) => s.starts_on).sort()[0];
  if (!cadence || !first) return [];
  const from = month && `${month}-01` > first ? `${month}-01` : first;
  const late: CadencePeriod[] = [];
  for (let period = periodOf(cadence, from); period.ends_on < TODAY; period = periodOf(cadence, addDays(period.ends_on, 1))) {
    if (month && period.starts_on > lastDayOf(month)) break;
    if (!sheets.some((s) => s.starts_on <= period.ends_on && s.ends_on >= period.starts_on)) late.push(period);
  }
  return late.slice(-60);
}

const isClosed = (db: Db, month: string) => db.periods.find((p) => p.id === month)?.status === "closed";

/** List value and share on tickets of cost lines, known models only, in nano-units and basis points. */
function onTickets(lines: Line[]): { total: number; share: number | null } {
  const known = lines.filter((line) => line.list !== null);
  const total = known.reduce((sum, line) => sum + (line.list ?? 0), 0);
  const ticketed = known.filter((line) => line.tags.ticket).reduce((sum, line) => sum + (line.list ?? 0), 0);
  return { total, share: total > 0 ? Math.min(10_000, Math.floor((Math.max(0, ticketed) * 10_000) / total)) : null };
}

/** Whether a split waits for its person: a draft, sent back with a reason, or an unknown ticket. */
const waits = (line: Line) => line.entry.status === "draft" || line.entry.status === "rejected" || Boolean(line.entry.splits[line.index]?.unknown_ticket);

/** The person's token sheet as `GET /token-sheets/current` answers it. */
export function tokenSheetOf(db: Db, me: Person): TokenSheet {
  const cadence = cadenceFor(db, me);
  const running = cadence ? periodOf(cadence, TODAY) : null;
  const waiting = allLines(db, ["draft", "rejected", "submitted", "approved"], null).filter((line) => line.entry.agent_manager === me.id && waits(line));
  const { total, share } = onTickets(waiting.filter((line) => line.entry.status === "draft"));
  const state = sheetState(db);
  const request = [...state.requests].filter((r) => !isClosed(db, r.period)).sort((a, b) => b.asked_at.localeCompare(a.asked_at))[0];
  const asker = db.people.find((p) => p.id === request?.asked_by)?.name.trim();
  const asked: AskedToSubmit | null = request ? { period: request.period, asked_by: asker || null, asked_at: request.asked_at } : null;
  return {
    person: me.name.trim() || me.email,
    cadence: cadence?.kind ?? null,
    period_start: running?.starts_on ?? null,
    period_end: running?.ends_on ?? null,
    due_at: running ? `${addDays(running.ends_on, 1)}T00:00:00Z` : null,
    late: running ? lateSheets(db, me).length > 0 : false,
    asked_to_submit: asked,
    token_entries: views(db, waiting),
    list_value: dec(total, cur(db)),
    currency: cur(db),
    share_on_tickets: share,
    token_sheets: state.sheets
      .filter((s) => s.person === me.id)
      .sort((a, b) => b.starts_on.localeCompare(a.starts_on))
      .slice(0, 52)
      .map((s) => ({ period_start: s.starts_on, period_end: s.ends_on, token_entries: s.entries.length, last_submitted_at: s.last_submitted_at })),
  };
}

/** Submits the person's drafts, as `POST /token-sheets/submit` with no token entries in the body. */
export function submitSheet(db: Db, me: Person, now: string): Submitted {
  const drafts = db.entries.filter((e) => e.agent_manager === me.id && e.status === "draft");
  for (const entry of drafts) {
    entry.status = "submitted";
    entry.automatic = false;
  }
  // Without a cadence, token sheets are kept per calendar month.
  const cadence = cadenceFor(db, me);
  const running = periodOf(cadence, TODAY);
  const state = sheetState(db);
  const mine = state.sheets.filter((s) => s.person === me.id);
  const lastCovered = mine.map((s) => s.ends_on).sort().at(-1);
  const covered = new Map<string, CadencePeriod>();
  for (const entry of drafts) {
    const period = periodOf(cadence, entry.started_at.slice(0, 10));
    covered.set(period.starts_on, period);
  }
  // A submit says everything up to now is reported: every cadence period since the last token sheet.
  const next = lastCovered ? addDays(lastCovered, 1) : running.starts_on;
  for (let period = periodOf(cadence, next < running.starts_on ? next : running.starts_on); period.starts_on <= running.starts_on; period = periodOf(cadence, addDays(period.ends_on, 1))) {
    covered.set(period.starts_on, period);
  }
  const periods = [...covered.values()].sort((a, b) => a.starts_on.localeCompare(b.starts_on)).slice(-400);
  for (const period of periods) {
    const sheet = mine.find((s) => s.starts_on === period.starts_on);
    if (sheet) Object.assign(sheet, { ends_on: sheet.ends_on > period.ends_on ? sheet.ends_on : period.ends_on, last_submitted_at: now });
    else state.sheets.push({ person: me.id, ...period, last_submitted_at: now, entries: [] });
  }
  for (const entry of drafts) {
    const day = entry.started_at.slice(0, 10);
    for (const sheet of state.sheets) sheet.entries = sheet.entries.filter((id) => id !== entry.id);
    state.sheets.find((s) => s.person === me.id && s.starts_on <= day && s.ends_on >= day)?.entries.push(entry.id);
  }
  // A token entry of a closed month is posted to it as a late adjustment and locked.
  let late = 0;
  for (const entry of drafts.filter((e) => isClosed(db, e.period))) {
    // One late adjustment per split, tagged like the split (close/late.rs).
    for (const line of linesOf(db, entry)) {
      if (!line.list) continue;
      db.nextId += 1;
      db.adjustments.push({ id: `adj_${String(db.nextId)}`, period: entry.period, amount: line.list, tags: entry.splits[line.index]?.tags ?? {}, reason: `Late token sheet, submitted after ${entry.period} closed, on ${now.slice(0, 10)}`, late: true, posted_by: null, posted_at: now });
    }
    entry.status = "locked";
    entry.late = true;
    late += 1;
  }
  const { total, share } = onTickets(drafts.flatMap((e) => linesOf(db, e)));
  return {
    token_entries: [],
    submitted: drafts.length,
    list_value: dec(total, cur(db)),
    currency: cur(db),
    share_on_tickets_percent: share === null ? null : Math.floor(share / 100),
    late,
    token_sheets: periods,
  };
}

// Fixing token entries by hand.

/** The service's check of tags set by hand. */
function checkTags(tags: Tags): string | null {
  for (const [name, value] of Object.entries(tags)) {
    if (!/^[a-z][a-z0-9_-]*$/.test(name) || name.length > 200) return `tag name ${JSON.stringify(name)} must be lowercase letters, digits, _ or -, starting with a letter`;
    if (name === "person") return 'tag "person" is set from the token that sends the token entry';
    if (!value.trim()) return `tag ${name} must not be empty`;
    if (Array.from(value).length > 200) return `tag ${name} is longer than 200 characters`;
  }
  return null;
}

/** The tags of a split placed by hand: the person's, the ticket with its project and what they pass on, and the other tags named. */
function placeByHand(db: Db, entry: EntryRecord, named: Tags): Tags {
  const person = db.people.find((p) => p.id === entry.agent_manager);
  const tags: Tags = person ? { person: person.email, ...person.tags } : {};
  const ticket = named.ticket ? db.tickets.find((t) => t.key === named.ticket) : undefined;
  const project = ticket ? db.projects.find((p) => p.id === ticket.project_id) : db.projects.find((p) => p.name === named.project);
  if (project) Object.assign(tags, { project: project.name }, project.tags);
  if (named.ticket) Object.assign(tags, ticket?.tags ?? {}, { ticket: named.ticket });
  for (const [name, value] of Object.entries(named)) {
    if (!["person", "ticket", "project"].includes(name) && db.tagNames.includes(name)) tags[name] = value;
  }
  return tags;
}

/** Applies a fix to one of the caller's own token entries, as the service's `apply_fix`. */
function applyFix(db: Db, me: Person, id: string, fail: Fail, splits: { share: number; named: Tags }[]): Response {
  const entry = db.entries.find((e) => e.id === id);
  if (!entry || entry.agent_manager !== me.id) return fail(404, "not_found", "not found");
  if (entry.status === "locked") return fail(409, "conflict", "its period is closed; a controller can post an adjustment instead");
  if (entry.status === "approved") return fail(409, "conflict", "it is approved; a people manager can send it back to you first");
  // Splits that end up with the same tags are one split.
  const placed: { share: number; tags: Tags }[] = [];
  for (const split of splits) {
    const tags = placeByHand(db, entry, split.named);
    const same = placed.find((p) => JSON.stringify(Object.entries(p.tags).sort()) === JSON.stringify(Object.entries(tags).sort()));
    if (same) same.share += split.share;
    else placed.push({ share: split.share, tags });
  }
  entry.splits = placed;
  entry.placed_by = "by hand";
  entry.candidates = [];
  // A rejected token entry becomes a draft again, to go with the next submit.
  if (entry.status === "rejected") {
    entry.status = "draft";
    entry.reject_reason = null;
  }
  const lines = linesOf(db, entry).filter((line) => ["draft", "rejected", "submitted", "approved"].includes(line.entry.status));
  const [view] = views(db, lines);
  return view ? HttpResponse.json(view) : fail(404, "not_found", "not found");
}

function readJson<T>(request: Request): Promise<T | null> {
  return request.json().then(
    (body: unknown) => body as T,
    () => null,
  );
}

export function tokenEntryHandlers({ API, member, fail, now }: { API: string; member: Member; fail: Fail; now: () => string }) {
  const forbidden = () => fail(403, "forbidden", "your role cannot do this");
  const atLeast = (me: Person, role: Role) => rank[me.role] >= rank[role];

  return [
    // The token entries table: every token entry the caller may see, as JSON pages or one CSV file.
    http.get(`${API}/reports/token-entries`, member(({ db, me, url }) => {
      const filter = parseFilter(url.searchParams.get("filter"));
      if (typeof filter === "string") return fail(400, "bad_request", filter);
      const statuses = tableStatuses(url.searchParams.get("status"));
      if (typeof statuses === "string") return fail(400, "bad_request", statuses);
      const period = url.searchParams.get("period");
      // Only one agent manager's token entries.
      const person = url.searchParams.get("person");
      const lines = allLines(db, statuses, period).filter((line) => matches(line.tags, filter) && sees(me, line) && (person === null || line.entry.agent_manager === person));
      const entries = views(db, lines);
      if (url.searchParams.get("format") === "csv") return csvFile(entriesCsv(entries), period ? `token-entries-${period}.csv` : "token-entries.csv");
      const limit = Math.min(5000, Math.max(1, Number(url.searchParams.get("limit") ?? "200") || 200));
      const offset = Number(url.searchParams.get("offset") ?? "0") || 0;
      const page: TokenEntriesPage = { token_entries: entries.slice(offset, offset + limit), total: entries.length, limit, offset };
      return HttpResponse.json(page);
    })),

    // The review list: drafts, token entries sent back with a reason, and unknown tickets.
    http.get(`${API}/token-sheets/current`, member(({ db, me }) => HttpResponse.json(tokenSheetOf(db, me)))),
    http.patch(`${API}/token-entries/:id/tags`, member(async ({ db, me, params, request }) => {
      const body = await readJson<{ tags?: Tags }>(request);
      if (!body?.tags) return fail(400, "bad_request", "send the tags, such as {\"tags\": {\"ticket\": \"KD-231\"}}");
      const problem = checkTags(body.tags);
      if (problem) return fail(400, "bad_request", problem);
      return applyFix(db, me, params.id ?? "", fail, [{ share: 10_000, named: body.tags }]);
    })),
    http.put(`${API}/token-entries/:id/splits`, member(async ({ db, me, params, request }) => {
      const splits = (await readJson<{ splits?: ShareInput[] }>(request))?.splits ?? [];
      if (splits.length < 2 || splits.length > MAX_SPLITS) return fail(400, "bad_request", `split a token entry into 2 to ${String(MAX_SPLITS)} shares`);
      if (splits.some((s) => !s.share_percent)) return fail(400, "bad_request", "every share must be above 0 percent");
      if (splits.reduce((sum, s) => sum + s.share_percent, 0) !== 100) return fail(400, "bad_request", "the shares must add up to 100 percent");
      for (const split of splits) {
        const problem = checkTags(split.tags ?? {});
        if (problem) return fail(400, "bad_request", problem);
      }
      return applyFix(db, me, params.id ?? "", fail, splits.map((s) => ({ share: s.share_percent * 100, named: s.tags ?? {} })));
    })),
    http.post(`${API}/token-sheets/submit`, member(({ db, me }) => HttpResponse.json(submitSheet(db, me, now())))),
    // A controller asks everyone to submit before the close.
    http.post(`${API}/token-sheets/remind`, member(async ({ db, me, request }) => {
      if (me.role !== "controller") return forbidden();
      const period = (await readJson<{ period?: string }>(request))?.period ?? "";
      if (!/^\d{4}-(0[1-9]|1[0-2])$/.test(period)) return fail(400, "bad_request", `${JSON.stringify(period)} is not a period: write it as 2026-09`);
      if (isClosed(db, period)) return fail(409, "conflict", `${period} is closed; a late token sheet is posted to it as an adjustment`);
      const state = sheetState(db);
      state.requests = [...state.requests.filter((r) => r.period !== period), { period, asked_by: me.id, asked_at: now() }];
      const people = new Set(db.people.filter((p) => p.status !== "removed" && lateSheets(db, p, period).length).map((p) => p.id));
      for (const entry of db.entries) if (entry.status === "draft" && entry.agent_manager && entry.started_at.slice(0, 7) === period) people.add(entry.agent_manager);
      const reminded: Reminded = { reminded: people.size };
      return HttpResponse.json(reminded);
    })),

    // Approval and reject (docs: Approval).
    http.get(`${API}/token-entries/approval`, member(({ db, me, url }) => {
      if (!atLeast(me, "people_manager")) return forbidden();
      const filter = parseFilter(url.searchParams.get("filter"));
      if (typeof filter === "string") return fail(400, "bad_request", filter);
      const entries = views(db, decidable(db, me, ["submitted"], url.searchParams.get("period"), filter));
      return url.searchParams.get("format") === "csv" ? csvFile(entriesCsv(entries), "approval.csv") : HttpResponse.json(entries);
    })),
    http.post(`${API}/token-entries/approve`, member(async ({ db, me, request }) => {
      if (!atLeast(me, "people_manager")) return forbidden();
      if (!db.organization.approval) return fail(409, "conflict", "approval is off for this organization: submitted token entries count as they are");
      const input = (await readJson<ApproveInput>(request)) ?? {};
      const filter = parseFilter(input.filter);
      if (typeof filter === "string") return fail(400, "bad_request", filter);
      const allowed = new Set(decidable(db, me, ["submitted"], input.period ?? null, filter).map((line) => line.entry.id));
      const asked = input.token_entry_ids ? new Set(input.token_entry_ids) : null;
      const ids = asked ? [...asked].filter((id) => allowed.has(id)) : [...allowed];
      let approved = 0;
      for (const entry of db.entries) {
        if (ids.includes(entry.id) && entry.status === "submitted") {
          entry.status = "approved";
          approved += 1;
        }
      }
      const decided: Decided = { approved, skipped: (asked ? asked.size - ids.length : 0) + ids.length - approved };
      return HttpResponse.json(decided);
    })),
    http.post(`${API}/token-entries/:id/reject`, member(async ({ db, me, params, request }) => {
      if (!atLeast(me, "people_manager")) return forbidden();
      const reason = ((await readJson<{ reason?: string }>(request))?.reason ?? "").trim();
      if (!reason) return fail(400, "bad_request", "say why the token entry goes back");
      const entry = db.entries.find((e) => e.id === params.id);
      if (!entry) return fail(404, "not_found", "not found");
      if (entry.status === "locked") return fail(409, "conflict", "the token entry is in a closed period; a controller can post an adjustment instead");
      if (!decidable(db, me, ["submitted", "approved"], null, {}).some((line) => line.entry.id === entry.id)) return fail(404, "not_found", "not found");
      entry.status = "rejected";
      entry.reject_reason = reason;
      const [view] = views(db, linesOf(db, entry));
      return view ? HttpResponse.json(view) : fail(404, "not_found", "not found");
    })),
  ];
}
