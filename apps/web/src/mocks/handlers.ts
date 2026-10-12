// Mock answers for every call in `src/api/endpoints.ts`, used in development,
// unit tests, Playwright journeys and screenshots until the service has the
// endpoint. State lives in sessionStorage so a reload keeps it.

import { http, HttpResponse, type HttpResponseResolver } from "msw";
import type {
  ApiToken,
  CadenceView,
  ExternalProject,
  NewBill,
  NewTicketSystem,
  NewTokenEntry,
  StoredEntry,
  Person,
  Role,
  OrganizationRulesChange,
  Rule,
  SignUpRequest,
  Tags,
  TicketMapping,
  TicketSystem,
  TokenEntryStatus,
} from "@/api/contract";
import { parseCsv } from "@/lib/csv";
import { filterFromText } from "@/lib/format";
import { customerRelationsHandlers } from "./customer-relations";
import { createDb, defaultRules, TODAY, type Db, type EntryRecord } from "./seed";
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

const ticketSystemName: Record<TicketSystem["kind"], string> = { jira: "Jira Cloud", azure_devops: "Azure DevOps", github_issues: "GitHub Issues" };
const siteExample: Record<TicketSystem["kind"], string> = {
  jira: "a Jira Cloud site, such as https://acme.atlassian.net",
  azure_devops: "an Azure DevOps organization, such as https://dev.azure.com/acme",
  github_issues: "https://api.github.com",
};

/** Every rule list: the organization's and each person's. */
function allRules(db: Db): Rule[] {
  return [...db.rules.rules, ...Object.values(db.personalRules).flat()];
}

/** The service's check of a rule list, or null when it is fine. */
function checkRules(db: Db, rules: Rule[]): string | null {
  if (rules.length > 100) return "A list holds at most 100 rules.";
  for (const rule of rules) {
    const name = rule.name.trim();
    if (!name || name.length > 100) return "Every rule needs a name of 1 to 100 characters.";
    if (typeof rule.then !== "object" || !("set_tags" in rule.then)) continue;
    const tags = Object.keys(rule.then.set_tags);
    if (!tags.length) return `The rule "${name}" sets no tags.`;
    for (const tag of tags) {
      if (tag === "person") return `The rule "${name}" sets person. The person is who did the work; no rule can change it.`;
      if (!db.tagNames.includes(tag)) return `The rule "${name}" sets "${tag}", which is not on the organization's list of tag names.`;
    }
  }
  return null;
}

/** The service's check of a ticket system's address. */
function siteFits(kind: TicketSystem["kind"], site: string): boolean {
  let url: URL;
  try {
    url = new URL(site);
  } catch {
    return false;
  }
  const path = url.pathname.split("/").filter(Boolean);
  if (url.protocol !== "https:" || url.port || url.username || url.search || url.hash) return false;
  if (kind === "jira") return url.hostname.endsWith(".atlassian.net") && !path.length;
  if (kind === "azure_devops") return (url.hostname === "dev.azure.com" && path.length === 1) || (url.hostname.endsWith(".visualstudio.com") && path.length <= 1);
  return (url.hostname === "api.github.com" || (url.hostname.startsWith("api.") && url.hostname.endsWith(".ghe.com"))) && !path.length;
}

/** What the token sees in each ticket system of the example organization. */
const externalProjects: Record<TicketSystem["kind"], ExternalProject[]> = {
  jira: [
    { key: "KD", name: "Storefront" },
    { key: "CP", name: "Claims Portal" },
    { key: "INT", name: "Intranet" },
    { key: "OPS", name: "Operations" },
  ],
  azure_devops: [
    { key: "6ce954b1-ce1f-45d1-b94d-e6bf2464ba2c", name: "Storefront" },
    { key: "0f1c7a52-3d0a-4b0e-9a3e-6a0d8f1e2b47", name: "Claims Portal" },
  ],
  github_issues: [
    { key: "kestrel/storefront", name: "storefront" },
    { key: "kestrel/claims-portal", name: "claims-portal" },
    { key: "kestrel/intranet", name: "intranet" },
  ],
};

/** The tags a token entry gets: its person (by email) and their tags, the ticket, its project and the project's tags. */
function tagsFor(db: Db, personId: string | null, target: string, extra: Tags = {}): Tags {
  const person = db.people.find((p) => p.id === personId);
  const tags: Tags = person ? { person: person.email, ...person.tags } : {};
  const ticket = db.tickets.find((t) => t.key === target);
  const project = db.projects.find((p) => p.id === ticket?.project_id) ?? db.projects.find((p) => p.name === target);
  if (ticket) tags.ticket = ticket.key;
  if (project) Object.assign(tags, { project: project.name }, project.tags);
  return { ...tags, ...extra };
}

