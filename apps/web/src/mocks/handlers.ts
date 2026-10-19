// Mock answers for every call in `src/api/endpoints.ts`, used in development,
// unit tests, Playwright journeys and screenshots. They mirror the service's
// handlers in apps/server/src: the same status codes and access rules. State
// lives in sessionStorage so a reload keeps it.

import { http, HttpResponse, type HttpHandler, type HttpResponseResolver } from "msw";
import { addedMocks } from "@/additions/mocks";
import type {
  CostReport,
  DeviationReport,
  ForecastReport,
  Member,
  NewOrganization,
  NewRule,
  Role,
  Rule,
  SignUpRequest,
  SignUpWithLink,
  Tags,
  TicketMapping,
} from "@/api/contract";
import { filterFromText } from "@/lib/format";
import { enterpriseHandlers, mockEdition } from "./enterprise";
import {
  acceptedFor,
  canSee,
  contains,
  costOf,
  days,
  dec,
  filterKey,
  fullTags,
  labelOf,
  lines,
  messageCost,
  parseDec,
  planOf,
  sessionView,
  splitOf,
  submittedOf,
  visibleLines,
  type Line,
  type Window,
} from "./fold";
import { createDb, TODAY, type Db, type MockEstimate, type MockSession, type MockValue } from "./seed";

const KEY = "tc-mock-db-2";
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
  return `${prefix}_${String(db.nextId)}`;
}

const now = () => `${TODAY}T12:00:00Z`;

type Ctx = { db: Db; me: Member; url: URL; params: Record<string, string>; request: Request };

const rank: Record<Role, number> = { agent_manager: 1, people_manager: 2, org_manager: 3 };

/** A handler that needs a signed-in member; `role` is the least role it needs. */
function member(handler: (ctx: Ctx) => Response | Promise<Response>, role?: Role): HttpResponseResolver {
  return async ({ request, params }) => {
    const db = load();
    const me = db.people.find((p) => p.id === db.session);
    if (!me) return error(401, "signed_out", "Sign in to continue.");
    if (role && rank[me.role] < rank[role]) return error(403, "forbidden", "Your grants do not cover this.");
    const result = await handler({ db, me, url: new URL(request.url), params: params as Record<string, string>, request });
    save(db);
    return result;
  };
}

/** What the mock answers of a build's additions get. */
export interface MockKit {
  API: string;
  member: typeof member;
  fail: typeof error;
  now: () => string;
}

/** The mock answers of a build's additions (`src/additions/mocks.ts`). */
export type AddedMocks = (kit: MockKit) => readonly HttpHandler[];

function record(db: Db, kind: string, rec: object, by: string | null, id?: string): string {
  const eventId = id ?? nextId(db, kind.slice(0, 3));
  db.events.push({ id: eventId, kind, record: { kind, ...rec } as unknown as Db["events"][number]["record"], recorded_at: now(), recorded_by: by, supersedes: null });
  return eventId;
}

function organizationOf(db: Db, email: string, sign_up: NewOrganization): Db {
  const name = sign_up.organization_name.trim();
  const founder: Member = {
    id: "f0d0e000-0000-4000-8000-000000000001",
    name: sign_up.name?.trim() ?? "",
    email,
    role: "org_manager",
    attributes: {},
    filters: [],
    status: "invited",
    invited_at: now(),
    joined_at: null,
  };
  return {
    ...db,
    organization: { ...db.organization, id: "0e000000-0000-4000-8000-000000000002", name, slug: name.toLowerCase().replace(/[^a-z0-9]+/g, "-"), reporting_currency: sign_up.reporting_currency?.toUpperCase() ?? "USD", approval: false, cutoff: null, sign_in_domains: [], created_at: now() },
    people: [founder],
    invites: [],
    tagValues: [],
    attributeValues: [],
    personAttributes: [],
    sessions: [],
    splits: [],
    bills: [],
    connections: [],
    estimates: [],
    accepted: {},
    shares: [],
    grants: [],
    cutoff: { cutoff: null, event_id: null, frozen_as_of: null, moved_at: null, moved_by: null },
    submissions: [],
    submitRequests: [],
    saved: [],
    tokens: [],
    events: [],
    pendingLink: { token: `link_${founder.id}`, email, start: sign_up.manages ? `/?first_steps=${sign_up.manages}` : "/" },
    session: null,
  };
}

function valueView(v: MockValue) {
  return {
    id: v.id,
    name: v.name,
    key: v.key,
    title: v.title,
    description: v.description,
    source_id: v.source_id,
    source_status: v.source_status,
    defaults: v.defaults as Record<string, never>,
    recorded_at: v.recorded_at,
  };
}

function windowOf(url: URL): Window | Response {
  const from = url.searchParams.get("from") ?? "2026-03-01";
  const to = url.searchParams.get("to") ?? from;
  if (!/^\d{4}-\d{2}-\d{2}$/.test(from) || !/^\d{4}-\d{2}-\d{2}$/.test(to)) return error(400, "bad_request", "from and to are days, YYYY-MM-DD.");
  if (to < from) return error(400, "bad_request", "to is before from.");
  return { from, to };
}

/** The value a line has for a grouping. */
function keyOf(line: Line, by: string): string | null {
  switch (by) {
    case "label":
      return labelOf(line.tags);
    case "agent_harness":
      return line.session.agent_harness;
    case "agent_name":
      return line.session.agent_name;
    case "model":
      return `${line.message.provider}/${line.message.model}`;
    case "day":
      return line.day;
    default:
      return line.tags[by] ?? null;
  }
}

function rowFilter(filter: Tags, by: string | null, key: string | null): Tags {
  if (!by || key === null || ["label", "agent_harness", "agent_name", "model", "day"].includes(by)) return filter;
  return { ...filter, [by]: key };
}

function groups(ls: Line[], by: string | null): Map<string | null, Line[]> {
  const out = new Map<string | null, Line[]>();
  for (const l of ls) {
    const key = by ? keyOf(l, by) : null;
    out.set(key, [...(out.get(key) ?? []), l]);
  }
  return new Map([...out.entries()].sort(([a], [b]) => (a === null ? 1 : b === null ? -1 : a.localeCompare(b))));
}

function reportSetup(url: URL) {
  return {
    filter: filterFromText(url.searchParams.get("filter") ?? ""),
    by: url.searchParams.get("by") || null,
    approved: url.searchParams.get("approved") === "true",
    share: url.searchParams.get("view") === "share",
    as_of: url.searchParams.get("as_of") ?? now(),
  };
}

/** Which tags a person may assign: their own sessions, or with assign inside a filter. */
function mayAssign(db: Db, me: Member, s: MockSession): boolean {
  if (s.person === me.email || me.role === "org_manager") return true;
  const tags = fullTags(db, splitOf(db, s).parts[0]?.tags ?? {}, s.person);
  if (me.role === "people_manager" && me.filters.some((f) => contains(tags, f))) return true;
  return db.grants.some((g) => g.person === me.email && g.capability === "assign" && contains(tags, g.filter));
}

