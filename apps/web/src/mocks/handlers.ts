// Mock answers for every call in `src/api/endpoints.ts`, used in development,
// unit tests, Playwright journeys and screenshots until the service has the
// endpoint. State lives in sessionStorage so a reload keeps it.

import { http, HttpResponse, type HttpResponseResolver } from "msw";
import type {
  CadenceEvery,
  CliAuthorization,
  CostSource,
  NewBill,
  NewTokenEntry,
  Person,
  Role,
  RuleSignal,
  Tags,
  TokenEntryStatus,
} from "@/api/contract";
import { parseCsv } from "@/lib/csv";
import { filterFromText } from "@/lib/format";
import { createDb, inheritTags, TODAY, type Db, type EntryRecord } from "./seed";
import {
  budgetVsActual,
  canSee,
  closeCheck,
  costReport,
  costSheet,
  costSheetCsv,
  cur,
  forecast,
  harnessReport,
  lateSheets,
  m,
  matches,
  NANOS,
  onTickets,
  outliers,
  reconciliation,
  splitsOf,
  toAdjustment,
  toBill,
  toEntry,
  toTicket,
  tokenSheet,
  viewer,
  visibleBudgets,
} from "./store";

const KEY = "tc-mock-db";
let memory: Db | null = null;

function load(): Db {
  if (memory) return memory;
  try {
    const saved = sessionStorage.getItem(KEY);
    if (saved) {
      memory = JSON.parse(saved) as Db;
      return memory;
    }
  } catch {
    // No storage in this context; start fresh.
  }
  memory = createDb();
  return memory;
}

function save(db: Db): void {
  memory = db;
  try {
    sessionStorage.setItem(KEY, JSON.stringify(db));
  } catch {
    // Keep it in memory only.
  }
}

export function resetMockDb(): void {
  memory = createDb();
  save(memory);
}

const API = "/api/v1";

function error(status: number, code: string, message: string) {
  return HttpResponse.json({ error: code, message }, { status });
}

function nextId(db: Db, prefix: string): string {
  db.nextId += 1;
  return `${prefix}_${db.nextId}`;
}

function toNanos(money: { nanos: number | string } | null | undefined): number {
  return money ? Number(money.nanos) : 0;
}

type Ctx = { db: Db; me: Person; url: URL; params: Record<string, string>; request: Request };

/** A handler that needs a signed-in member. `roles` limits it further. */
function member(handler: (ctx: Ctx) => Response | Promise<Response>, roles?: Role[]): HttpResponseResolver {
  return async ({ request, params }) => {
    const db = load();
    const me = viewer(db);
    if (!me) return error(401, "signed_out", "Sign in to continue.");
    if (roles && !roles.includes(me.role) && me.role !== "controller") return error(403, "forbidden", "Your role cannot do this.");
    const result = await handler({ db, me, url: new URL(request.url), params: params as Record<string, string>, request });
    save(db);
    return result;
  };
}

const PM: Role[] = ["people_manager"];
const CONTROLLER: Role[] = ["controller"];

function cliAuthorization(db: Db, me: Person, code: string, status: CliAuthorization["status"]): Response {
  if (!/^[A-Z]{4}-[A-Z]{4}$/.test(code)) return error(404, "not_found", "No computer is waiting with this code. Run tc connect again.");
  const device = `${(me.name.split(" ")[0] ?? "my").toLowerCase()}-laptop`;
  return HttpResponse.json({ user_code: code, device, organization: db.organization.name, status, expires_at: `${TODAY}T23:59:00Z` } satisfies CliAuthorization);
}

function tagNameRows(db: Db) {
  return db.tagNames.map((name) => {
    const counts = new Map<string, number>();
    for (const entry of db.entries) {
      if (db.periods.find((p) => p.id === entry.period)?.status === "closed") continue;
      for (const split of entry.splits) {
        const value = split.tags[name];
        if (value) counts.set(value, (counts.get(value) ?? 0) + 1);
      }
    }
    return {
      name,
      built_in: ["person", "ticket", "project"].includes(name),
      values: [...counts.entries()].sort((a, b) => b[1] - a[1]).map(([value, entries]) => ({ value, entries })),
    };
  });
}

function isClosed(db: Db, period: string): boolean {
  return db.periods.find((p) => p.id === period)?.status === "closed";
}

function retag(db: Db, entry: EntryRecord, tags: Tags): Tags {
  // Setting the ticket fills project and the tags the project passes on.
  const merged: Tags = { ...tags };
  const ticket = merged.ticket ? db.tickets.find((t) => t.key === merged.ticket) : undefined;
  if (ticket) {
    const inherited = inheritTags(entry.agent_manager, ticket.key);
    return { ...inherited, ...Object.fromEntries(Object.entries(merged).filter(([k]) => !["ticket", "project", "client"].includes(k))) };
  }
  if (merged.project && !merged.ticket) {
    const project = db.projects.find((p) => p.key === merged.project);
    if (project) Object.assign(merged, project.tags);
  }
  return Object.fromEntries(Object.entries(merged).filter(([, value]) => value));
}