function isClosed(db: Db, period: string): boolean {
  return db.periods.find((p) => p.id === period)?.status === "closed";
}

function retag(db: Db, entry: EntryRecord, tags: Tags): Tags {
  // Setting the ticket fills project and the tags the project passes on.
  const merged: Tags = { ...tags };
  const ticket = merged.ticket ? db.tickets.find((t) => t.key === merged.ticket) : undefined;
  if (ticket) {
    const inherited = tagsFor(db, entry.agent_manager, ticket.key);
    return { ...inherited, ...Object.fromEntries(Object.entries(merged).filter(([k]) => !["ticket", "project", "client"].includes(k))) };
  }
  if (merged.project && !merged.ticket) {
    const project = db.projects.find((p) => p.name === merged.project);
    if (project) Object.assign(merged, project.tags);
  }
  return Object.fromEntries(Object.entries(merged).filter(([, value]) => value));
}

/** The service's CSV reading: columns named after the token entry schema, nested ones with a dot. */
function csvEntries(text: string): { rows: { line: number; entry: NewTokenEntry }[]; problems: { line: number; problem: string }[] } | string {
  const [header = [], ...rows] = parseCsv(text);
  const names = header.map((h) => h.trim());
  if (!names.length || !names.some(Boolean)) return "the file is empty";
  const known = ["agent_harness", "agent_name", "model", "provider", "effort", "speed", "started_at", "ended_at", "tokens.input", "tokens.output", "tokens.cache_write", "tokens.cache_read", "reported_cost", "summary", "ticket", "project", "session_id", "signals.branch", "signals.working_directory", "signals.repository", "signals.pull_request", "signals.subagent_type"];
  for (const name of names) if (!known.includes(name) && !name.startsWith("tags.")) return `column "${name}" is not a field of the token entry schema; tags go in columns named tags.<name>`;
  for (const required of ["agent_harness", "model", "started_at", "ended_at"]) if (!names.includes(required)) return `the file has no ${required} column`;
  const problems: { line: number; problem: string }[] = [];
  const read: { line: number; entry: NewTokenEntry }[] = [];
  rows.forEach((cells, index) => {
    const line = index + 2;
    const get = (name: string) => (cells[names.indexOf(name)] ?? "").trim();
    const missing = ["agent_harness", "model", "started_at", "ended_at"].find((name) => !get(name));
    if (missing) return problems.push({ line, problem: `${missing} is empty` });
    if (Number.isNaN(Date.parse(get("started_at"))) || Number.isNaN(Date.parse(get("ended_at")))) return problems.push({ line, problem: "started_at and ended_at must be RFC 3339 times, such as 2026-09-18T09:12:00Z" });
    const count = (name: string) => Number(get(name) || "0");
    const tokens = { input: count("tokens.input"), output: count("tokens.output"), cache_write: count("tokens.cache_write"), cache_read: count("tokens.cache_read") };
    if (Object.values(tokens).some((n) => !Number.isInteger(n) || n < 0)) return problems.push({ line, problem: "token counts must be whole numbers of 0 or more" });
    const cost = get("reported_cost");
    if (cost && !/^\d+(\.\d{1,9})?$/.test(cost)) return problems.push({ line, problem: "reported_cost must be a decimal number such as 0.42" });
    const tags = Object.fromEntries(names.flatMap((name, i) => (name.startsWith("tags.") && (cells[i] ?? "").trim() ? [[name.slice(5), (cells[i] ?? "").trim()]] : [])));
    read.push({ line, entry: { agent_harness: get("agent_harness"), agent_name: get("agent_name") || null, model: get("model"), started_at: get("started_at"), ended_at: get("ended_at"), tokens, reported_cost: cost || null, summary: get("summary") || null, ticket: get("ticket") || null, project: get("project") || null, tags } });
    return undefined;
  });
  return { rows: read, problems };
}