/** Runs a rule back over sessions since the cutoff that nobody assigned. */
function runBack(db: Db, rule: Pick<Rule, "signal" | "pattern" | "tag_name" | "key" | "scope">): void {
  const since = db.cutoff.cutoff ?? "0000-00-00";
  for (const s of db.sessions) {
    if (s.started_at.slice(0, 10) <= since || db.splits.some((x) => x.session_id === s.session_id) || s.ruleTags[rule.tag_name]) continue;
    if (rule.scope !== "organization" && rule.scope !== s.person) continue;
    const signal = s.signals[rule.signal];
    if (typeof signal !== "string") continue;
    const match = new RegExp(rule.pattern).exec(signal);
    if (!match) continue;
    const key = rule.key ?? match.groups?.key ?? match[0];
    if (db.tagValues.some((v) => v.name === rule.tag_name && v.key === key)) s.ruleTags = { ...s.ruleTags, [rule.tag_name]: key };
  }
}

function estimateView(db: Db, e: MockEstimate) {
  const accepted = acceptedFor(db, e.tags)?.id === e.id;
  const cost = costOf(lines(db).filter((l) => l.day >= e.from && l.day <= e.to && contains(l.tags, e.tags)));
  const hasCost = cost > 0;
  return {
    id: e.id,
    work: e.work,
    tags: e.tags as Record<string, never>,
    amount: dec(e.median),
    median: dec(e.median),
    markup_percent: e.markup_percent,
    distribution: e.bands,
    estimator: e.estimator,
    from: e.from,
    to: e.to,
    currency: "USD",
    recorded_at: e.recorded_at,
    accepted,
    cost: hasCost ? dec(cost) : null,
    deviation: accepted && hasCost ? dec(e.median - cost) : null,
    deviation_label: accepted && hasCost ? e.label : null,
  };
}

function reconciliationOf(db: Db, b: Db["bills"][number]) {
  const ls = lines(db).filter((l) => l.message.provider === b.provider && l.day >= b.from && l.day <= b.to);
  const messages = costOf(ls);
  const unexplained = b.amount - messages;
  const ids = new Set<string>();
  const unknown = new Set<string>();
  for (const l of ls) {
    if (l.costLabel === "count_error") ids.add(l.message.message_id);
    if (l.costLabel === "unknown_model") unknown.add(`${l.message.provider}/${l.message.model}`);
  }
  return { bill: dec(b.amount), messages: dec(messages), unexplained: dec(unexplained), a_message_is_wrong: unexplained < 0, count_errors: ids.size, unknown_models: [...unknown], currency: "USD" };
}

function billView(db: Db, b: Db["bills"][number]) {
  return { ...b, amount: dec(b.amount), lines: [], exchange_rate: null, exchange_rate_given: false, reversed_by: null, reconciliation: reconciliationOf(db, b) };
}

function forecastPast(w: Window): Window[] {
  const n = days(w);
  const out: Window[] = [];
  let to = new Date(Date.parse(w.from) - 86_400_000);
  for (let i = 0; i < 3; i += 1) {
    const from = new Date(to.getTime() - (n - 1) * 86_400_000);
    out.unshift({ from: from.toISOString().slice(0, 10), to: to.toISOString().slice(0, 10) });
    to = new Date(from.getTime() - 86_400_000);
  }
  return out;
}