function csvEntries(db: Db, text: string) {
  const [header = [], ...rows] = parseCsv(text);
  const col = (name: string) => header.findIndex((h) => h.trim() === name);
  const problems: { row: number; message: string }[] = [];
  const parsed: NewTokenEntry[] = [];
  rows.forEach((cells, index) => {
    const get = (name: string) => (col(name) === -1 ? "" : (cells[col(name)] ?? "").trim());
    const row = index + 1;
    for (const required of ["agent_harness", "model", "started_at", "ended_at"]) {
      if (!get(required)) problems.push({ row, message: `${required} is missing.` });
    }
    const tags: Tags = {};
    if (get("ticket")) tags.ticket = get("ticket");
    if (get("project")) tags.project = get("project");
    for (const name of db.tagNames) if (!["person", "ticket", "project"].includes(name) && get(name)) tags[name] = get(name);
    const cost = get("reported_cost");
    parsed.push({
      agent_harness: get("agent_harness"),
      agent_name: get("agent_name") || null,
      model: get("model"),
      started_at: get("started_at"),
      ended_at: get("ended_at"),
      tokens: { input: Number(get("input")) || 0, output: Number(get("output")) || 0, cache_write: Number(get("cache_write")) || 0, cache_read: Number(get("cache_read")) || 0 },
      reported_cost: cost ? m(Math.round(Number(cost) * 100) * 10_000_000, cur(db)) : null,
      summary: get("summary"),
      tags,
    });
  });
  return { rows: parsed, problems };
}

function addEntry(db: Db, me: Person, body: NewTokenEntry): EntryRecord {
  const ticket = body.tags.ticket ?? body.tags.project ?? "";
  const extra = Object.fromEntries(Object.entries(body.tags).filter(([k]) => !["ticket", "project"].includes(k)));
  const record: EntryRecord = {
    id: nextId(db, "te"),
    period: body.started_at.slice(0, 7),
    agent_harness: body.agent_harness,
    agent_name: body.agent_name,
    model: body.model,
    effort: null,
    started_at: new Date(body.started_at).toISOString(),
    ended_at: new Date(body.ended_at).toISOString(),
    tokens: body.tokens,
    list: body.reported_cost ? toNanos(body.reported_cost) : Math.round((body.tokens.input * 3 + body.tokens.output * 15 + body.tokens.cache_write * 3.75 + body.tokens.cache_read * 0.3) * 1000),
    billed: null,
    summary: body.summary,
    agent_manager: me.id,
    automatic: false,
    status: "draft",
    reject_reason: null,
    placed_by: "by hand",
    candidates: [],
    splits: [{ share: 10000, tags: inheritTags(me.id, ticket, extra) }],
    late: false,
    provider: body.agent_harness === "Codex" ? "openai" : "anthropic",
  };
  db.entries.push(record);
  return record;
}

function billCsv(db: Db, text: string) {
  const [header = [], ...rows] = parseCsv(text);
  const col = (name: string) => header.findIndex((h) => h.trim().toLowerCase() === name);
  const problems: { row: number; message: string }[] = [];
  const bills: NewBill[] = rows.map((cells, index) => {
    const get = (name: string) => (col(name) === -1 ? "" : (cells[col(name)] ?? "").trim());
    const amount = (name: string) => m(Math.round(Number(get(name) || "0") * 100) * 10_000_000, get("currency") || cur(db));
    if (!/^\d{4}-\d{2}$/.test(get("period"))) problems.push({ row: index + 1, message: "period must look like 2026-09." });
    if (!get("net")) problems.push({ row: index + 1, message: "net is missing." });
    return { provider: "other", label: get("provider") || "Other", period: get("period"), gross: amount("gross"), discount: amount("discount"), credit: amount("credit"), net: amount("net") };
  });
  return { rows: bills, problems };
}