/** Stores a token entry the way the service does: tags not on the list are left out. */
function addEntry(db: Db, me: Person, body: NewTokenEntry): StoredEntry {
  const sent = body.tags ?? {};
  const dropped = Object.keys(sent).filter((name) => !db.tagNames.includes(name) || ["person", "ticket", "project"].includes(name));
  const extra = Object.fromEntries(Object.entries(sent).filter(([name]) => !dropped.includes(name)));
  const reported = body.reported_cost ? Math.round(Number(body.reported_cost) * 1e9) : null;
  const record: EntryRecord = {
    id: nextId(db, "te"),
    period: body.started_at.slice(0, 7),
    agent_harness: body.agent_harness,
    agent_name: body.agent_name ?? null,
    model: body.model,
    effort: body.effort ?? null,
    started_at: new Date(body.started_at).toISOString(),
    ended_at: new Date(body.ended_at).toISOString(),
    tokens: body.tokens,
    list: reported ?? Math.round((body.tokens.input * 3 + body.tokens.output * 15 + body.tokens.cache_write * 3.75 + body.tokens.cache_read * 0.3) * 1000),
    billed: null,
    summary: body.summary ?? "",
    agent_manager: me.id,
    automatic: false,
    status: "draft",
    reject_reason: null,
    placed_by: body.placed_by === "hand" ? "by hand" : body.ticket || body.project ? "by hand" : "none",
    candidates: [],
    splits: [{ share: 10000, tags: tagsFor(db, me.id, body.ticket ?? body.project ?? "", extra) }],
    late: false,
    provider: body.provider ?? (body.agent_harness.toLowerCase().includes("codex") ? "openai" : "anthropic"),
  };
  db.entries.push(record);
  const [split] = toEntry(db, record).splits;
  return { id: record.id, status: "draft", currency: cur(db), cost_kind: split?.cost_kind ?? "organization_overhead", dropped_tags: dropped, list_value: (Number(record.list) / 1e9).toFixed(2), local_id: body.local_id ?? null };
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

const now = () => `${TODAY}T12:00:00Z`;

function organizationOf(db: Db, name: string, email: string, founderName: string): Db {
  // A new organization on Solo: one person who holds every role, no data yet.
  const founder: Person = {
    id: "f0d0e000-0000-4000-8000-000000000001",
    // Optional at sign-up; without it the start page asks once.
    name: founderName,
    email,
    role: "controller",
    tags: {},
    filters: [],
    status: "invited",
    invited_at: now(),
    joined_at: null,
  };
  return {
    ...db,
    organization: { ...db.organization, id: "0e000000-0000-4000-8000-000000000002", name, slug: name.toLowerCase().replace(/[^a-z0-9]+/g, "-"), cost_source: null, approval: false, sign_in_domains: [], created_at: now() },
    billing: { enabled: true, plan: "solo", status: null, interval: null, current_period_end: null, cancel_at_period_end: false },
    people: [founder],
    invites: [],
    projects: [],
    tickets: [],
    cadences: [],
    entries: [],
    bills: [],
    connections: [],
    budgets: [],
    reports: [],
    adjustments: [],
    tokens: [],
    ticketSystems: [],
    tagNames: ["person", "ticket", "project"],
    rules: { rules: defaultRules(), several_keys: "most_often", summary_match: true },
    personalRules: {},
    periods: [{ id: TODAY.slice(0, 7), status: "open", closed_at: null, closed_by: null, tag_map: null }],
    pendingLink: { token: `link_${founder.id}`, email },
    session: null,
  };
}

function billing(db: Db) {
  const members = db.people.filter((p) => p.status !== "removed").length;
  const b = db.billing;
  return { plan: b.plan, status: b.status, interval: b.interval, current_period_end: b.current_period_end, cancel_at_period_end: b.cancel_at_period_end, members, member_limit: b.plan === "team" ? 150 : 1 };
}

function tagValues(db: Db, name: string) {
  const counts = new Map<string, number>();
  const count = (tags: Tags) => {
    const value = tags[name];
    if (value) counts.set(value, (counts.get(value) ?? 0) + 1);
  };
  for (const entry of db.entries) for (const split of entry.splits) count(split.tags);
  for (const record of [...db.people, ...db.projects, ...db.tickets]) count(record.tags);
  return [...counts.entries()].sort((a, b) => b[1] - a[1]).map(([value, records]) => ({ value, records }));
}

function ticketView(t: Db["tickets"][number]) {
  return toTicket(t);
}

export const handlers = [
  // Sign-in, sign-up and the session
  http.post(`${API}/sign-in`, async ({ request }) => {
    const db = load();
    const { email } = (await request.json()) as { email: string };
    const person = db.people.find((p) => p.email.toLowerCase() === email.trim().toLowerCase() && p.status !== "removed");
    // The service answers the same either way, so nobody can probe for members.
    db.pendingLink = person ? { token: `link_${person.id}`, email: person.email } : null;
    save(db);
    return HttpResponse.json({ poll_token: null }, { status: 202 });
  }),
  http.post(`${API}/sign-up`, async ({ request }) => {
    const body = (await request.json()) as SignUpRequest;
    if (!body.organization_name.trim()) return error(400, "bad_request", "Give the organization a name.");
    save(organizationOf(createDb(), body.organization_name.trim(), body.email.trim(), body.name?.trim() ?? ""));
    return HttpResponse.json({ poll_token: null }, { status: 202 });
  }),
  // The button on the link page: signs the browser in, then 303 to the start page.
  http.post(`${API}/sign-in/link`, async ({ request }) => {
    const db = load();
    const token = new URLSearchParams(await request.text()).get("token") ?? "";
    const person = db.people.find((p) => `link_${p.id}` === token);
    if (!person) return new HttpResponse("This sign-in link has expired. Ask for a new one.", { status: 410, headers: { "content-type": "text/html" } });
    db.session = person.id;
    db.pendingLink = null;
    if (person.status === "invited") Object.assign(person, { status: "joined", joined_at: now() });
    save(db);
    return new HttpResponse(null, { status: 303, headers: { location: "/" } });
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
  http.get(`${API}/health`, () => HttpResponse.json({ status: "ok", database: "ok", version: "1.0.0", edition: "open" })),
  // Connecting a computer: in the mocks every well-formed code is waiting.
  http.get(`${API}/__mock/last-link`, () => HttpResponse.json(load().pendingLink)),
  http.post(`${API}/__mock/reset`, () => {
    resetMockDb();
    return new HttpResponse(null, { status: 204 });
  }),
  http.get(`${API}/me`, member(({ db, me }) => HttpResponse.json({ credential: "session", person: me, organization: { id: db.organization.id, name: db.organization.name, slug: db.organization.slug } }))),

  // The organization, its plan and settings
  http.get(`${API}/organization`, member(({ db }) => HttpResponse.json(db.organization))),
  http.patch(`${API}/organization`, member(async ({ db, request }) => {
    const change = (await request.json()) as Partial<Db["organization"]>;
    // A missing field keeps its value; null clears it.
    for (const [key, value] of Object.entries(change)) {
      if (["name", "approval", "cost_source", "reporting_currency", "sign_in_domains"].includes(key)) Object.assign(db.organization, { [key]: value });
    }
    return HttpResponse.json(db.organization);
  }, CONTROLLER)),
  http.get(`${API}/billing`, member(({ db }) => (db.billing.enabled ? HttpResponse.json(billing(db)) : error(404, "not_found", "Billing is off on this instance.")))),
  http.post(`${API}/billing/checkout`, member(async ({ db, request }) => {
    if (!db.billing.enabled) return error(404, "not_found", "Billing is off on this instance.");
    if (db.billing.plan === "team") return error(409, "conflict", "This organization is on the Team plan already.");
    const { country } = (await request.json()) as { interval: string; country: string };
    if (!/^[A-Z]{2}$/.test(country)) return error(400, "bad_request", `"${country}" is not a country code; use two letters such as AT or US.`);
    return HttpResponse.json({ url: "https://checkout.stripe.com/c/pay/mock" });
  }, CONTROLLER)),
  http.post(`${API}/billing/portal`, member(({ db }) => {
    if (!db.billing.enabled) return error(404, "not_found", "Billing is off on this instance.");
    if (db.billing.plan !== "team") return error(409, "conflict", "There is no subscription to manage yet.");
    return HttpResponse.json({ url: "https://billing.stripe.com/p/session/mock" });
  }, CONTROLLER)),
  http.get(`${API}/cadences`, member(({ db }) => HttpResponse.json(db.cadences))),
  http.put(`${API}/cadences`, member(async ({ db, me, request }) => {
    const body = (await request.json()) as { kind: CadenceView["kind"]; every_days?: number | null; project?: string | null };
    if (body.kind === "custom" && !body.every_days) return error(400, "bad_request", "Give the number of days.");
    const project = body.project ?? null;
    const existing = db.cadences.find((c) => (c.project ?? null) === project);
    const cadence: CadenceView = { id: existing?.id ?? `c0de0000-0000-4000-8000-${String(db.nextId++).padStart(12, "0")}`, kind: body.kind, every_days: body.every_days ?? null, project, starts_on: TODAY, set_at: now(), set_by: me.email, current: { starts_on: "2026-10-12", ends_on: "2026-10-18" } };
    db.cadences = [...db.cadences.filter((c) => c !== existing), cadence];
    return HttpResponse.json(cadence);
  }, PM)),
  http.delete(`${API}/cadences/:id`, member(({ db, params }) => {
    db.cadences = db.cadences.filter((c) => c.id !== params.id);
    return new HttpResponse(null, { status: 204 });
  }, PM)),

  // People
  http.get(`${API}/members`, member(({ db }) => HttpResponse.json(db.people))),
  http.post(`${API}/members`, member(async ({ db, request }) => {
    const body = (await request.json()) as { email: string; name?: string; role?: Role; tags?: Tags };
    const active = db.people.filter((p) => p.status !== "removed").length;
    if (db.billing.enabled && db.billing.plan === "solo") return error(409, "conflict", "Solo is for one person. Choose the Team plan to invite people.");
    if (active >= 150) return error(409, "conflict", "The Team plan stops at 150 members.");
    if (db.people.some((p) => p.email.toLowerCase() === body.email.toLowerCase() && p.status !== "removed")) return error(409, "conflict", "Someone with this email is already a member.");
    const person: Person = { id: `p0000000-0000-4000-8000-${String(db.nextId++).padStart(12, "0")}`, name: body.name?.trim() || (body.email.split("@")[0] ?? body.email), email: body.email, role: body.role ?? "agent_manager", tags: body.tags ?? {}, filters: [], status: "invited", invited_at: now(), joined_at: null };
    db.people.push(person);
    const code = `${db.organization.slug}-${String(db.nextId++).slice(-4)}`;
    db.invites.push({ code, person: person.id });
    return HttpResponse.json({ member: person, invite_url: `${new URL(request.url).origin}/i/${code}` }, { status: 201 });
  }, CONTROLLER)),
  http.patch(`${API}/members/:id`, member(async ({ db, params, request }) => {
    const person = db.people.find((p) => p.id === params.id);
    if (!person) return error(404, "not_found", "No such member.");
    const change = (await request.json()) as { name?: string | null; role?: Role | null; tags?: Tags | null };
    const controllers = db.people.filter((p) => p.role === "controller" && p.status !== "removed");
    if (change.role && change.role !== "controller" && person.role === "controller" && controllers.length === 1) return error(409, "conflict", "The last controller cannot change role. Make someone else a controller first.");
    if (change.name) person.name = change.name;
    if (change.role) person.role = change.role;
    if (change.tags !== undefined) person.tags = change.tags ?? {};
    if (person.role !== "people_manager") person.filters = [];
    return HttpResponse.json(person);
  }, CONTROLLER)),
  http.put(`${API}/members/:id/filters`, member(async ({ db, params, request }) => {
    const person = db.people.find((p) => p.id === params.id);
    if (!person) return error(404, "not_found", "No such member.");
    person.filters = (await request.json()) as Tags[];
    return HttpResponse.json(person);
  }, CONTROLLER)),
  http.post(`${API}/members/:id/invite`, member(({ db, params, request }) => {
    const person = db.people.find((p) => p.id === params.id);
    if (!person) return error(404, "not_found", "No such member.");
    if (person.status !== "invited") return error(409, "conflict", "They have joined already.");
    const code = db.invites.find((i) => i.person === person.id)?.code ?? `${db.organization.slug}-${String(db.nextId++).slice(-4)}`;
    return HttpResponse.json({ member: person, invite_url: `${new URL(request.url).origin}/i/${code}` });
  }, CONTROLLER)),
  http.delete(`${API}/members/:id`, member(({ db, params }) => {
    const person = db.people.find((p) => p.id === params.id);
    if (!person) return error(404, "not_found", "No such member.");
    if (person.role === "controller" && db.people.filter((p) => p.role === "controller" && p.status !== "removed").length === 1) return error(409, "conflict", "The last controller cannot be removed.");
    person.status = "removed";
    return new HttpResponse(null, { status: 204 });
  }, CONTROLLER)),

  // Tags and the tag map
  http.get(`${API}/tags`, member(({ db }) => HttpResponse.json(db.tagNames.map((name) => ({ name, built_in: ["person", "ticket", "project"].includes(name) }))))),
  http.post(`${API}/tags`, member(async ({ db, request }) => {
    const { name } = (await request.json()) as { name: string };
    const clean = name.trim().toLowerCase();
    if (!/^[a-z][a-z0-9_-]*$/.test(clean)) return error(400, "bad_request", "A tag name is lowercase letters, digits, - and _.");
    if (db.tagNames.includes(clean)) return error(409, "conflict", "This tag name is already on the list.");
    db.tagNames.push(clean);
    return HttpResponse.json({ name: clean, built_in: false }, { status: 201 });
  }, CONTROLLER)),
  http.delete(`${API}/tags/:name`, member(({ db, params }) => {
    if (["person", "ticket", "project"].includes(String(params.name))) return error(409, "conflict", "Built-in tags cannot be removed.");
    db.tagNames = db.tagNames.filter((n) => n !== params.name);
    // As the service: rules stop setting it; a rule left setting nothing stays, so its owner sees it.
    for (const rule of allRules(db)) {
      if (typeof rule.then === "object" && "set_tags" in rule.then) rule.then.set_tags = Object.fromEntries(Object.entries(rule.then.set_tags).filter(([name]) => name !== params.name));
    }
    return new HttpResponse(null, { status: 204 });
  }, CONTROLLER)),
  http.get(`${API}/tags/:name/values`, member(({ db, params }) => HttpResponse.json(tagValues(db, String(params.name))))),
  http.post(`${API}/tags/:name/merge`, member(async ({ db, params, request }) => {
    const { from, into } = (await request.json()) as { from: string[]; into: string };
    const name = String(params.name);
    if (["person", "ticket", "project"].includes(name)) return error(400, "bad_request", `"${name}" is built in: its values are members, projects and tickets, which cannot be merged.`);
    if (!into.trim() || into.length > 200 || /[\r\n]/.test(into)) return error(400, "bad_request", "The merged value must have 1 to 200 characters and no line breaks.");
    // As the service: people, projects, tickets, people managers' filters, rules and token entries.
    const rename = (tags: Tags) => {
      const value = tags[name];
      if (value && from.includes(value)) tags[name] = into;
    };
    for (const entry of db.entries) {
      if (isClosed(db, entry.period)) continue;
      for (const split of entry.splits) rename(split.tags);
    }
    for (const record of [...db.people, ...db.projects, ...db.tickets]) rename(record.tags);
    for (const person of db.people) person.filters.forEach(rename);
    for (const rule of allRules(db)) if (typeof rule.then === "object" && "set_tags" in rule.then) rename(rule.then.set_tags);
    return HttpResponse.json(tagValues(db, name));
  }, CONTROLLER)),
  http.get(`${API}/tag-map`, member(({ db }) => HttpResponse.json(db.tagMap))),
  http.put(`${API}/tag-map`, member(async ({ db, request }) => {
    const change = (await request.json()) as { cost_center: string; ladder: string[] };
    db.tagMap = { ...change, version: db.tagMap.version + 1, created_at: now() };
    return HttpResponse.json(db.tagMap);
  }, CONTROLLER)),

  // Projects, clients and tickets
  http.get(`${API}/projects`, member(({ db, url }) => HttpResponse.json(db.projects.filter((p) => !p.archived || url.searchParams.get("archived") === "true")))),
  http.post(`${API}/projects`, member(async ({ db, request }) => {
    const body = (await request.json()) as { name: string; ticket_key_pattern?: string | null; repositories?: string[]; tags?: Tags };
    if (!/^[a-z0-9][a-z0-9-]*$/.test(body.name)) return error(400, "bad_request", "A project name is lowercase letters, digits and -, such as storefront.");
    if (db.projects.some((p) => p.name === body.name)) return error(409, "conflict", "A project with this name exists.");
    const project = { id: `0b000000-0000-4000-8000-${String(db.nextId++).padStart(12, "0")}`, name: body.name, ticket_key_pattern: body.ticket_key_pattern ?? null, repositories: body.repositories ?? [], tags: body.tags ?? {}, archived: false, created_at: now() };
    db.projects.push(project);
    return HttpResponse.json(project, { status: 201 });
  }, PM)),
  http.patch(`${API}/projects/:id`, member(async ({ db, params, request }) => {
    const project = db.projects.find((p) => p.id === params.id);
    if (!project) return error(404, "not_found", "No such project.");
    const change = (await request.json()) as { name?: string | null; ticket_key_pattern?: string | null; repositories?: string[] | null; tags?: Tags | null };
    if (change.name) project.name = change.name;
    if (change.ticket_key_pattern !== undefined) project.ticket_key_pattern = change.ticket_key_pattern;
    if (change.repositories !== undefined) project.repositories = change.repositories ?? [];
    if (change.tags !== undefined) project.tags = change.tags ?? {};
    return HttpResponse.json(project);
  }, PM)),
  http.delete(`${API}/projects/:id`, member(({ db, params }) => {
    const project = db.projects.find((p) => p.id === params.id);
    if (!project) return error(404, "not_found", "No such project.");
    Object.assign(project, { archived: true, repositories: [] });
    return new HttpResponse(null, { status: 204 });
  }, PM)),
  http.get(`${API}/clients`, member(({ db }) => {
    const clients = new Map<string, string[]>();
    for (const p of db.projects) if (p.tags.client && !p.archived) clients.set(p.tags.client, [...(clients.get(p.tags.client) ?? []), p.name]);
    return HttpResponse.json([...clients.entries()].map(([name, projects]) => ({ name, projects })));
  })),
  http.get(`${API}/tickets`, member(({ db, url }) => {
    const project = url.searchParams.get("project_id");
    const parent = url.searchParams.get("parent_id");
    const q = url.searchParams.get("q")?.toLowerCase();
    return HttpResponse.json(
      db.tickets
        .filter((t) => (!project || t.project_id === project) && (!parent || t.parent_id === parent))
        .filter((t) => !q || t.key.toLowerCase().includes(q) || (t.title ?? "").toLowerCase().includes(q))
        .map(ticketView),
    );
  })),
  http.post(`${API}/tickets`, member(async ({ db, request }) => {
    const body = (await request.json()) as { key: string; title?: string | null; description?: string | null; parent_id?: string | null; assignee_id?: string | null; project_id?: string | null; tags?: Tags };
    if (!body.key.trim()) return error(400, "bad_request", "Give the ticket a key.");
    if (db.tickets.some((t) => t.key === body.key)) return error(409, "conflict", "A ticket with this key exists.");
    const ticket = { id: `71c00000-0000-4000-8000-${String(db.nextId++).padStart(12, "0")}`, key: body.key.trim(), title: body.title ?? null, description: body.description ?? null, parent_id: body.parent_id ?? null, assignee_id: body.assignee_id ?? null, project_id: body.project_id ?? null, tags: body.tags ?? {}, source: "manual" as const, updated_at: now(), open: true };
    db.tickets.push(ticket);
    return HttpResponse.json(ticketView(ticket), { status: 201 });
  })),

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
    const body = (await request.json()) as NewTokenEntry | NewTokenEntry[];
    const stored = Array.isArray(body) ? body.map((entry) => addEntry(db, me, entry)) : addEntry(db, me, body);
    return HttpResponse.json(stored, { status: 201 });
  })),
  http.post(`${API}/token-entries/csv/preview`, member(async ({ request }) => {
    const read = csvEntries(await request.text());
    if (typeof read === "string") return error(400, "bad_request", read);
    return HttpResponse.json({ ready: read.rows.length, problems: read.problems });
  })),
  http.post(`${API}/token-entries/csv`, member(async ({ db, me, request }) => {
    const read = csvEntries(await request.text());
    if (typeof read === "string") return error(400, "bad_request", read);
    const [first] = read.problems;
    if (first) return error(400, "bad_request", `Line ${String(first.line)}: ${first.problem}. Nothing was saved.`);
    return HttpResponse.json({ token_entries: read.rows.map((row) => addEntry(db, me, row.entry)) }, { status: 201 });
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
    if (!db.forecasts) return error(404, "off", "Token forecasts are switched off.");
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
  http.get(`${API}/organization/rules`, member(({ db }) => HttpResponse.json(db.rules))),
  http.put(`${API}/organization/rules`, member(async ({ db, request }) => {
    const change = (await request.json()) as OrganizationRulesChange;
    if (change.rules) {
      const problem = checkRules(db, change.rules);
      if (problem) return error(400, "bad_request", problem);
      db.rules.rules = change.rules;
    }
    if (change.several_keys) db.rules.several_keys = change.several_keys;
    if (typeof change.summary_match === "boolean") db.rules.summary_match = change.summary_match;
    return HttpResponse.json(db.rules);
  }, CONTROLLER)),
  http.get(`${API}/me/rules`, member(({ db, me }) => HttpResponse.json({ rules: db.personalRules[me.id] ?? [] }))),
  http.put(`${API}/me/rules`, member(async ({ db, me, request }) => {
    const { rules } = (await request.json()) as { rules: Rule[] };
    const problem = checkRules(db, rules);
    if (problem) return error(400, "bad_request", problem);
    db.personalRules[me.id] = rules;
    return HttpResponse.json({ rules });
  })),

  // Settings
  http.get(`${API}/api-tokens`, member(({ db, me }) => HttpResponse.json(db.tokens.filter((t) => me.role === "controller" || t.person_id === me.id)))),
  http.post(`${API}/api-tokens`, member(async ({ db, me, request }) => {
    const { kind, name } = (await request.json()) as { kind: ApiToken["kind"]; name?: string };
    if (kind === "cli") return error(400, "bad_request", "A CLI token comes from tc connect on the computer it is for.");
    if (kind === "organization" && me.role !== "controller") return error(403, "forbidden", "Only a controller creates the organization token.");
    const prefix = ({ service: "tc_svc_", read: "tc_read_", organization: "tc_org_", cli: "tc_cli_" } satisfies Record<ApiToken["kind"], string>)[kind];
    const token = `${prefix}${Math.random().toString(36).slice(2, 10)}${Math.random().toString(36).slice(2, 18)}`;
    const created: ApiToken = { id: `a0ce0000-0000-4000-8000-${String(db.nextId++).padStart(12, "0")}`, kind, name: name?.trim() ?? "", shown_prefix: token.slice(0, prefix.length + 4), person_id: me.id, needs_owner: false, created_at: now(), last_used_at: null };
    db.tokens.push(created);
    return HttpResponse.json({ ...created, token }, { status: 201 });
  })),
  http.delete(`${API}/api-tokens/:id`, member(({ db, params }) => {
    db.tokens = db.tokens.filter((t) => t.id !== params.id);
    return new HttpResponse(null, { status: 204 });
  })),
  http.post(`${API}/api-tokens/:id/hand-over`, member(async ({ db, params, request }) => {
    const token = db.tokens.find((t) => t.id === params.id);
    if (!token) return error(404, "not_found", "No such token.");
    const { person_id } = (await request.json()) as { person_id: string };
    if (token.kind !== "service") return error(400, "bad_request", "Only service tokens move to another person.");
    if (!db.people.some((p) => p.id === person_id && p.status === "joined")) return error(400, "bad_request", "Hand it to someone who has joined.");
    // Only future token entries follow; past ones keep their agent manager.
    Object.assign(token, { person_id, needs_owner: false });
    return new HttpResponse(null, { status: 204 });
  }, CONTROLLER)),
  http.get(`${API}/ticket-systems`, member(({ db }) => HttpResponse.json(db.ticketSystems))),
  http.post(`${API}/ticket-systems`, member(async ({ db, request }) => {
    const body = (await request.json()) as NewTicketSystem;
    const site = body.site.trim().replace(/\/+$/, "");
    if (!siteFits(body.kind, site)) return error(400, "bad_request", `Give ${siteExample[body.kind]}.`);
    const token = body.token.trim();
    if (!token) return error(400, "bad_request", `Add an API token from ${ticketSystemName[body.kind]}.`);
    if (body.kind === "jira" && !body.email?.trim()) return error(400, "bad_request", "Jira Cloud signs in with the token and the email of its Atlassian account; add the email.");
    const system: TicketSystem = { id: crypto.randomUUID(), kind: body.kind, site, key_hint: token.slice(-4), mappings: [], last_sync_at: null, message: null, status: "ok" };
    db.ticketSystems.push(system);
    return HttpResponse.json(system, { status: 201 });
  }, CONTROLLER)),
  http.delete(`${API}/ticket-systems/:id`, member(({ db, params }) => {
    const index = db.ticketSystems.findIndex((s) => s.id === params.id);
    if (index < 0) return error(404, "not_found", "No such ticket system.");
    // Its tickets stay and stop syncing.
    db.ticketSystems.splice(index, 1);
    return new HttpResponse(null, { status: 204 });
  }, CONTROLLER)),
  http.get(`${API}/ticket-systems/:id/projects`, member(({ db, params }) => {
    const system = db.ticketSystems.find((s) => s.id === params.id);
    if (!system) return error(404, "not_found", "No such ticket system.");
    return HttpResponse.json(externalProjects[system.kind]);
  }, CONTROLLER)),
  http.put(`${API}/ticket-systems/:id/mappings`, member(async ({ db, params, request }) => {
    const system = db.ticketSystems.find((s) => s.id === params.id);
    if (!system) return error(404, "not_found", "No such ticket system.");
    const mappings = (await request.json()) as TicketMapping[];
    const seen = new Set<string>();
    for (const m of mappings) {
      if (!m.external.trim()) return error(400, "bad_request", "Each mapping needs the ticket system's project.");
      if (seen.has(m.external)) return error(400, "bad_request", `${m.external} is mapped twice; map each project once.`);
      seen.add(m.external);
      if (!db.projects.some((p) => p.name === m.project)) return error(400, "bad_request", `There is no project ${m.project}.`);
    }
    system.mappings = mappings;
    system.last_sync_at = `${TODAY}T17:05:00Z`;
    return HttpResponse.json(system);
  }, CONTROLLER)),
  http.post(`${API}/ticket-systems/:id/sync`, member(({ db, params }) => {
    const system = db.ticketSystems.find((s) => s.id === params.id);
    if (!system) return error(404, "not_found", "No such ticket system.");
    system.last_sync_at = `${TODAY}T17:05:00Z`;
    return HttpResponse.json(system, { status: 202 });
  }, CONTROLLER)),
  http.get(`${API}/exports/raw`, member(({ db }) => {
    const files = { "organization.json": db.organization, "people.json": db.people, "token_entries.json": db.entries, "bills.json": db.bills, "rules.json": db.rules, "tickets.json": db.tickets, "periods.json": db.periods, "tags.json": { names: db.tagNames, map: db.tagMap } };
    return new HttpResponse(JSON.stringify(files, null, 2), { headers: { "content-type": "application/json" } });
  }, CONTROLLER)),
  ...customerRelationsHandlers({ API, member, fail: error, now }),
];

/** Money helper for tests. */
export function dollars(n: number) {
  return n * NANOS;
}