export const handlers = [
  // A build's own answers come first, so they win where they answer a path too.
  ...addedMocks({ API, member, fail: error, now }),

  // Sign-in, sign-up and the session
  http.post(`${API}/sign-in`, async ({ request }) => {
    const db = load();
    const { email } = (await request.json()) as { email: string };
    const person = db.people.find((p) => p.email.toLowerCase() === email.trim().toLowerCase() && p.status !== "removed");
    const token = person ? `link_${person.id}` : `signup_${crypto.randomUUID()}`;
    db.pendingLink = person ? { token, email: person.email } : { token, email: email.trim().toLowerCase(), url: `/signup?token=${token}` };
    save(db);
    return HttpResponse.json({ poll_token: null }, { status: 202 });
  }),
  http.post(`${API}/sign-up`, async ({ request }) => {
    const body = (await request.json()) as SignUpRequest;
    if (!body.organization_name.trim()) return error(400, "bad_request", "Give the organization a name.");
    save(organizationOf(createDb(), body.email.trim(), body));
    return HttpResponse.json({ poll_token: null }, { status: 202 });
  }),
  http.get(`${API}/sign-up/link`, ({ request }) => {
    const token = new URL(request.url).searchParams.get("token");
    const link = load().pendingLink;
    if (!link?.url || link.token !== token) return error(410, "gone", "This link no longer works: it was used, or it is older than 15 minutes.");
    return HttpResponse.json({ email: link.email });
  }),
  http.post(`${API}/sign-up/link`, async ({ request }) => {
    const body = (await request.json()) as SignUpWithLink;
    const link = load().pendingLink;
    if (!link?.url || link.token !== body.token) return error(410, "gone", "This link no longer works: it was used, or it is older than 15 minutes.");
    if (!body.organization_name.trim()) return error(400, "bad_request", "Give the organization a name.");
    const db = organizationOf(createDb(), link.email, body);
    const founder = db.people[0];
    if (founder) Object.assign(founder, { status: "joined", joined_at: now() });
    db.session = founder?.id ?? null;
    db.pendingLink = null;
    save(db);
    return new HttpResponse(null, { status: 204 });
  }),
  http.post(`${API}/sign-in/link`, async ({ request }) => {
    const db = load();
    const token = new URLSearchParams(await request.text()).get("token") ?? "";
    const person = db.people.find((p) => `link_${p.id}` === token);
    if (!person) return new HttpResponse("This sign-in link has expired. Ask for a new one.", { status: 410, headers: { "content-type": "text/html" } });
    const start = db.pendingLink?.start ?? "/";
    db.session = person.id;
    db.pendingLink = null;
    if (person.status === "invited") Object.assign(person, { status: "joined", joined_at: now() });
    save(db);
    return new HttpResponse(null, { status: 303, headers: { location: start } });
  }),
  http.post(`${API}/sign-out`, () => {
    const db = load();
    db.session = null;
    save(db);
    return new HttpResponse(null, { status: 204 });
  }),
  http.get(`${API}/invites/:code`, ({ params }) => {
    const db = load();
    const invite = db.invites.find((i) => i.code === params.code);
    const person = db.people.find((p) => p.id === invite?.person && p.status === "invited");
    if (!person) return error(404, "not_found", "This invite link does not work any more. Ask for a new one.");
    return HttpResponse.json({ email: person.email, organization_name: db.organization.name });
  }),
  http.get(`${API}/health`, () => HttpResponse.json({ status: "ok", database: "ok", version: "1.0.0", edition: mockEdition() })),
  http.get(`${API}/__mock/last-link`, () => HttpResponse.json(load().pendingLink)),
  http.post(`${API}/__mock/reset`, () => {
    resetMockDb();
    return new HttpResponse(null, { status: 204 });
  }),
  http.get(`${API}/me`, member(({ db, me }) => HttpResponse.json({ credential: "session", person: me, organization: { id: db.organization.id, name: db.organization.name, slug: db.organization.slug }, org_manager_notice: null }))),

  // The organization
  http.get(`${API}/organization`, member(({ db }) => HttpResponse.json({ ...db.organization, cutoff: db.cutoff.cutoff }))),
  http.patch(
    `${API}/organization`,
    member(async ({ db, request }) => {
      const change = (await request.json()) as Record<string, unknown>;
      for (const [key, value] of Object.entries(change)) {
        if (value !== null && ["name", "approval", "reporting_currency", "sign_in_domains"].includes(key)) Object.assign(db.organization, { [key]: value });
      }
      return HttpResponse.json({ ...db.organization, cutoff: db.cutoff.cutoff });
    }, "org_manager"),
  ),

  // People, attributes and grants
  http.get(`${API}/members`, member(({ db }) => HttpResponse.json(db.people.filter((p) => p.status !== "removed")))),
  http.get(`${API}/organization/member-limit`, member(({ db }) => HttpResponse.json({ members: db.people.filter((p) => p.status !== "removed").length, member_limit: null, message: null }))),
  http.post(
    `${API}/members`,
    member(async ({ db, request }) => {
      const body = (await request.json()) as { email: string; name?: string; role?: Role; attributes?: Tags };
      if (db.people.some((p) => p.email.toLowerCase() === body.email.toLowerCase() && p.status !== "removed")) return error(409, "conflict", "Someone with this email is already a member.");
      const person: Member = {
        id: `p0000000-0000-4000-8000-${String(db.nextId++).padStart(12, "0")}`,
        name: body.name?.trim() || (body.email.split("@")[0] ?? body.email),
        email: body.email,
        role: body.role ?? "agent_manager",
        attributes: (body.attributes ?? {}) as Member["attributes"],
        filters: [],
        status: "invited",
        invited_at: now(),
        joined_at: null,
      };
      db.people.push(person);
      const code = `${db.organization.slug}-${String(db.nextId++).slice(-4)}`;
      db.invites.push({ code, person: person.id });
      return HttpResponse.json({ member: person, invite_url: `${new URL(request.url).origin}/i/${code}` }, { status: 201 });
    }, "people_manager"),
  ),
  http.patch(
    `${API}/members/:id`,
    member(async ({ db, params, request }) => {
      const person = db.people.find((p) => p.id === params.id);
      if (!person) return error(404, "not_found", "No such member.");
      const change = (await request.json()) as { name?: string | null; role?: Role | null; filters?: Tags[] | null };
      const managers = db.people.filter((p) => p.role === "org_manager" && p.status !== "removed");
      if (change.role && change.role !== "org_manager" && person.role === "org_manager" && managers.length === 1) return error(409, "conflict", "The last org manager cannot change role. Make someone else an org manager first.");
      if (change.name) person.name = change.name;
      if (change.role) person.role = change.role;
      if (change.filters) person.filters = change.filters as Member["filters"];
      if (person.role !== "people_manager") person.filters = [];
      return HttpResponse.json(person);
    }, "org_manager"),
  ),
  http.post(`${API}/members/:id/invite`, member(({ db, params, request }) => {
    const person = db.people.find((p) => p.id === params.id);
    if (!person) return error(404, "not_found", "No such member.");
    const code = db.invites.find((i) => i.person === person.id)?.code ?? "kestrel-0000";
    return HttpResponse.json({ member: person, invite_url: `${new URL(request.url).origin}/i/${code}` });
  }, "people_manager")),
  http.delete(`${API}/members/:id`, member(({ db, params, me }) => {
    const person = db.people.find((p) => p.id === params.id);
    if (!person) return error(404, "not_found", "No such member.");
    if (person.id === me.id) return error(409, "conflict", "You cannot remove yourself.");
    person.status = "removed";
    return new HttpResponse(null, { status: 204 });
  }, "org_manager")),
  http.get(`${API}/attributes`, member(({ db }) => HttpResponse.json(db.attributeNames.map((n) => ({ ...n, values: db.attributeValues.filter((v) => v.name === n.name).map(valueView) }))))),
  http.post(`${API}/attributes`, member(async ({ db, request }) => {
    const { name } = (await request.json()) as { name: string };
    if (!/^[a-z][a-z0-9_-]*$/.test(name)) return error(400, "bad_request", "An attribute name is lowercase letters, digits, - and _.");
    if (db.attributeNames.some((n) => n.name === name)) return new HttpResponse(null, { status: 200 });
    db.attributeNames.push({ name, built_in: false });
    return new HttpResponse(null, { status: 201 });
  }, "org_manager")),
  http.post(`${API}/attributes/:name/values`, member(async ({ db, me, params, request }) => {
    const body = (await request.json()) as { key: string; title?: string | null; defaults?: Tags | null };
    if (!db.attributeNames.some((n) => n.name === params.name)) return error(404, "not_found", `There is no attribute name ${params.name ?? ""}.`);
    if (!body.key.trim()) return error(400, "bad_request", "Give the value a key.");
    const v: MockValue = { id: nextId(db, "val"), name: params.name ?? "", key: body.key.trim(), title: body.title ?? null, description: null, source_id: null, source_status: null, defaults: body.defaults ?? {}, recorded_at: now() };
    db.attributeValues = [...db.attributeValues.filter((x) => !(x.name === v.name && x.key === v.key)), v];
    record(db, "attribute value", { name: v.name, key: v.key }, me.email, v.id);
    return HttpResponse.json(valueView(v), { status: 201 });
  }, "people_manager")),
  http.post(`${API}/people/:id/attributes`, member(async ({ db, me, params, request }) => {
    const body = (await request.json()) as { name: string; key: string; from?: string | null };
    const person = db.people.find((p) => p.id === params.id);
    if (!person) return error(404, "not_found", "No such member.");
    if (!db.attributeValues.some((v) => v.name === body.name && v.key === body.key)) return error(400, "bad_request", `${body.name}: ${body.key} is not an attribute value yet. Add it first.`);
    person.attributes = { ...(person.attributes as Tags), [body.name]: body.key } as Member["attributes"];
    const view = { id: nextId(db, "pat"), person: person.id, name: body.name, key: body.key, from: body.from ?? null };
    db.personAttributes.push(view);
    record(db, "attribute", view, me.email, view.id);
    return HttpResponse.json(view, { status: 201 });
  }, "people_manager")),
  http.get(`${API}/grants`, member(({ db, me }) => {
    const fromRoles = db.people
      .filter((p) => p.status !== "removed")
      .flatMap((p) => {
        const self = { person: p.email };
        const grants: { capability: string; filter: Tags }[] = [{ capability: "record", filter: {} }, { capability: "view", filter: self }];
        if (p.role === "people_manager") for (const f of p.filters as unknown as Tags[]) for (const c of ["view", "assign", "approve", "plan", "grant"]) grants.push({ capability: c, filter: f });
        if (p.role === "org_manager") for (const c of ["view", "assign", "approve", "plan", "grant", "adjust"]) grants.push({ capability: c, filter: {} });
        return grants.map((g, i) => ({ id: `role_${p.id}_${String(i)}`, person: p.email, capability: g.capability, filter: g.filter as Record<string, never>, from_role: true, recorded_at: p.invited_at }));
      });
    const all = [...fromRoles, ...db.grants];
    return HttpResponse.json(me.role === "org_manager" ? all : all.filter((g) => g.person === me.email || (me.role === "people_manager" && me.filters.some((f) => contains(g.filter, f)))));
  })),
  http.post(`${API}/grants`, member(async ({ db, me, request }) => {
    const body = (await request.json()) as { person: string; capability: string; filter?: Tags };
    if (!["view", "assign", "approve", "plan", "grant"].includes(body.capability)) return error(400, "bad_request", "The capability is one of view, assign, approve, plan or grant.");
    if (!db.people.some((p) => p.email === body.person)) return error(400, "bad_request", `${body.person} is not a member.`);
    const filter = body.filter ?? {};
    if (me.role !== "org_manager" && !me.filters.some((f) => contains(filter, f))) return error(403, "forbidden", "You can grant only inside your own filters.");
    db.grants = db.grants.filter((g) => !(g.person === body.person && g.capability === body.capability));
    const grant = { id: nextId(db, "grt"), person: body.person, capability: body.capability, filter: filter as Record<string, never>, from_role: false, recorded_at: now() };
    db.grants.push(grant);
    record(db, "grant", grant, me.email, grant.id);
    return HttpResponse.json(grant, { status: 201 });
  }, "people_manager")),

  // Tags
  http.get(`${API}/tags`, member(({ db }) => HttpResponse.json(db.tagNames.map((n) => ({ ...n, values: db.tagValues.filter((v) => v.name === n.name).map(valueView) }))))),
  http.post(`${API}/tags`, member(async ({ db, request }) => {
    const { name } = (await request.json()) as { name: string };
    if (!/^[a-z][a-z0-9_-]*$/.test(name)) return error(400, "bad_request", "A tag name is lowercase letters, digits, - and _.");
    if (["person", "team", "department", "location"].includes(name)) return error(400, "bad_request", `${name} is an attribute name, not a tag name.`);
    if (db.tagNames.some((n) => n.name === name)) return new HttpResponse(null, { status: 200 });
    db.tagNames.push({ name, built_in: false });
    return new HttpResponse(null, { status: 201 });
  }, "org_manager")),
  http.delete(`${API}/tags/:name`, member(({ db, params }) => {
    const found = db.tagNames.find((n) => n.name === params.name);
    if (!found) return error(404, "not_found", "No such tag name.");
    if (found.built_in) return error(409, "conflict", "The built-in tag names stay.");
    db.tagNames = db.tagNames.filter((n) => n !== found);
    return new HttpResponse(null, { status: 204 });
  }, "org_manager")),
  http.post(`${API}/tags/:name/values`, member(async ({ db, me, params, request }) => {
    const body = (await request.json()) as { key: string; title?: string | null; description?: string | null; defaults?: Tags | null };
    if (!db.tagNames.some((n) => n.name === params.name)) return error(404, "not_found", `There is no tag name ${params.name ?? ""}.`);
    if (!body.key.trim()) return error(400, "bad_request", "Give the value a key.");
    const v: MockValue = { id: nextId(db, "val"), name: params.name ?? "", key: body.key.trim(), title: body.title ?? null, description: body.description ?? null, source_id: null, source_status: null, defaults: body.defaults ?? {}, recorded_at: now() };
    db.tagValues = [...db.tagValues.filter((x) => !(x.name === v.name && x.key === v.key)), v];
    record(db, "tag value", { name: v.name, key: v.key }, me.email, v.id);
    return HttpResponse.json(valueView(v), { status: 201 });
  }, "people_manager")),

  // Sessions, messages, splits
  http.get(`${API}/sessions`, member(({ db, me, url }) => {
    const filter = filterFromText(url.searchParams.get("filter") ?? "");
    const from = url.searchParams.get("from") ?? "0000-00-00";
    const to = url.searchParams.get("to") ?? "9999-99-99";
    const one = url.searchParams.get("session_id");
    const unassigned = url.searchParams.get("unassigned") === "true";
    const sessions = db.sessions
      .filter((s) => !one || s.session_id === one)
      .filter((s) => s.started_at.slice(0, 10) >= from && s.started_at.slice(0, 10) <= to)
      .map((s) => ({ s, view: sessionView(db, s) }))
      .filter(({ view }) => view.parts.some((p) => canSee(db, me, p.tags) && contains(p.tags, filter)))
      .filter(({ view }) => !unassigned || view.assigned_by === "default")
      .sort((a, b) => b.s.started_at.localeCompare(a.s.started_at))
      .map(({ view }) => view);
    return HttpResponse.json({ as_of: now(), sessions });
  })),
  http.get(`${API}/messages`, member(({ db, me, url }) => {
    const one = url.searchParams.get("session_id");
    const messages = db.sessions
      .filter((s) => !one || s.session_id === one)
      .flatMap((s) => {
        const split = splitOf(db, s);
        return s.messages.map((m) => {
          const cost = messageCost(db, m);
          return {
            message_id: m.message_id,
            session_id: s.session_id,
            person: s.person,
            provider: m.provider,
            model: m.model,
            at: m.at,
            agent_harness: s.agent_harness,
            agent_name: s.agent_name,
            tokens: m.tokens,
            cost: cost.nanos === null ? null : dec(cost.nanos),
            cost_label: cost.label,
            reported_cost: null,
            facts: {},
            signals: {},
            assigned_by: split.by,
            submitted: submittedOf(db, s),
            parts: split.parts.map((p) => {
              const tags = fullTags(db, p.tags, s.person);
              return { fraction: String(p.bp / 10_000), assigned: p.tags as Record<string, never>, tags: tags as Record<string, never>, label: labelOf(tags), amount: dec(((cost.nanos ?? 0) * p.bp) / 10_000) };
            }),
          };
        });
      })
      .filter((m) => m.parts.some((p) => canSee(db, me, p.tags)));
    return HttpResponse.json({ as_of: now(), messages });
  })),
  http.post(`${API}/splits`, member(async ({ db, me, request }) => {
    const body = (await request.json()) as { session_id: string; parts: { fraction?: string | null; tags?: Tags }[] };
    const s = db.sessions.find((x) => x.session_id === body.session_id);
    if (!s) return error(404, "not_found", "No such session.");
    if (!mayAssign(db, me, s)) return error(403, "forbidden", "You may split your own sessions, or others' inside a filter you hold assign on.");
    if (s.started_at.slice(0, 10) <= (db.cutoff.cutoff ?? "") && me.role !== "org_manager") return error(403, "forbidden", "This session is before the cutoff. Only an org manager with adjust may add events there.");
    if (!body.parts.length) return error(400, "bad_request", "A split has at least one part.");
    const bps = body.parts.map((p) => Math.round(Number(p.fraction ?? (body.parts.length === 1 ? "1" : "NaN")) * 10_000));
    if (bps.some((b) => !Number.isFinite(b) || b <= 0)) return error(400, "bad_request", "Each fraction is a number above 0.");
    if (bps.reduce((a, b) => a + b, 0) !== 10_000) return error(400, "bad_request", "The fractions of a split add up to 1.");
    for (const part of body.parts) {
      for (const [name, key] of Object.entries(part.tags ?? {})) {
        if (!db.tagNames.some((n) => n.name === name)) return error(400, "bad_request", `${name} is not a tag name.`);
        if (!db.tagValues.some((v) => v.name === name && v.key === key)) return error(400, "bad_request", `${name}: ${key} is not a tag value yet.`);
      }
    }
    const split = { id: nextId(db, "spl"), session_id: s.session_id, parts: body.parts.map((p, i) => ({ bp: bps[i] ?? 0, tags: p.tags ?? {} })), recorded_at: now(), by: me.email };
    const earlier = db.splits.filter((x) => x.session_id === s.session_id).at(-1);
    db.splits.push(split);
    record(db, "split", { session_id: s.session_id }, me.email, split.id);
    const view = sessionView(db, s);
    return HttpResponse.json({ id: split.id, session_id: s.session_id, message_id: null, parts: view.parts, recorded_at: split.recorded_at, supersedes: earlier?.id ?? null }, { status: 201 });
  })),

  // Submissions and approval
  http.get(`${API}/submissions`, member(({ db, me }) => {
    const mine = (p: string) => p === me.email;
    const inView = (p: string) => canSee(db, me, fullTags(db, {}, p));
    return HttpResponse.json(db.submissions.filter((s) => mine(s.person) || inView(s.person)).sort((a, b) => b.to.localeCompare(a.to)));
  })),
  http.post(`${API}/submissions`, member(({ db, me }) => {
    const last = db.submissions.filter((s) => s.person === me.email && s.status !== "rejected").map((s) => s.to).sort().at(-1);
    const rejected = db.submissions.filter((s) => s.person === me.email && s.status === "rejected");
    const from = rejected.map((s) => s.from).sort()[0] ?? (last ? new Date(Date.parse(last) + 86_400_000).toISOString().slice(0, 10) : "2026-01-01");
    db.submissions = db.submissions.filter((s) => !(s.person === me.email && s.status === "rejected"));
    const mineIn = db.sessions.filter((s) => s.person === me.email && s.started_at.slice(0, 10) >= from && s.started_at.slice(0, 10) <= TODAY);
    if (!mineIn.length) return error(400, "nothing_to_submit", "There is nothing to submit: no messages since your last submit.");
    const cost = mineIn.reduce((sum, s) => sum + s.messages.reduce((t, m) => t + (messageCost(db, m).nanos ?? 0), 0), 0);
    const sub = { id: nextId(db, "sub"), person: me.email, from, to: TODAY, messages: mineIn.reduce((n, s) => n + s.messages.length, 0), cost: dec(cost), status: db.organization.approval ? "submitted" : "approved", reason: null };
    db.submissions.push(sub);
    record(db, "submit", sub, me.email, sub.id);
    return HttpResponse.json(sub, { status: 201 });
  })),
  http.post(`${API}/submissions/:id/approve`, member(({ db, me, params }) => {
    const sub = db.submissions.find((s) => s.id === params.id);
    if (!sub) return error(404, "not_found", "No such submission.");
    if (!db.organization.approval) return error(409, "conflict", "Approval is off in this organization.");
    if (sub.person === me.email) return error(403, "forbidden", "Nobody approves their own submission.");
    if (!canSee(db, me, fullTags(db, {}, sub.person))) return error(403, "forbidden", "You hold approve on no filter its messages are in.");
    sub.status = "approved";
    sub.reason = null;
    record(db, "approval", { submission: sub.id, approved: true }, me.email);
    return HttpResponse.json(sub);
  }, "people_manager")),
  http.post(`${API}/submissions/:id/reject`, member(async ({ db, me, params, request }) => {
    const sub = db.submissions.find((s) => s.id === params.id);
    if (!sub) return error(404, "not_found", "No such submission.");
    const { reason } = (await request.json()) as { reason: string };
    if (!reason.trim()) return error(400, "bad_request", "Give a reason, so they know what to fix.");
    if (sub.person === me.email) return error(403, "forbidden", "Nobody rejects their own submission.");
    sub.status = "rejected";
    sub.reason = reason.trim();
    record(db, "approval", { submission: sub.id, approved: false, reason }, me.email);
    return HttpResponse.json(sub);
  }, "people_manager")),
  http.get(`${API}/submit-requests`, member(({ db, me }) =>
    HttpResponse.json(db.submitRequests.map((r) => ({ ...r, done: db.submissions.some((s) => s.person === me.email && s.to >= r.until && s.status !== "rejected") }))),
  )),
  http.post(`${API}/submit-requests`, member(async ({ db, me, request }) => {
    const body = (await request.json()) as { until: string; message?: string | null };
    const req = { id: nextId(db, "req"), until: body.until, message: body.message ?? null, requested_at: now(), requested_by: me.email, done: false };
    db.submitRequests.push(req);
    return HttpResponse.json(req, { status: 201 });
  }, "org_manager")),

  // Reversals and the log
  http.post(`${API}/reversals`, member(async ({ db, me, request }) => {
    const { event_id } = (await request.json()) as { event_id: string };
    const before = JSON.stringify([db.rules, db.grants, db.prices, db.splits, db.saved]);
    db.rules = db.rules.filter((r) => r.id !== event_id || (r.scope === "organization" && me.role !== "org_manager"));
    db.grants = db.grants.filter((g) => g.id !== event_id);
    db.prices = db.prices.filter((p) => p.id !== event_id || p.source === "managed");
    db.splits = db.splits.filter((s) => s.id !== event_id);
    db.saved = db.saved.filter((s) => s.id !== event_id);
    if (before === JSON.stringify([db.rules, db.grants, db.prices, db.splits, db.saved])) return error(404, "not_found", "There is no event with that id you may reverse.");
    const id = record(db, "reversal", { event_id }, me.email);
    return HttpResponse.json({ id, event_id }, { status: 201 });
  })),
  http.get(`${API}/events`, member(({ db, url }) => {
    const kind = url.searchParams.get("kind");
    const estimates = db.estimates.map((e) => ({ id: e.id, kind: "estimate", record: { kind: "estimate", description: e.work, tags: e.tags }, recorded_at: e.recorded_at, recorded_by: e.estimator === "classifier" ? null : e.estimator, supersedes: null }));
    return HttpResponse.json([...estimates, ...db.events].filter((e) => !kind || e.kind === kind));
  })),

  // Plans and estimates
  http.post(`${API}/estimates`, member(async ({ db, me, request }) => {
    const body = (await request.json()) as { work: string; tags?: Tags; median?: string | null; markup_percent?: number | null; from?: string | null; to?: string | null };
    if (!body.work.trim()) return error(400, "bad_request", "Describe the work.");
    const markup = body.markup_percent ?? 0;
    const from = body.from ?? TODAY;
    const to = body.to ?? new Date(Date.parse(from) + 29 * 86_400_000).toISOString().slice(0, 10);
    // The classifier's answer in the mocks: 140 for every 100 characters, at least 60.
    const base = body.median ? parseDec(body.median) : Math.max(60, Math.round(body.work.length * 1.4)) * 1e9;
    const median = Math.round((base * (100 + markup)) / 100);
    const m = median / 1e9;
    const bands = body.median ? [] : [
      { p: "0.25", up_to: (m * 0.7).toFixed(2) },
      { p: "0.50", up_to: (m * 1.3).toFixed(2) },
      { p: "0.25", up_to: null },
    ];
    const e: MockEstimate = { id: nextId(db, "est"), work: body.work.trim(), tags: body.tags ?? {}, median, markup_percent: markup, bands, estimator: body.median ? me.email : "classifier", from, to, recorded_at: now(), label: null };
    db.estimates.push(e);
    return HttpResponse.json(estimateView(db, e), { status: 201 });
  })),
  http.get(`${API}/estimates/:id`, member(({ db, params }) => {
    const e = db.estimates.find((x) => x.id === params.id);
    return e ? HttpResponse.json(estimateView(db, e)) : error(404, "not_found", "No such estimate.");
  })),
  http.post(`${API}/estimates/:id/accept`, member(({ db, me, params }) => {
    const e = db.estimates.find((x) => x.id === params.id);
    if (!e) return error(404, "not_found", "No such estimate.");
    if (!Object.keys(e.tags).length) return error(400, "bad_request", "Tag the estimate first: a plan is for a tag set.");
    db.accepted[filterKey(e.tags)] = e.id;
    const id = record(db, "accept", { estimate: e.id }, me.email);
    return HttpResponse.json({ id, estimate: estimateView(db, e) }, { status: 201 });
  }, "people_manager")),
  http.get(`${API}/shares`, member(({ db }) => {
    const sums = (project: string, day: string) => db.shares.filter((s) => s.project === project && s.from <= day && (!s.to || s.to >= day)).reduce((t, s) => t + Number(s.fraction), 0);
    return HttpResponse.json(db.shares.map((s) => {
      const total = sums(s.project, s.from);
      return { ...s, over: total > 1 ? String(Math.round(total * 100) / 100) : null };
    }));
  })),
  http.post(`${API}/shares`, member(async ({ db, me, request }) => {
    const body = (await request.json()) as { person: string; project: string; fraction: string; from: string; to?: string | null };
    const fraction = Number(body.fraction);
    if (!(fraction > 0 && fraction <= 1)) return error(400, "bad_request", "A share is a fraction above 0 and up to 1.");
    if (!db.tagValues.some((v) => v.name === "project" && v.key === body.project)) return error(400, "bad_request", `project: ${body.project} is not a tag value.`);
    if (!db.people.some((p) => p.email === body.person)) return error(400, "bad_request", `${body.person} is not a member.`);
    const share = { id: nextId(db, "shr"), person: body.person, project: body.project, fraction: String(fraction), from: body.from, to: body.to ?? null, over: null };
    db.shares.push(share);
    record(db, "share", share, me.email, share.id);
    return HttpResponse.json(share, { status: 201 });
  }, "people_manager")),

  // Cutoff
  http.get(`${API}/cutoff`, member(({ db }) => HttpResponse.json(db.cutoff))),
  http.post(`${API}/cutoff`, member(async ({ db, me, request }) => {
    const { cutoff } = (await request.json()) as { cutoff: string };
    if (!/^\d{4}-\d{2}-\d{2}$/.test(cutoff)) return error(400, "bad_request", "The cutoff is a day, YYYY-MM-DD.");
    if (cutoff > TODAY) return error(400, "bad_request", "The cutoff cannot be after today.");
    const id = record(db, "cutoff", { cutoff }, me.email);
    db.cutoff = { cutoff, event_id: id, frozen_as_of: `${cutoff}T00:00:00Z`, moved_at: now(), moved_by: me.email };
    db.organization.cutoff = cutoff;
    return HttpResponse.json(db.cutoff, { status: 201 });
  }, "org_manager")),

  // Price table
  http.get(`${API}/price-table`, member(({ db }) => HttpResponse.json({ as_of: now(), rows: db.prices }))),
  http.post(`${API}/price-table/rows`, member(async ({ db, request }) => {
    const body = (await request.json()) as { provider: string; model: string; kind: string; price_per_million: string; valid_from: string };
    if (!["input", "output", "cache_read", "cache_write"].includes(body.kind)) return error(400, "bad_request", "The token kind is input, output, cache_read or cache_write.");
    if (!/^\d+(\.\d+)?$/.test(body.price_per_million)) return error(400, "bad_request", "The price per million is a decimal number, such as 15.00.");
    if (!body.provider.trim() || !body.model.trim()) return error(400, "bad_request", "Give the provider and the model.");
    if (!/^\d{4}-\d{2}-\d{2}$/.test(body.valid_from)) return error(400, "bad_request", "Valid from is a day, YYYY-MM-DD.");
    const row = { id: nextId(db, "prc"), ...body, provider: body.provider.trim(), model: body.model.trim(), source: "organization", recorded_at: now() };
    db.prices.push(row);
    return HttpResponse.json(row, { status: 201 });
  }, "org_manager")),

  // Bills and reconciliation
  http.get(`${API}/bills`, member(({ db }) => HttpResponse.json(db.bills.map((b) => billView(db, b))), "org_manager")),
  http.post(`${API}/bills`, member(async ({ db, me, request }) => {
    const body = (await request.json()) as { provider: string; from: string; to: string; amount: string; currency: string };
    if (!body.provider.trim()) return error(400, "bad_request", "Give the provider.");
    if (!/^\d+(\.\d+)?$/.test(body.amount)) return error(400, "bad_request", "The amount is a decimal number, such as 1000.00.");
    if (!(body.from <= body.to)) return error(400, "bad_request", "The first day is on or before the last.");
    const bill = { id: nextId(db, "bil"), provider: body.provider.trim(), from: body.from, to: body.to, amount: parseDec(body.amount), currency: body.currency.toUpperCase(), source: "manual" as const, final: true, recorded_at: now(), recorded_by: me.email };
    db.bills = db.bills.filter((b) => !(b.provider === bill.provider && b.from === bill.from && b.to === bill.to));
    db.bills.push(bill);
    return HttpResponse.json(billView(db, bill), { status: 201 });
  }, "org_manager")),
  http.get(`${API}/bill-connections`, member(({ db }) => HttpResponse.json(db.connections), "org_manager")),
  http.post(`${API}/bill-connections`, member(async ({ db, request }) => {
    const body = (await request.json()) as { provider: "anthropic_enterprise" | "anthropic_console" | "openai"; key: string };
    if (body.key.trim().length < 12) return error(400, "bad_request", "That does not look like an admin key.");
    const c = { id: nextId(db, "con"), provider: body.provider, key_hint: `${body.key.slice(0, 7)}…${body.key.slice(-4)}`, status: "ok" as const, last_pull_at: null, message: null };
    db.connections.push(c);
    return HttpResponse.json(c, { status: 201 });
  }, "org_manager")),
  http.delete(`${API}/bill-connections/:id`, member(({ db, params }) => {
    db.connections = db.connections.filter((c) => c.id !== params.id);
    return new HttpResponse(null, { status: 204 });
  }, "org_manager")),
  http.post(`${API}/bill-connections/:id/pull`, member(({ db, params }) => {
    const c = db.connections.find((x) => x.id === params.id);
    if (!c) return error(404, "not_found", "No such connection.");
    c.last_pull_at = now();
    return HttpResponse.json(c, { status: 202 });
  }, "org_manager")),
  http.get(`${API}/reconciliation`, member(({ db }) =>
    HttpResponse.json(db.bills.map((b) => ({ bill_id: b.id, provider: b.provider, from: b.from, to: b.to, final: b.final, ...reconciliationOf(db, b) }))),
  "org_manager")),

  // Reports
  http.get(`${API}/reports/cost`, member(({ db, me, url }) => {
    const w = windowOf(url);
    if (w instanceof Response) return w;
    const q = reportSetup(url);
    const ls = visibleLines(db, me, q.filter, w, q.approved);
    const total = costOf(ls);
    const rows = [...groups(ls, q.by).entries()].map(([key, group]) => {
      const amount = costOf(group);
      const share = total ? (amount / total).toFixed(6) : "0";
      return { key, amount: q.share ? share : dec(amount), share, messages: new Set(group.map((l) => l.message.message_id)).size };
    });
    const report: CostReport = {
      as_of: q.as_of,
      from: w.from,
      to: w.to,
      filter: q.filter,
      by: q.by,
      currency: "USD",
      view: q.share ? "share" : "money",
      total: dec(total),
      rows,
      count_errors: new Set(ls.filter((l) => l.costLabel === "count_error").map((l) => l.message.message_id)).size,
      unknown_models: [...new Set(ls.filter((l) => l.costLabel === "unknown_model").map((l) => `${l.message.provider}/${l.message.model}`))],
    };
    return HttpResponse.json(report);
  })),
  http.get(`${API}/reports/deviation`, member(({ db, me, url }) => {
    const w = windowOf(url);
    if (w instanceof Response) return w;
    const q = reportSetup(url);
    const ls = visibleLines(db, me, q.filter, w, q.approved);
    const rows = [...groups(ls, q.by).keys()].map((key) => {
      const filter = rowFilter(q.filter, q.by, key);
      const cost = costOf(ls.filter((l) => contains(l.tags, filter)));
      const plan = planOf(db, filter, w);
      const estimate = acceptedFor(db, filter);
      const delta = plan === null ? null : plan - cost;
      return { key, filter, cost: dec(cost), plan: plan === null ? null : dec(plan), deviation: delta === null ? null : dec(delta), favourable: delta === null ? null : delta >= 0, label: delta ? (estimate?.label ?? null) : null };
    });
    const report: DeviationReport = { as_of: q.as_of, from: w.from, to: w.to, filter: q.filter, by: q.by, currency: "USD", rows };
    return HttpResponse.json(report);
  })),
  http.get(`${API}/reports/forecast`, member(({ db, me, url }) => {
    const w = windowOf(url);
    if (w instanceof Response) return w;
    const q = reportSetup(url);
    const past = forecastPast(w);
    const pastLines = past.map((p) => visibleLines(db, me, q.filter, p, q.approved));
    const keys = [...new Set(pastLines.flatMap((ls) => ls.map((l) => (q.by ? keyOf(l, q.by) : null))))];
    const rows = keys.map((key) => {
      const filter = rowFilter(q.filter, q.by, key);
      const costs = pastLines.map((ls) => costOf(ls.filter((l) => contains(l.tags, filter) && (!q.by || keyOf(l, q.by) === key))));
      const perDay = costs.reduce((a, b) => a + b, 0) / past.reduce((n, p) => n + days(p), 0);
      return { key, filter, forecast: dec(perDay * days(w)), past: past.map((p, i) => ({ from: p.from, to: p.to, cost: dec(costs[i] ?? 0) })) };
    });
    const report: ForecastReport = { as_of: q.as_of, from: w.from, to: w.to, filter: q.filter, by: q.by, currency: "USD", rows };
    return HttpResponse.json(report);
  })),
  http.get(`${API}/reports/saved`, member(({ db }) => HttpResponse.json(db.saved))),
  http.post(`${API}/reports/saved`, member(async ({ db, me, request }) => {
    const body = (await request.json()) as { name: string; by?: string | null; filter?: Tags; from?: string | null; to?: string | null };
    if (!body.name.trim()) return error(400, "bad_request", "Give the report a name.");
    const saved = { id: nextId(db, "rep"), name: body.name.trim(), by: body.by ?? null, filter: body.filter ?? {}, from: body.from ?? null, to: body.to ?? null, created_at: now(), created_by: me.email };
    db.saved.push(saved);
    return HttpResponse.json(saved, { status: 201 });
  })),

  // Rules
  http.get(`${API}/rules`, member(({ db, me }) => HttpResponse.json(db.rules.filter((r) => r.scope === "organization" || r.scope === me.email).sort((a, b) => (a.scope === "organization" ? 0 : 1) - (b.scope === "organization" ? 0 : 1) || a.order - b.order)))),
  http.post(`${API}/rules`, member(async ({ db, me, request }) => {
    const body = (await request.json()) as NewRule;
    const problem = ruleProblem(db, body);
    if (problem) return error(400, "bad_request", problem);
    const org = db.rules.filter((r) => r.scope === "organization");
    const rule: Rule = { id: nextId(db, "rul"), name: body.name.trim(), signal: body.signal, pattern: body.pattern, tag_name: body.tag_name, key: body.key ?? null, order: body.order ?? org.length + 1, scope: "organization", recorded_at: now() };
    db.rules.push(rule);
    record(db, "rule", rule, me.email, rule.id);
    runBack(db, rule);
    return HttpResponse.json(rule, { status: 201 });
  }, "people_manager")),
  http.get(`${API}/me/rules`, member(({ db, me }) => HttpResponse.json(db.rules.filter((r) => r.scope === me.email)))),
  http.put(`${API}/me/rules`, member(async ({ db, me, request }) => {
    const body = (await request.json()) as NewRule[];
    for (const r of body) {
      const problem = ruleProblem(db, r);
      if (problem) return error(400, "bad_request", problem);
    }
    const old = db.rules.filter((r) => r.scope === me.email);
    db.rules = db.rules.filter((r) => r.scope !== me.email);
    const mine = body.map((r, i): Rule => ({ id: old.find((o) => o.name === r.name)?.id ?? nextId(db, "rul"), name: r.name.trim(), signal: r.signal, pattern: r.pattern, tag_name: r.tag_name, key: r.key ?? null, order: i + 1, scope: me.email, recorded_at: now() }));
    db.rules.push(...mine);
    for (const r of mine) runBack(db, r);
    return HttpResponse.json(mine);
  })),

  // API tokens, Jira, the raw export
  http.get(`${API}/api-tokens`, member(({ db, me }) => HttpResponse.json(db.tokens.filter((t) => me.role === "org_manager" || t.person_id === me.id)))),
  http.post(`${API}/api-tokens`, member(async ({ db, me, request }) => {
    const body = (await request.json()) as { kind: "service" | "read" | "organization" | "cli"; name?: string };
    if (body.kind === "organization" && me.role !== "org_manager") return error(403, "forbidden", "Only an org manager creates an organization token.");
    const secret = `tc_${body.kind.slice(0, 3)}_${crypto.randomUUID().replace(/-/g, "")}`;
    const token = { id: nextId(db, "tok"), name: body.name?.trim() || body.kind, kind: body.kind, person_id: me.id, shown_prefix: secret.slice(0, 11), created_at: now(), last_used_at: null, needs_owner: false };
    db.tokens.push(token);
    return HttpResponse.json({ ...token, token: secret }, { status: 201 });
  })),
  http.delete(`${API}/api-tokens/:id`, member(({ db, params }) => {
    db.tokens = db.tokens.filter((t) => t.id !== params.id);
    return new HttpResponse(null, { status: 204 });
  })),
  http.get(`${API}/ticket-systems`, member(({ db }) => HttpResponse.json(db.ticketSystems))),
  http.post(`${API}/ticket-systems`, member(async ({ db, request }) => {
    const body = (await request.json()) as { kind: "jira"; site: string; token: string; email?: string | null };
    if (!/^https:\/\/[a-z0-9-]+\.atlassian\.net\/?$/.test(body.site.trim())) return error(400, "bad_request", "The address is your Jira Cloud site, such as https://acme.atlassian.net.");
    if (!body.email?.trim()) return error(400, "bad_request", "Give the email of the Atlassian account the API token belongs to.");
    if (body.token.trim().length < 8) return error(400, "bad_request", "That does not look like an API token.");
    const system = { id: nextId(db, "tks"), kind: "jira" as const, site: body.site.trim().replace(/\/$/, ""), key_hint: `…${body.token.slice(-4)}`, status: "ok" as const, last_sync_at: null, message: null, mappings: [] };
    db.ticketSystems.push(system);
    return HttpResponse.json(system, { status: 201 });
  }, "org_manager")),
  http.delete(`${API}/ticket-systems/:id`, member(({ db, params }) => {
    db.ticketSystems = db.ticketSystems.filter((t) => t.id !== params.id);
    return new HttpResponse(null, { status: 204 });
  }, "org_manager")),
  http.get(`${API}/ticket-systems/:id/projects`, member(({ db, params }) => {
    if (!db.ticketSystems.some((t) => t.id === params.id)) return error(404, "not_found", "No such ticket system.");
    return HttpResponse.json([{ key: "KD", name: "Kestrel Delivery" }, { key: "OPS", name: "Operations" }]);
  }, "org_manager")),
  http.put(`${API}/ticket-systems/:id/mappings`, member(async ({ db, params, request }) => {
    const system = db.ticketSystems.find((t) => t.id === params.id);
    if (!system) return error(404, "not_found", "No such ticket system.");
    const mappings = (await request.json()) as TicketMapping[];
    for (const m of mappings) if (!db.tagValues.some((v) => v.name === "project" && v.key === m.project)) return error(400, "bad_request", `project: ${m.project} is not a tag value.`);
    system.mappings = mappings;
    return HttpResponse.json(system);
  }, "org_manager")),
  http.post(`${API}/ticket-systems/:id/sync`, member(({ db, params }) => {
    const system = db.ticketSystems.find((t) => t.id === params.id);
    if (!system) return error(404, "not_found", "No such ticket system.");
    system.last_sync_at = now();
    for (const v of db.tagValues) if (v.name === "feature" && v.key.startsWith("KD-")) Object.assign(v, { source_id: `100${v.key.slice(3)}`, source_status: v.key === "KD-1" ? "done" : "in progress" });
    return HttpResponse.json(system, { status: 202 });
  }, "org_manager")),
  http.get(`${API}/exports/raw`, member(() => new HttpResponse(new Blob(["PK"]), { headers: { "content-type": "application/zip" } }), "org_manager")),

  ...enterpriseHandlers({ API, member, fail: error, now }),
];

const SIGNALS = ["branch", "working_directory", "repository", "pull_request_title", "subagent_type", "prompt_keys", "agent_name"];

function ruleProblem(db: Db, rule: NewRule): string | null {
  if (!rule.name.trim()) return "Give the rule a name.";
  if (!SIGNALS.includes(rule.signal) && !/^(config|resource):.+/.test(rule.signal)) return `${rule.signal} is not a signal.`;
  try {
    new RegExp(rule.pattern);
  } catch {
    return "The pattern is not a regular expression.";
  }
  if (!db.tagNames.some((n) => n.name === rule.tag_name)) return `${rule.tag_name} is not a tag name.`;
  if (rule.key && !db.tagValues.some((v) => v.name === rule.tag_name && v.key === rule.key)) return `${rule.tag_name}: ${rule.key} is not a tag value.`;
  return null;
}