export const handlers = [
  // Sign-in, sign-up and the session
  http.post(`${API}/sign-in`, async ({ request }) => {
    const db = load();
    const { email, next } = (await request.json()) as { email: string; next?: string };
    const person = db.people.find((p) => p.email.toLowerCase() === email.trim().toLowerCase());
    // The service answers the same either way, so nobody can probe for members.
    db.pendingLink = person ? { token: `link_${person.id}`, email: person.email, ...(next ? { next } : {}) } : null;
    save(db);
    return new HttpResponse(null, { status: 204 });
  }),
  http.post(`${API}/sign-up`, async ({ request }) => {
    const body = (await request.json()) as { organization: string; name: string; email: string; manages: "agents" | "people"; cost_source: CostSource; reporting_currency: string };
    const db = createDb();
    // A new organization: one person who holds every role, no data yet.
    const founder: Person = { id: "founder", name: body.name, email: body.email, role: "controller", tags: {}, filters: [], status: "active" };
    Object.assign(db, {
      organization: { ...db.organization, id: "org_new", name: body.organization, plan: "solo", cost_source: body.cost_source, reporting_currency: body.reporting_currency, approval: false, sign_in_domains: [], members: 1, member_limit: 1 },
      people: [founder],
      projects: [],
      tickets: [],
      entries: [],
      bills: [],
      connections: [],
      budgets: [],
      reports: [],
      adjustments: [],
      tokens: [],
      ticketSystems: [],
      tagNames: ["person", "ticket", "project"],
      rules: db.rules.filter((r) => r.built_in),
      periods: [{ id: TODAY.slice(0, 7), status: "open", closed_at: null, closed_by: null, tag_map: null }],
      pendingLink: { token: "link_founder", email: body.email },
      session: null,
    } satisfies Partial<Db>);
    save(db);
    return new HttpResponse(null, { status: 204 });
  }),
  http.post(`${API}/sign-in/verify`, async ({ request }) => {
    const db = load();
    const { token } = (await request.json()) as { token: string };
    const person = db.people.find((p) => `link_${p.id}` === token);
    if (!person) return error(400, "link_expired", "This sign-in link has expired. Ask for a new one.");
    db.session = person.id;
    db.pendingLink = null;
    if (person.status === "invited") person.status = "active";
    save(db);
    return HttpResponse.json({ person, organization: db.organization });
  }),
  http.post(`${API}/sign-out`, () => {
    const db = load();
    db.session = null;
    save(db);
    return new HttpResponse(null, { status: 204 });
  }),
  // Connecting a computer: in the mocks every well-formed code is waiting.
  http.get(`${API}/cli/authorizations/by-code/:code`, member(({ db, me, params }) => cliAuthorization(db, me, String(params.code), "pending"))),
  http.post(`${API}/cli/authorizations/by-code/:code/confirm`, member(({ db, me, params }) => cliAuthorization(db, me, String(params.code), "confirmed"))),
  http.post(`${API}/cli/authorizations/by-code/:code/decline`, member(({ db, me, params }) => cliAuthorization(db, me, String(params.code), "declined"))),
  http.get(`${API}/__mock/last-link`, () => HttpResponse.json(load().pendingLink)),
  http.post(`${API}/__mock/reset`, () => {
    resetMockDb();
    return new HttpResponse(null, { status: 204 });
  }),
  http.get(`${API}/me`, member(({ db, me }) => HttpResponse.json({ person: me, organization: { ...db.organization, members: db.people.filter((p) => p.status !== "left").length } }))),

  // Organization settings
  http.patch(`${API}/organization`, member(async ({ db, request }) => {
    Object.assign(db.organization, (await request.json()) as object);
    return HttpResponse.json(db.organization);
  }, CONTROLLER)),
  http.get(`${API}/organization/cadence`, member(({ db }) => HttpResponse.json(db.cadence))),
  http.put(`${API}/organization/cadence`, member(async ({ db, request }) => {
    db.cadence = (await request.json()) as { every: CadenceEvery; days: number | null } | null;
    return HttpResponse.json(db.cadence);
  }, PM)),

  // People
  http.get(`${API}/members`, member(({ db }) => HttpResponse.json(db.people.filter((p) => p.status !== "left")))),
  http.post(`${API}/members`, member(async ({ db, request }) => {
    const body = (await request.json()) as { name: string; email: string; role: Role; tags: Tags };
    if (db.organization.plan === "solo") return error(402, "plan_limit", "Solo is for one person. Choose the Team plan to invite people.");
    if (db.organization.member_limit !== null && db.people.length >= db.organization.member_limit) return error(402, "plan_limit", "The Team plan stops at 150 members.");
    if (db.people.some((p) => p.email.toLowerCase() === body.email.toLowerCase())) return error(409, "exists", "Someone with this email is already a member.");
    const person: Person = { id: nextId(db, "p"), name: body.name, email: body.email, role: body.role, tags: body.tags, filters: [], status: "invited" };
    db.people.push(person);
    return HttpResponse.json(person, { status: 201 });
  }, CONTROLLER)),
  http.patch(`${API}/members/:id`, member(async ({ db, params, request }) => {
    const person = db.people.find((p) => p.id === params.id);
    if (!person) return error(404, "not_found", "No such member.");
    Object.assign(person, (await request.json()) as Partial<Person>);
    return HttpResponse.json(person);
  }, CONTROLLER)),
  http.delete(`${API}/members/:id`, member(({ db, params }) => {
    const person = db.people.find((p) => p.id === params.id);
    if (person) person.status = "left";
    return new HttpResponse(null, { status: 204 });
  }, CONTROLLER)),

  // Tags and the tag map
  http.get(`${API}/tag-names`, member(({ db }) => HttpResponse.json(tagNameRows(db)))),
  http.post(`${API}/tag-names`, member(async ({ db, request }) => {
    const { name } = (await request.json()) as { name: string };
    const clean = name.trim().toLowerCase();
    if (!/^[a-z][a-z0-9_-]*$/.test(clean)) return error(400, "invalid", "A tag name is lowercase letters, digits, - and _.");
    if (db.tagNames.includes(clean)) return error(409, "exists", "This tag name is already on the list.");
    db.tagNames.push(clean);
    return HttpResponse.json(tagNameRows(db).find((t) => t.name === clean), { status: 201 });
  }, CONTROLLER)),
  http.delete(`${API}/tag-names/:name`, member(({ db, params }) => {
    if (["person", "ticket", "project"].includes(params.name ?? "")) return error(400, "built_in", "Built-in tags cannot be removed.");
    db.tagNames = db.tagNames.filter((n) => n !== params.name);
    return new HttpResponse(null, { status: 204 });
  }, CONTROLLER)),
  http.post(`${API}/tag-names/:name/merge`, member(async ({ db, params, request }) => {
    const { from, into } = (await request.json()) as { from: string[]; into: string };
    const name = params.name ?? "";
    for (const entry of db.entries) {
      if (isClosed(db, entry.period)) continue;
      for (const split of entry.splits) {
        const value = split.tags[name];
        if (value && from.includes(value)) split.tags[name] = into;
      }
    }
    return HttpResponse.json(tagNameRows(db).find((t) => t.name === name));
  }, CONTROLLER)),
  http.get(`${API}/tag-map`, member(({ db }) => HttpResponse.json(db.tagMap))),
  http.put(`${API}/tag-map`, member(async ({ db, request }) => {
    db.tagMap = (await request.json()) as Db["tagMap"];
    return HttpResponse.json(db.tagMap);
  }, CONTROLLER)),

  // Projects and tickets
  http.get(`${API}/projects`, member(({ db }) => HttpResponse.json(db.projects))),
  http.post(`${API}/projects`, member(async ({ db, request }) => {
    const body = (await request.json()) as Db["projects"][number];
    if (db.projects.some((p) => p.key === body.key)) return error(409, "exists", "A project with this key exists.");
    const project = { ...body, id: nextId(db, "p") };
    db.projects.push(project);
    return HttpResponse.json(project, { status: 201 });
  }, CONTROLLER)),
  http.patch(`${API}/projects/:key`, member(async ({ db, params, request }) => {
    const project = db.projects.find((p) => p.key === params.key);
    if (!project) return error(404, "not_found", "No such project.");
    Object.assign(project, (await request.json()) as object);
    return HttpResponse.json(project);
  }, PM)),
  http.get(`${API}/tickets`, member(({ db, url }) => {
    const project = url.searchParams.get("project");
    return HttpResponse.json(db.tickets.filter((t) => !project || t.project === project).map((t) => toTicket(db, t)));
  })),
  http.post(`${API}/tickets`, member(async ({ db, request }) => {
    const body = (await request.json()) as Db["tickets"][number];
    if (db.tickets.some((t) => t.key === body.key)) return error(409, "exists", "A ticket with this key exists.");
    const ticket = { ...body, source: "manual" as const, open: true };
    db.tickets.push(ticket);
    return HttpResponse.json(toTicket(db, ticket), { status: 201 });
  }, PM)),

  // Token entries
  http.get(`${API}/token-entries`, member(({ db, me, url }) => {
    const period = url.searchParams.get("period") ?? TODAY.slice(0, 7);
    const status = url.searchParams.get("status") as TokenEntryStatus | null;
    const person = url.searchParams.get("person");
    const filter = filterFromText(url.searchParams.get("filter") ?? "");
    const rows = db.entries
      .filter((e) => e.period === period && canSee(me, e))
      .filter((e) => !status || e.status === status)
      .filter((e) => !person || e.agent_manager === person)
      .filter((e) => e.splits.some((s) => matches(s.tags, filter)))
      .sort((a, b) => b.started_at.localeCompare(a.started_at));
    return HttpResponse.json(rows.map((e) => toEntry(db, e)));
  })),
  http.post(`${API}/token-entries`, member(async ({ db, me, request }) => {
    const record = addEntry(db, me, (await request.json()) as NewTokenEntry);
    return HttpResponse.json(toEntry(db, record), { status: 201 });
  })),
  http.post(`${API}/token-entries/import/preview`, member(async ({ db, request }) => HttpResponse.json(csvEntries(db, await request.text())))),
  http.post(`${API}/token-entries/import`, member(async ({ db, me, request }) => {
    const { rows, problems } = csvEntries(db, await request.text());
    if (problems.length) return error(400, "invalid_csv", "Fix the rows with problems first.");
    for (const row of rows) addEntry(db, me, row);
    return HttpResponse.json({ entries: rows.length });
  })),
  http.patch(`${API}/token-entries/:id/tags`, member(async ({ db, me, params, request }) => {
    const entry = db.entries.find((e) => e.id === params.id);
    if (!entry || !canSee(me, entry)) return error(404, "not_found", "No such token entry.");
    if (entry.status === "locked") return error(409, "locked", "Its period is closed. Post an adjustment instead.");
    const { tags } = (await request.json()) as { tags: Tags };
    entry.splits = [{ share: 10000, tags: retag(db, entry, tags) }];
    entry.placed_by = "by hand";
    entry.candidates = [];
    // A fixed rejected entry goes back on the token sheet.
    if (entry.status === "rejected" && entry.agent_manager === me.id) {
      entry.status = "draft";
      entry.reject_reason = null;
    }
    return HttpResponse.json(toEntry(db, entry));
  })),
  http.put(`${API}/token-entries/:id/splits`, member(async ({ db, me, params, request }) => {
    const entry = db.entries.find((e) => e.id === params.id);
    if (!entry || !canSee(me, entry)) return error(404, "not_found", "No such token entry.");
    const { splits } = (await request.json()) as { splits: { share: number; tags: Tags }[] };
    if (splits.reduce((s, x) => s + x.share, 0) !== 10000) return error(400, "invalid", "The shares must add up to 100%.");
    entry.splits = splits.map((s) => ({ share: s.share, tags: retag(db, entry, s.tags) }));
    entry.placed_by = "split by hand";
    entry.candidates = [];
    return HttpResponse.json(toEntry(db, entry));
  })),
  http.post(`${API}/token-entries/approve`, member(async ({ db, me, request }) => {
    const { ids } = (await request.json()) as { ids: string[] };
    let approved = 0;
    for (const entry of db.entries) {
      if (ids.includes(entry.id) && entry.status === "submitted" && canSee(me, entry)) {
        entry.status = "approved";
        approved++;
      }
    }
    return HttpResponse.json({ approved });
  }, PM)),
  http.post(`${API}/token-entries/:id/reject`, member(async ({ db, me, params, request }) => {
    const entry = db.entries.find((e) => e.id === params.id);
    if (!entry || !canSee(me, entry)) return error(404, "not_found", "No such token entry.");
    const { reason } = (await request.json()) as { reason: string };
    if (!reason.trim()) return error(400, "invalid", "Give a reason.");
    entry.status = "rejected";
    entry.reject_reason = reason.trim();
    return HttpResponse.json(toEntry(db, entry));
  }, PM)),
  http.post(`${API}/token-entries/:id/take-over`, member(({ db, me, params }) => {
    const entry = db.entries.find((e) => e.id === params.id);
    if (!entry) return error(404, "not_found", "No such token entry.");
    entry.agent_manager = me.id;
    for (const split of entry.splits) split.tags = { ...split.tags, person: me.id, ...me.tags };
    return HttpResponse.json(toEntry(db, entry));
  }, PM)),
  http.get(`${API}/token-sheets/current`, member(({ db, me }) => HttpResponse.json(tokenSheet(db, me)))),
  http.post(`${API}/token-sheets/submit`, member(({ db, me }) => {
    const drafts = db.entries.filter((e) => e.agent_manager === me.id && e.status === "draft");
    const rows = drafts.flatMap(splitsOf);
    const total = rows.reduce((s, r) => s + (r.nanos ?? 0), 0);
    for (const entry of drafts) {
      if (isClosed(db, entry.period)) {
        // After the close a sheet becomes an adjustment to that month, marked late.
        entry.status = "locked";
        entry.late = true;
        const tags = entry.splits[0]?.tags ?? {};
        const rung = db.tagMap.ladder.find((r) => tags[r]) ?? "organization";
        db.adjustments.push({ id: nextId(db, "adj"), period: entry.period, rung, value: tags[rung] ?? "organization", amount: splitsOf(entry).reduce((s, r) => s + (r.nanos ?? 0), 0), reason: `Late token sheet from ${me.name}`, posted_by: me.id, posted_at: `${TODAY}T12:00:00Z`, late_sheet: true });
      } else entry.status = "submitted";
    }
    return HttpResponse.json({ entries: drafts.length, total: m(total, cur(db)), on_tickets: onTickets(rows) });
  })),
  http.post(`${API}/token-sheets/remind`, member(async ({ db, request }) => {
    const { period } = (await request.json()) as { period?: string };
    return HttpResponse.json({ reminded: new Set(lateSheets(db, period).map((s) => s.person)).size });
  }, CONTROLLER)),

  // Bills
  http.get(`${API}/bills`, member(({ db, url }) => {
    const period = url.searchParams.get("period");
    return HttpResponse.json(db.bills.filter((b) => !period || b.period === period).map((b) => toBill(db, b)));
  }, PM)),
  http.post(`${API}/bills`, member(async ({ db, request }) => {
    const body = (await request.json()) as NewBill;
    const bill = { id: nextId(db, "bill"), provider: body.provider, label: body.label, period: body.period, gross: toNanos(body.gross), discount: toNanos(body.discount), credit: toNanos(body.credit), net: toNanos(body.net), source: "manual" as const, revision: 1, received_at: `${TODAY}T12:00:00Z`, revisions: [], final: true };
    db.bills.push(bill);
    return HttpResponse.json(toBill(db, bill), { status: 201 });
  }, CONTROLLER)),
  http.post(`${API}/bills/import/preview`, member(async ({ db, request }) => HttpResponse.json(billCsv(db, await request.text())), CONTROLLER)),
  http.post(`${API}/bills/import`, member(async ({ db, request }) => {
    const { rows, problems } = billCsv(db, await request.text());
    if (problems.length) return error(400, "invalid_csv", "Fix the rows with problems first.");
    const added = rows.map((body) => {
      const bill = { id: nextId(db, "bill"), provider: body.provider, label: body.label, period: body.period, gross: toNanos(body.gross), discount: toNanos(body.discount), credit: toNanos(body.credit), net: toNanos(body.net), source: "csv" as const, revision: 1, received_at: `${TODAY}T12:00:00Z`, revisions: [], final: true };
      db.bills.push(bill);
      return toBill(db, bill);
    });
    return HttpResponse.json(added);
  }, CONTROLLER)),
  http.get(`${API}/bill-connections`, member(({ db }) => HttpResponse.json(db.connections), PM)),
  http.post(`${API}/bill-connections`, member(async ({ db, request }) => {
    const { provider, key } = (await request.json()) as { provider: Db["connections"][number]["provider"]; key: string };
    if (key.trim().length < 12) return error(400, "invalid_key", "This does not look like an admin key.");
    const connection = { id: nextId(db, "bc"), provider, status: "ok" as const, last_pull_at: `${TODAY}T12:00:00Z`, message: null };
    db.connections.push(connection);
    return HttpResponse.json(connection, { status: 201 });
  }, CONTROLLER)),
  http.post(`${API}/bill-connections/:id/pull`, member(({ db, params }) => {
    const connection = db.connections.find((c) => c.id === params.id);
    if (!connection) return error(404, "not_found", "No such connection.");
    connection.last_pull_at = `${TODAY}T17:00:00Z`;
    return HttpResponse.json(connection);
  }, CONTROLLER)),
  http.get(`${API}/reconciliation`, member(({ db, url }) => HttpResponse.json(reconciliation(db, url.searchParams.get("period") ?? TODAY.slice(0, 7))), PM)),

  // Periods and the close
  http.get(`${API}/periods`, member(({ db }) => HttpResponse.json([...db.periods].reverse().map((p) => ({
    id: p.id,
    status: p.status,
    closed_at: p.closed_at,
    closed_by: p.closed_by,
    total: m(db.entries.filter((e) => e.period === p.id && (e.status === "submitted" || e.status === "approved" || e.status === "locked")).reduce((s, e) => s + (e.list ?? 0), 0), cur(db)),
  }))))),
  http.get(`${API}/periods/:id/close-check`, member(({ db, params }) => HttpResponse.json(closeCheck(db, params.id ?? "")), CONTROLLER)),
  http.post(`${API}/periods/:id/close`, member(({ db, me, params }) => {
    const id = params.id ?? "";
    const check = closeCheck(db, id);
    if (!check.can_close) return error(409, "cannot_close", check.blocked_by.join(" "));
    const period = db.periods.find((p) => p.id === id);
    if (!period) return error(404, "not_found", "No such period.");
    Object.assign(period, { status: "closed", closed_at: `${TODAY}T17:30:00Z`, closed_by: me.id, tag_map: { ...db.tagMap } });
    for (const entry of db.entries) if (entry.period === id && (entry.status === "approved" || entry.status === "submitted")) entry.status = "locked";
    return HttpResponse.json({ ...period, total: m(0, cur(db)) });
  }, CONTROLLER)),
  http.get(`${API}/periods/:id/cost-sheet`, member(({ db, params }) => {
    const sheet = costSheet(db, params.id ?? "");
    return sheet ? HttpResponse.json(sheet) : error(404, "not_closed", "This period is not closed yet.");
  }, CONTROLLER)),
  http.post(`${API}/periods/:id/cost-sheet/link`, member(({ params, url }) => HttpResponse.json({ url: `${url.origin}/shared/cost-sheet/cs_${params.id ?? ""}` }), CONTROLLER)),
  http.get(`${API}/shared/cost-sheets/:token`, ({ params }) => {
    // A link finance opens without signing in.
    const db = load();
    const sheet = costSheet(db, String(params.token).replace(/^cs_/, ""));
    const closer = db.people.find((p) => p.id === sheet?.closed_by)?.name ?? sheet?.closed_by ?? "";
    return sheet ? HttpResponse.json({ ...sheet, closed_by: closer, organization: db.organization.name }) : error(404, "not_found", "This link does not work any more.");
  }),
  http.get(`${API}/periods/:id/cost-sheet.csv`, member(({ db, params, url }) => {
    const columns = (url.searchParams.get("columns") ?? "").split(",").filter(Boolean);
    return new HttpResponse(costSheetCsv(db, params.id ?? "", columns), { headers: { "content-type": "text/csv; charset=utf-8" } });
  }, CONTROLLER)),
  http.post(`${API}/periods/:id/adjustments`, member(async ({ db, me, params, request }) => {
    if (!isClosed(db, params.id ?? "")) return error(409, "open", "Adjustments go on closed periods. Change the token entries of an open period instead.");
    const body = (await request.json()) as { rung: string; value: string; amount: { nanos: number | string }; reason: string };
    if (!body.reason.trim()) return error(400, "invalid", "Give a reason.");
    const adjustment = { id: nextId(db, "adj"), period: params.id ?? "", rung: body.rung, value: body.value, amount: toNanos(body.amount), reason: body.reason, posted_by: me.id, posted_at: `${TODAY}T12:00:00Z`, late_sheet: false };
    db.adjustments.push(adjustment);
    return HttpResponse.json(toAdjustment(db, adjustment), { status: 201 });
  }, CONTROLLER)),

  // Token budgets and forecasts
  http.get(`${API}/budgets`, member(({ db, me, url }) => HttpResponse.json(visibleBudgets(db, me, url.searchParams.get("period") ?? TODAY.slice(0, 7))))),
  http.post(`${API}/budgets`, member(async ({ db, me, request }) => {
    const body = (await request.json()) as { filter: Tags; period: string; amount: { nanos: number | string }; from_forecast: boolean };
    if (me.role === "people_manager" && !me.filters.some((f) => Object.entries(f).every(([k, v]) => body.filter[k] === v))) {
      return error(403, "forbidden", "Token budgets go within the filters you are assigned.");
    }
    const budget = { id: nextId(db, "tb"), filter: body.filter, period: body.period, amount: toNanos(body.amount), from_forecast: body.from_forecast, created_by: me.id };
    db.budgets.push(budget);
    return HttpResponse.json(visibleBudgets(db, me, body.period).find((b) => b.id === budget.id), { status: 201 });
  }, PM)),
  http.delete(`${API}/budgets/:id`, member(({ db, params }) => {
    db.budgets = db.budgets.filter((b) => b.id !== params.id);
    return new HttpResponse(null, { status: 204 });
  }, PM)),
  http.get(`${API}/forecasts/:ticket`, member(({ db, params }) => {
    if (!db.organization.forecasts) return error(404, "off", "Token forecasts are switched off.");
    const result = forecast(db, params.ticket ?? "");
    return result ? HttpResponse.json(result) : error(404, "no_history", "No similar past work yet. Close a few periods first.");
  }, PM)),

  // Reports
  http.get(`${API}/reports/saved`, member(({ db }) => HttpResponse.json(db.reports))),
  http.post(`${API}/reports/saved`, member(async ({ db, request }) => {
    const report = { ...((await request.json()) as { name: string; filter: Tags; group_by: string }), id: nextId(db, "rp") };
    db.reports.push(report);
    return HttpResponse.json(report, { status: 201 });
  })),
  http.delete(`${API}/reports/saved/:id`, member(({ db, params }) => {
    db.reports = db.reports.filter((r) => r.id !== params.id);
    return new HttpResponse(null, { status: 204 });
  })),
  http.get(`${API}/reports/cost`, member(({ db, me, url }) => HttpResponse.json(costReport(db, me, {
    period: url.searchParams.get("period") ?? TODAY.slice(0, 7),
    group_by: url.searchParams.get("by") ?? "project",
    filter: filterFromText(url.searchParams.get("filter") ?? ""),
    currency: url.searchParams.get("currency") ?? undefined,
    approved_only: url.searchParams.get("approved_only") === "true",
  })))),
  http.get(`${API}/reports/budget-vs-actual`, member(({ db, me, url }) => HttpResponse.json(budgetVsActual(db, me, url.searchParams.get("period") ?? TODAY.slice(0, 7))))),
  http.get(`${API}/reports/agent-harness`, member(({ db, me, url }) => HttpResponse.json(harnessReport(db, me, url.searchParams.get("period") ?? TODAY.slice(0, 7), (url.searchParams.get("by") ?? "agent_harness") as "agent_harness")))),
  http.get(`${API}/reports/outliers`, member(({ db, me, url }) => HttpResponse.json(outliers(db, me, url.searchParams.get("period") ?? TODAY.slice(0, 7))))),

  // Rules
  http.get(`${API}/rules`, member(({ db, me }) => HttpResponse.json(db.rules.filter((r) => r.scope === "organization" || r.id.endsWith(`:${me.id}`))))),
  http.post(`${API}/rules`, member(async ({ db, me, request }) => {
    const body = (await request.json()) as { scope: "organization" | "personal"; name: string; signal: RuleSignal; match: "ticket_key" | "starts_with" | "contains" | "equals"; pattern: string; sets: Tags };
    if (body.scope === "organization" && me.role !== "controller") return error(403, "forbidden", "Only a controller sets organization rules.");
    const rule = { ...body, id: body.scope === "personal" ? `${nextId(db, "r")}:${me.id}` : nextId(db, "r"), built_in: false };
    db.rules.push(rule);
    return HttpResponse.json(rule, { status: 201 });
  })),
  http.delete(`${API}/rules/:id`, member(({ db, params }) => {
    db.rules = db.rules.filter((r) => r.id !== params.id || r.built_in);
    return new HttpResponse(null, { status: 204 });
  })),
  http.put(`${API}/rules/order`, member(async ({ db, request }) => {
    const { ids } = (await request.json()) as { ids: string[] };
    db.rules.sort((a, b) => ids.indexOf(a.id) - ids.indexOf(b.id));
    return HttpResponse.json(db.rules);
  })),

  // Settings
  http.get(`${API}/api-tokens`, member(({ db, me }) => HttpResponse.json(db.tokens.filter((t) => me.role === "controller" || t.owner === me.id || (me.role === "people_manager" && t.owner === null))))),
  http.post(`${API}/api-tokens`, member(async ({ db, me, request }) => {
    const { kind, name } = (await request.json()) as { kind: "svc" | "read" | "org"; name: string };
    if (kind === "org" && me.role !== "controller") return error(403, "forbidden", "Only a controller creates the organization token.");
    const secret = `tc_${kind}_${Math.random().toString(36).slice(2, 10)}${Math.random().toString(36).slice(2, 18)}`;
    const token = { id: nextId(db, "tok"), kind, name, prefix: secret.slice(0, kind === "read" ? 12 : 11), owner: me.id, created_at: `${TODAY}T12:00:00Z`, last_used_at: null };
    db.tokens.push(token);
    return HttpResponse.json({ token, secret }, { status: 201 });
  })),
  http.delete(`${API}/api-tokens/:id`, member(({ db, params }) => {
    db.tokens = db.tokens.filter((t) => t.id !== params.id);
    return new HttpResponse(null, { status: 204 });
  })),
  http.post(`${API}/api-tokens/:id/owner`, member(async ({ db, params, request }) => {
    const token = db.tokens.find((t) => t.id === params.id);
    if (!token) return error(404, "not_found", "No such token.");
    const { person } = (await request.json()) as { person: string };
    token.owner = person;
    // Only future token entries follow; past ones keep their agent manager.
    return HttpResponse.json(token);
  }, CONTROLLER)),
  http.get(`${API}/ticket-systems`, member(({ db }) => HttpResponse.json(db.ticketSystems))),
  http.post(`${API}/ticket-systems`, member(async ({ db, request }) => {
    const body = (await request.json()) as { kind: "jira" | "azure_devops" | "github_issues"; site: string; token: string };
    if (!body.site.trim() || !body.token.trim()) return error(400, "invalid", "Give the site and an API token.");
    const system = { id: nextId(db, "ts"), kind: body.kind, site: body.site.trim(), mappings: [], last_sync_at: null, status: "ok" as const };
    db.ticketSystems.push(system);
    return HttpResponse.json(system, { status: 201 });
  }, CONTROLLER)),
  http.post(`${API}/ticket-systems/:id/sync`, member(({ db, params }) => {
    const system = db.ticketSystems.find((s) => s.id === params.id);
    if (!system) return error(404, "not_found", "No such ticket system.");
    system.last_sync_at = `${TODAY}T17:05:00Z`;
    return HttpResponse.json(system);
  }, CONTROLLER)),
  http.get(`${API}/export/raw`, member(({ db }) => {
    const files = { "organization.json": db.organization, "people.json": db.people, "token_entries.json": db.entries, "bills.json": db.bills, "rules.json": db.rules, "tickets.json": db.tickets, "periods.json": db.periods, "tags.json": { names: db.tagNames, map: db.tagMap } };
    return new HttpResponse(JSON.stringify(files, null, 2), { headers: { "content-type": "application/json" } });
  }, CONTROLLER)),
  http.post(`${API}/billing/checkout`, member(() => HttpResponse.json({ url: "https://checkout.stripe.com/c/pay/mock" }), CONTROLLER)),
  http.post(`${API}/billing/portal`, member(() => HttpResponse.json({ url: "https://billing.stripe.com/p/session/mock" }), CONTROLLER)),
];

/** Money helper for tests. */
export function dollars(n: number) {
  return n * NANOS;
}
