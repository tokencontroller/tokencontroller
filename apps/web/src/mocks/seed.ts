// The example organization the docs use, Kestrel Digital, for development,
// tests and screenshots while the service's endpoints are being built. People,
// projects and tickets are the ones the service's seed command loads (same
// IDs, emails and titles), so screenshots match staging. Token entries, bills
// and the rest are generated from a fixed random seed, so every run and every
// screenshot shows the same numbers. "Today" is fixed too.

import type {
  ApiToken,
  BillConnectionView,
  BillCostSource,
  BillSource,
  CadenceView,
  Organization,
  Person,
  Project,
  OrganizationRules,
  Rule,
  Tags,
  Ticket,
  TicketSystem,
  TokenCounts,
  TokenEntryStatus,
} from "@/api/contract";

export const TODAY = "2026-10-12";
export const CURRENCY = "USD";
const NANOS = 1_000_000_000;

export interface EntryRecord {
  id: string;
  period: string;
  agent_harness: string;
  agent_name: string | null;
  model: string;
  effort: string | null;
  started_at: string;
  ended_at: string;
  tokens: TokenCounts;
  /** List value in nano-units, or null for an unknown model. */
  list: number | null;
  billed: number | null;
  summary: string;
  /** The person id of the agent manager. */
  agent_manager: string | null;
  automatic: boolean;
  status: TokenEntryStatus;
  reject_reason: string | null;
  placed_by: string;
  candidates: string[];
  /** `unknown_ticket`: a ticket key that matched a pattern but that the ticket sync did not find. */
  splits: { share: number; tags: Tags; unknown_ticket?: string | null }[];
  late: boolean;
  /** Which bill this entry's cost appears on. */
  provider: string;
}

/** One figure a provider gave for a bill, in nano-units of its currency. */
export interface BillRevisionRecord {
  revision: number;
  currency: string;
  gross: number;
  discount: number;
  credit: number;
  net: number;
  /** To the reporting currency in nano-units (0.9214 is 921_400_000); null in the reporting currency. */
  exchange_rate: number | null;
  exchange_rate_given: boolean;
  source: BillSource;
  recorded_at: string;
  /** What a cloud's export was summed from, as the service keeps it. */
  lines?: Record<string, string | null>[];
  /** False while a pulled bill can still change; the close waits for it. Left out means final. */
  final?: boolean;
}

/** A provider's bill for one period and cost source, with every revision. */
export interface BillRecord {
  id: string;
  period: string;
  /** Who charged, as token entries name it, such as `anthropic`. */
  provider: string;
  cost_source: BillCostSource;
  /** Oldest first; the last is the latest. */
  revisions: BillRevisionRecord[];
}

/** A token budget as the service stores it, the amount in nano-units of the reporting currency. */
export interface BudgetRecord {
  id: string;
  name: string | null;
  filter: Tags;
  /** The one period, or the first of every month; null for all time. */
  period: string | null;
  every_month: boolean;
  amount: number;
  token_forecast_id: string | null;
  /** The person id of who set it. */
  set_by: string;
  set_at: string;
}

/** One past ticket a token forecast matched, as stored when it was made. */
export interface ForecastMatchRecord {
  ticket: string;
  title: string | null;
  confidence: number;
  /** Its actual cost over all time when the forecast was made, in nano-units. */
  actual: number;
  token_entries: number;
  unknown_model_entries: number;
}

export interface ForecastRecord {
  id: string;
  ticket: string;
  currency: string;
  /** The median of the matches with a known cost, in nano-units; null when none has one. */
  proposed: number | null;
  matches: ForecastMatchRecord[];
  model: string;
  created_by: string;
  created_at: string;
}

/** A saved report: a tag filter with a grouping. */
export interface SavedReportRecord {
  id: string;
  name: string;
  filter: Tags;
  by: string;
  /** The person id of who saved it. */
  created_by: string | null;
  created_at: string;
}

export interface AdjustmentRecord {
  id: string;
  period: string;
  /** Plus or minus, in nano-units of the reporting currency. */
  amount: number;
  /** The cost object and any other tags; none puts it on the organization. */
  tags: Tags;
  reason: string;
  /** Posted on its own for a token sheet that came in after the close. */
  late: boolean;
  /** The person id; null when posted for a late token sheet. */
  posted_by: string | null;
  posted_at: string;
}

export interface PeriodRecord {
  id: string;
  status: "open" | "closed";
  closed_at: string | null;
  closed_by: string | null;
  tag_map: { cost_center: string; ladder: string[] } | null;
  /** A closed period keeps the tag map version, approval and the bill revisions it closed with. */
  tag_map_version?: number;
  approval?: boolean;
  bills?: { bill: string; revision: number }[];
}

/** A link that shows a closed period's cost sheet without signing in. */
export interface CostSheetLinkRecord {
  id: string;
  period: string;
  secret: string;
  /** The person id. */
  created_by: string;
  created_at: string;
  revoked_at: string | null;
}

export interface TicketRecord extends Ticket {
  /** Whether work still goes on it; closed tickets only have past cost. */
  open: boolean;
}

export interface Db {
  organization: Organization;
  /** Token forecasts: on unless a controller switched them off; `model` null is an instance without an embeddings model. */
  forecasts: { enabled: boolean; model: string | null; made: ForecastRecord[] };
  session: string | null;
  /** The last magic link "sent", so the mock sign-in page can offer it. */
  pendingLink: { token: string; email: string } | null;
  people: Person[];
  /** Invite codes, as in `/i/kestrel-4f2a`. */
  invites: { code: string; person: string }[];
  tagNames: string[];
  tagMap: { cost_center: string; ladder: string[]; version: number; created_at: string };
  projects: Project[];
  tickets: TicketRecord[];
  cadences: CadenceView[];
  entries: EntryRecord[];
  bills: BillRecord[];
  connections: BillConnectionView[];
  periods: PeriodRecord[];
  adjustments: AdjustmentRecord[];
  links: CostSheetLinkRecord[];
  budgets: BudgetRecord[];
  reports: SavedReportRecord[];
  rules: OrganizationRules;
  /** Each person's own rules, by person ID. */
  personalRules: Record<string, Rule[]>;
  tokens: ApiToken[];
  ticketSystems: TicketSystem[];
  nextId: number;
}

/** The fixed IDs of the service's seed data (`seed::ids`). */
const uuid = (group: number, n: number) => `6b657374-0000-4000-800${group}-${n.toString(16).padStart(12, "0")}`;
export const ORGANIZATION_ID = uuid(0, 1);

// A small, fast, seeded random number generator (mulberry32).
function random(seed: number): () => number {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function person(n: number, name: string, email: string, role: Person["role"], tags: Tags, joined = true): Person {
  return {
    id: uuid(1, n),
    name,
    email,
    role,
    tags,
    filters: [],
    status: joined ? "joined" : "invited",
    invited_at: "2026-05-04T09:00:00Z",
    joined_at: joined ? "2026-05-04T09:20:00Z" : null,
  };
}

const people: Person[] = [
  person(1, "Nadia Berger", "n.berger@kestrel.example", "controller", { department: "operations" }),
  { ...person(2, "Priya Shah", "p.shah@kestrel.example", "people_manager", { team: "web", department: "engineering" }), filters: [{ team: "web" }] },
  { ...person(3, "Kai Novak", "k.novak@kestrel.example", "people_manager", { department: "accounts" }), filters: [{ client: "acme" }] },
  person(4, "Mara Ortner", "m.ortner@kestrel.example", "agent_manager", { team: "web", department: "engineering" }),
  person(5, "Jonas Weber", "j.weber@kestrel.example", "agent_manager", { team: "web", department: "engineering" }),
  { ...person(6, "Tom Brandt", "t.brandt@kestrel.example", "people_manager", { team: "mobile", department: "engineering" }), filters: [{ team: "mobile" }] },
  person(7, "Sam Rossi", "s.rossi@kestrel.example", "agent_manager", { team: "mobile", department: "engineering" }),
  person(8, "Lena Hofer", "l.hofer@kestrel.example", "agent_manager", { team: "data", department: "engineering" }),
  person(9, "Ola Nowak", "o.nowak@kestrel.example", "agent_manager", { team: "data", department: "engineering" }, false),
  // Only in the mocks: someone who left, so a service token needs an owner.
  { ...person(10, "Felix Wagner", "f.wagner@kestrel.example", "agent_manager", { team: "web", department: "engineering" }), status: "removed" },
];

/** People by their first name, as the generator below refers to them. */
const who = (first: string): Person => {
  const found = people.find((p) => p.name.toLowerCase().startsWith(`${first} `));
  if (!found) throw new Error(`no ${first} in the seed`);
  return found;
};
export const MARA = who("mara");

/** The rules every organization starts with, as the service's core crate has them. */
export function defaultRules(): Rule[] {
  return [
    { name: "ticket key in branch", when: [], then: { ticket_key_in: "branch" } },
    { name: "ticket key in pull request title", when: [], then: { ticket_key_in: "pull_request_title" } },
    { name: "ticket key in prompts or session title", when: [], then: { ticket_key_in: "prompts_or_session_title" } },
    { name: "repository", when: [], then: "repository_project" },
  ];
}

function project(n: number, name: string, pattern: string | null, tags: Tags, repositories: string[]): Project {
  return { id: uuid(2, n), name, ticket_key_pattern: pattern, tags, repositories, archived: false, created_at: "2026-05-04T10:00:00Z" };
}

const projects: Project[] = [
  project(1, "storefront", "^KD-\\d+", { client: "acme" }, ["kestrel/storefront"]),
  project(2, "claims-portal", "^CP-\\d+", { client: "globex" }, ["kestrel/claims-portal"]),
  project(3, "intranet", "^INT-\\d+", {}, ["kestrel/intranet", "kestrel/sandbox"]),
  project(4, "support", null, {}, []),
];

const projectId = (name: string) => projects.find((p) => p.name === name)?.id ?? null;
/** Ticket ids follow the service's seed: KD-n is n, CP-n is 1000 + n, INT-n is 2000 + n. */
const ticketId = (key: string) => {
  const [prefix = "", number = "0"] = key.split("-");
  return uuid(3, Number(number) + ({ KD: 0, CP: 1000, INT: 2000 }[prefix] ?? 3000));
};

function ticket(key: string, projectName: string, title: string, parent: string | null, assignee: string | null, open = true, description: string | null = null): TicketRecord {
  return {
    id: ticketId(key),
    key,
    title,
    description,
    parent_id: parent ? ticketId(parent) : null,
    assignee_id: assignee ? who(assignee).id : null,
    project_id: projectId(projectName),
    source: key.startsWith("INT") ? "manual" : "jira",
    tags: {},
    updated_at: "2026-10-12T06:00:00Z",
    open,
  };
}

const tickets: TicketRecord[] = [
  ticket("KD-120", "storefront", "Cart page", null, "mara", false),
  ticket("KD-131", "storefront", "Search filters", null, "jonas", false),
  ticket("KD-150", "storefront", "Checkout address form", null, "mara", false, "Shoppers type their address once; postcode lookup fills the town."),
  ticket("KD-200", "storefront", "Checkout rebuild", null, "priya"),
  ticket("KD-214", "storefront", "Fix checkout redirect loop", "KD-200", "mara", true, "Payment step sends the shopper back to the cart after a declined card."),
  ticket("KD-231", "storefront", "Saved addresses at checkout", "KD-200", "jonas", true, "Shoppers pick a saved address instead of typing it again."),
  ticket("KD-240", "storefront", "Address autocomplete", "KD-200", "jonas"),
  ticket("KD-260", "storefront", "Retry failed card payments", null, "mara", true, "Retry a declined card payment once before sending the shopper back."),
  ticket("CP-4", "claims-portal", "Claims list filters", null, "lena", false),
  ticket("CP-9", "claims-portal", "Claim status page", null, "sam", false),
  ticket("CP-12", "claims-portal", "Claim upload form", null, "sam"),
  ticket("CP-15", "claims-portal", "Claim status emails", null, "lena"),
  ticket("CP-18", "claims-portal", "Upload retry", "CP-12", "sam"),
  ticket("CP-20", "claims-portal", "Pre-release pen test", null, "sam"),
  ticket("INT-3", "intranet", "Upgrade the build tooling", null, "jonas"),
  ticket("INT-8", "intranet", "Wiki search", null, "lena"),
];

interface Work {
  person: string;
  harness: string;
  model: string;
  provider: string;
  /** Ticket keys or project names this person works on, with weights. */
  work: [string, number][];
  sessionsPerDay: number;
}

const workers: Work[] = [
  { person: "mara", harness: "Claude Code", model: "claude-opus-5", provider: "anthropic", work: [["KD-214", 4], ["KD-260", 3], ["KD-150", 2], ["KD-120", 2], ["storefront", 1], ["", 1]], sessionsPerDay: 1.8 },
  { person: "jonas", harness: "Cursor", model: "claude-sonnet-5", provider: "cursor", work: [["KD-231", 4], ["KD-240", 3], ["KD-131", 2], ["storefront", 1], ["", 1]], sessionsPerDay: 1.3 },
  { person: "jonas", harness: "Claude Code", model: "claude-sonnet-5", provider: "anthropic", work: [["KD-231", 2], ["INT-3", 1]], sessionsPerDay: 0.5 },
  { person: "lena", harness: "Gemini CLI", model: "gemini-3-pro", provider: "google", work: [["CP-15", 3], ["CP-4", 2], ["INT-8", 3], ["intranet", 1], ["", 1]], sessionsPerDay: 1.0 },
  { person: "lena", harness: "Claude Code", model: "claude-sonnet-5", provider: "anthropic", work: [["CP-15", 2], ["INT-8", 1]], sessionsPerDay: 0.6 },
  { person: "sam", harness: "Codex", model: "gpt-5-codex", provider: "openai", work: [["CP-12", 3], ["CP-18", 3], ["CP-9", 2], ["claims-portal", 1], ["", 1]], sessionsPerDay: 1.4 },
  { person: "sam", harness: "Copilot CLI", model: "gpt-5", provider: "github", work: [["CP-18", 2], ["CP-20", 1], ["support", 1], ["", 1]], sessionsPerDay: 0.4 },
  { person: "priya", harness: "Claude Code", model: "claude-opus-5", provider: "anthropic", work: [["KD-200", 2], ["INT-3", 2], ["", 1]], sessionsPerDay: 0.4 },
];

const summaries: Record<string, string[]> = {
  "KD-120": ["Built the cart line item list", "Fixed rounding in cart totals"],
  "KD-131": ["Added price and brand filters", "Wrote tests for the filter panel"],
  "KD-150": ["Validated the address form", "Added postcode lookup to checkout"],
  "KD-200": ["Planned the checkout rebuild", "Reviewed checkout architecture notes"],
  "KD-214": ["Fixed checkout redirect loop", "Traced the declined card redirect", "Added a test for the redirect"],
  "KD-231": ["Saved address list at checkout", "Address book API for checkout", "Picked the default address"],
  "KD-240": ["Address autocomplete with the places API", "Debounced the address lookup"],
  "KD-260": ["Retry for declined card payments", "Backoff for the payment retry"],
  "CP-4": ["Filters for the claims list", "Date range filter"],
  "CP-9": ["Claim status timeline", "Status page copy fixes"],
  "CP-12": ["Built the claim upload form", "Checked file types on upload"],
  "CP-15": ["Claim status email templates", "Email when a claim changes status"],
  "CP-18": ["Built upload retry", "Backoff for failed uploads"],
  "CP-20": ["Pre-release pen test"],
  "INT-3": ["Upgraded the build tooling", "Fixed a breaking change in the bundler"],
  "INT-8": ["Indexed wiki pages for search", "Search result snippets"],
  storefront: ["Set up the storefront preview environment", "Read up on the new payment API"],
  "claims-portal": ["Explored the claims API", "Set up the claims dev database"],
  intranet: ["Looked at the intranet build"],
  support: ["Answered a support question about refunds"],
  "": ["Tried a new test runner", "Internal tooling script", "Learning the new framework", "Wrote a shell helper"],
};

const projectOfTicket = new Map(tickets.map((t) => [t.key, projects.find((p) => p.id === t.project_id)?.name ?? ""]));
const projectTags = new Map(projects.map((p) => [p.name, p.tags]));

/**
 * The tags a token entry ends up with: its person (by email, as the service
 * stores it), their tags, the ticket, its project and what they pass on.
 */
export function inheritTags(person: Person | null, target: string, extra: Tags = {}): Tags {
  const tags: Tags = {};
  if (person) {
    tags.person = person.email;
    Object.assign(tags, person.tags);
  }
  const project = projectOfTicket.get(target) ?? (projectTags.has(target) ? target : null);
  if (projectOfTicket.has(target)) tags.ticket = target;
  if (project) {
    tags.project = project;
    Object.assign(tags, projectTags.get(project) ?? {});
  }
  return { ...tags, ...extra };
}

function pick<T>(rand: () => number, items: [T, number][]): T {
  const total = items.reduce((s, [, w]) => s + w, 0);
  let r = rand() * total;
  for (const [item, weight] of items) {
    r -= weight;
    if (r <= 0) return item;
  }
  const last = items[items.length - 1];
  if (!last) throw new Error("pick from an empty list");
  return last[0];
}

function daysOf(period: string): string[] {
  const [y = 2026, m = 1] = period.split("-").map(Number);
  const days: string[] = [];
  for (let d = 1; d <= 31; d++) {
    const date = new Date(Date.UTC(y, m - 1, d));
    if (date.getUTCMonth() !== m - 1) break;
    const iso = date.toISOString().slice(0, 10);
    if (iso >= TODAY) break;
    const weekday = date.getUTCDay();
    if (weekday !== 0 && weekday !== 6) days.push(iso);
  }
  return days;
}

function tokensFor(rand: () => number, cents: number): TokenCounts {
  const scale = cents / 100;
  return {
    input: Math.round(800 + rand() * 1500 * scale),
    output: Math.round(5_000 + 900 * scale * (0.6 + rand())),
    cache_write: Math.round(20_000 + 5_000 * scale * rand()),
    cache_read: Math.round(120_000 * scale * (0.6 + rand())),
  };
}

/** What placed a split, as the service's review list shows it (the default rules' names). */
function placedBy(target: string, harness: string): string {
  if (!target) return "no ticket";
  if (projectOfTicket.has(target)) return harness === "Cursor" || harness === "Gemini CLI" ? "rule: ticket key in prompts or session title" : "rule: ticket key in branch";
  return "rule: repository";
}

export function createDb(): Db {
  const rand = random(20261012);
  const entries: EntryRecord[] = [];
  let n = 0;
  const id = () => `te_${String(++n).padStart(4, "0")}`;

  const periods = ["2026-07", "2026-08", "2026-09", "2026-10"];
  for (const period of periods) {
    for (const day of daysOf(period)) {
      for (const worker of workers) {
        let sessions = Math.floor(worker.sessionsPerDay + rand() * 0.9);
        while (sessions-- > 0) {
          const target = pick(rand, worker.work);
          // Past tickets only get work in July and August.
          const ticketRecord = tickets.find((t) => t.key === target);
          if (ticketRecord && !ticketRecord.open && period > "2026-08") continue;
          if (ticketRecord?.open && ticketRecord.key !== "KD-200" && period < "2026-09" && !["KD-214", "CP-12", "INT-3"].includes(target)) continue;
          const startHour = 8 + Math.floor(rand() * 9);
          const minutes = Math.floor(rand() * 60);
          const length = 15 + Math.floor(rand() * 140);
          const started = new Date(`${day}T${String(startHour).padStart(2, "0")}:${String(minutes).padStart(2, "0")}:00Z`);
          const ended = new Date(started.getTime() + length * 60_000);
          const cents = Math.round(150 + rand() * rand() * 8_000);
          const pool = summaries[target] ?? summaries[""] ?? ["Session"];
          const summary = pool[Math.floor(rand() * pool.length)] ?? "Session";
          const extra: Tags = {};
          if (!target && summary.startsWith("Learning")) extra.purpose = "learning";
          const unknown = worker.harness === "Gemini CLI" && rand() < 0.04;
          entries.push({
            id: id(),
            period,
            agent_harness: worker.harness,
            agent_name: null,
            model: unknown ? "gemini-3-pro-preview-0917" : worker.model,
            effort: worker.harness === "Claude Code" ? pick(rand, [["high", 2], ["medium", 3], [null, 1]] as [string | null, number][]) : null,
            started_at: started.toISOString(),
            ended_at: ended.toISOString(),
            tokens: tokensFor(rand, cents),
            list: unknown ? null : cents * 10_000_000,
            billed: null,
            summary,
            agent_manager: who(worker.person).id,
            automatic: false,
            status: "submitted",
            reject_reason: null,
            placed_by: placedBy(target, worker.harness),
            candidates: [],
            splits: [{ share: 10000, tags: inheritTags(who(worker.person), target, extra) }],
            late: false,
            provider: worker.provider,
          });
        }
      }
      // The CI review bot reviews a pull request most days. Priya set it up.
      if (rand() < 0.8) {
        const target = pick(rand, [["KD-214", 2], ["KD-231", 2], ["KD-240", 1], ["CP-18", 2], ["CP-15", 1]] as [string, number][]);
        if (period < "2026-09" && !["KD-214", "CP-12"].includes(target)) continue;
        const cents = Math.round(180 + rand() * 500);
        const started = new Date(`${day}T${String(9 + Math.floor(rand() * 8)).padStart(2, "0")}:15:00Z`);
        entries.push({
          id: id(),
          period,
          agent_harness: "Claude Code",
          agent_name: "review bot",
          model: "claude-sonnet-5",
          effort: "medium",
          started_at: started.toISOString(),
          ended_at: new Date(started.getTime() + 6 * 60_000).toISOString(),
          tokens: tokensFor(rand, cents),
          list: cents * 10_000_000,
          billed: null,
          summary: `Reviewed pull request for ${target}`,
          agent_manager: who("priya").id,
          automatic: true,
          status: "submitted",
          reject_reason: null,
          placed_by: "rule: ticket key in pull request title",
          candidates: [],
          splits: [{ share: 10000, tags: inheritTags(who("priya"), target, { purpose: "code-review" }) }],
          late: false,
          provider: "anthropic",
        });
      }
    }
  }

  // Closed months are locked. September is approved except a handful.
  let septemberOpen = 0;
  for (const entry of entries) {
    if (entry.period < "2026-09") entry.status = "locked";
    else if (entry.period === "2026-09") {
      entry.status = rand() < 0.94 || septemberOpen >= 7 ? "approved" : "submitted";
      if (entry.status === "submitted") septemberOpen++;
    }
  }

  // Jonas has not submitted his sheet for the last week of September: late.
  for (const entry of entries) {
    if (entry.agent_manager === who("jonas").id && !entry.automatic && entry.started_at >= "2026-09-28") {
      entry.status = "draft";
    }
  }

  // Sam's entry was sent back with a reason.
  const rejected = entries.find((e) => e.agent_manager === who("sam").id && e.period === "2026-10" && e.splits[0]?.tags.ticket === "CP-18");
  if (rejected) {
    rejected.status = "rejected";
    rejected.reject_reason = "This was the upload form itself. Please move it to CP-12.";
  }

  // Mara's branch named KD-299, which the ticket sync never found: her review
  // list shows the token entry as unknown ticket (docs: Ticket keys).
  const unsynced = entries.find((e) => e.agent_manager === MARA.id && e.period === "2026-10" && e.status === "submitted" && Object.keys(e.splits[0]?.tags ?? {}).every((name) => ["person", "team", "department"].includes(name)));
  const unsyncedSplit = unsynced?.splits[0];
  if (unsyncedSplit) unsyncedSplit.unknown_ticket = "KD-299";

  // Mara's drafts for today, as the Review and submit docs page shows them.
  const today: [string, number, string, string, string[]][] = [
    ["09:12", 4187, "KD-214", "rule: ticket key in branch", []],
    ["11:46", 1210, "KD-214", "rule: ticket key in branch", []],
    ["13:05", 8830, "KD-231", "rule: ticket key in pull request title", []],
    ["15:20", 975, "storefront", "rule: repository", []],
    ["16:41", 6038, "", "no ticket", ["KD-231", "KD-240"]],
  ];
  const todaySummaries = ["Fixed checkout redirect loop", "Added a test for the redirect", "Saved address list at checkout", "Read up on the new payment API", "Address lookup edge cases"];
  today.forEach(([time, cents, target, by, candidates], index) => {
    const started = new Date(`${TODAY}T${time}:00Z`);
    entries.push({
      id: id(),
      period: "2026-10",
      agent_harness: "Claude Code",
      agent_name: null,
      model: "claude-opus-5",
      effort: "high",
      started_at: started.toISOString(),
      ended_at: new Date(started.getTime() + 70 * 60_000).toISOString(),
      tokens: tokensFor(rand, cents),
      list: cents * 10_000_000,
      billed: null,
      summary: todaySummaries[index] ?? "Session",
      agent_manager: MARA.id,
      automatic: false,
      status: "draft",
      reject_reason: null,
      placed_by: by,
      candidates,
      splits: [{ share: 10000, tags: inheritTags(MARA, target) }],
      late: false,
      provider: "anthropic",
    });
  });

  // A release-notes bot whose service token belonged to someone who left.
  entries.push({
    id: id(),
    period: "2026-10",
    agent_harness: "Claude Code",
    agent_name: "release-notes bot",
    model: "claude-sonnet-5",
    effort: null,
    started_at: "2026-10-09T17:00:00Z",
    ended_at: "2026-10-09T17:04:00Z",
    tokens: tokensFor(rand, 240),
    list: 240 * 10_000_000,
    billed: null,
    summary: "Wrote release notes for storefront 4.2",
    agent_manager: null,
    automatic: true,
    status: "submitted",
    reject_reason: null,
    placed_by: "rule: repository",
    candidates: [],
    splits: [{ share: 10000, tags: inheritTags(null, "storefront") }],
    late: false,
    provider: "anthropic",
  });

  // Jonas's token sheet for a week of August came in after August closed:
  // its entry stays locked and reaches August as an adjustment, marked late.
  const lateEntry = [...entries].reverse().find((e) => e.period === "2026-08" && e.agent_manager === who("jonas").id && !e.automatic && e.list !== null);
  if (lateEntry) lateEntry.late = true;

  // Bills: token entries plus a small residual, per provider. Discount and
  // credit are zero unless a provider restated the bill.
  const providers: { key: string; cost_source: BillCostSource; source: BillSource; residual: number }[] = [
    { key: "anthropic", cost_source: "usage_at_api_rates", source: "pulled", residual: 0.031 },
    { key: "openai", cost_source: "api_key", source: "pulled", residual: 0.024 },
    { key: "google", cost_source: "api_key", source: "csv", residual: 0.05 },
    { key: "cursor", cost_source: "reseller", source: "csv", residual: 0.0 },
    { key: "github", cost_source: "reseller", source: "csv", residual: 0.04 },
  ];
  const bills: BillRecord[] = [];
  let billNumber = 0;
  for (const period of ["2026-07", "2026-08", "2026-09"]) {
    for (const p of providers) {
      const covered = entries
        .filter((e) => e.period === period && e.provider === p.key)
        .reduce((s, e) => s + (e.list ?? 0), 0);
      // Unknown models are billed too; Google's bill covers them.
      const net = Math.round((covered * (1 + p.residual)) / 10_000_000) * 10_000_000 + (p.key === "google" ? 1_850_000_000 : 0);
      const [y = 2026, m = 1] = period.split("-").map(Number);
      const recorded = new Date(Date.UTC(y, m, p.source === "pulled" ? 3 : 6, 6)).toISOString();
      bills.push({
        id: uuid(5, ++billNumber),
        period,
        provider: p.key,
        cost_source: p.cost_source,
        revisions: [{ revision: 1, currency: CURRENCY, gross: net, discount: 0, credit: 0, net, exchange_rate: null, exchange_rate_given: false, source: p.source, recorded_at: recorded }],
      });
    }
  }
  const billOf = (period: string, provider: string) => bills.find((b) => b.period === period && b.provider === provider);
  // Anthropic restated September once, before the close.
  const september = billOf("2026-09", "anthropic")?.revisions;
  const first = september?.[0];
  if (september && first) {
    september[0] = { ...first, gross: first.gross - 4_120_000_000, net: first.net - 4_120_000_000, recorded_at: "2026-10-03T06:00:00Z" };
    september.push({ ...first, revision: 2, recorded_at: "2026-10-09T06:00:00Z" });
  }
  // GitHub credited one of two pen test runs after August closed. August
  // keeps the figure it closed with; the controller posted an adjustment.
  const august = billOf("2026-08", "github")?.revisions;
  const closedWith = august?.[0];
  if (august && closedWith) {
    august.push({ ...closedWith, revision: 2, credit: 18_400_000_000, net: closedWith.net - 18_400_000_000, recorded_at: "2026-09-14T06:00:00Z" });
  }
  const closedBills = (period: string) => bills.filter((b) => b.period === period).map((b) => ({ bill: b.id, revision: 1 }));

  // Token budgets, as people managers set them: some for every month, one
  // for a single period, and two all-time ones on tickets, one of them
  // accepted from a token forecast made when KD-231 started.
  const kai = who("kai").id;
  const budget = (n: number, name: string | null, filter: Tags, applies: { period: string | null; every_month?: boolean }, nanos: number, by: string, at: string, forecast: string | null = null): BudgetRecord => ({
    id: `7b000000-0000-4000-8000-${String(n).padStart(12, "0")}`,
    name,
    filter,
    period: applies.period,
    every_month: applies.every_month ?? false,
    amount: nanos,
    token_forecast_id: forecast,
    set_by: by,
    set_at: at,
  });
  // What a storefront ticket had cost by the end of August, when KD-231's token forecast was made.
  const before = (e: EntryRecord, key: string) => e.period <= "2026-08" && e.splits.some((split) => split.tags.ticket === key);
  const costBySeptember = (key: string) =>
    entries
      .filter((e) => before(e, key))
      .reduce((sum, e) => sum + e.splits.filter((split) => split.tags.ticket === key).reduce((part, split) => part + Math.round(((e.list ?? 0) * split.share) / 10_000), 0), 0);
  const forecastMatches: ForecastMatchRecord[] = (
    [
      ["KD-150", 81],
      ["KD-200", 64],
      ["KD-214", 52],
      ["KD-120", 38],
    ] as const
  ).map(([key, confidence]) => ({
    ticket: key,
    title: tickets.find((t) => t.key === key)?.title ?? null,
    confidence,
    actual: costBySeptember(key),
    token_entries: entries.filter((e) => before(e, key)).length,
    unknown_model_entries: 0,
  }));
  const costs = forecastMatches.map((match) => match.actual).sort((a, b) => a - b);
  // The median of four: the two middle costs, halved the way the service splits money.
  const proposed = Math.ceil(((costs[1] ?? 0) + (costs[2] ?? 0)) / 2);
  const forecasts: ForecastRecord[] = [
    { id: "7f000000-0000-4000-8000-000000000001", ticket: "KD-231", currency: CURRENCY, proposed, matches: forecastMatches, model: "text-embedding-3-small", created_by: kai, created_at: "2026-09-01T09:20:00Z" },
  ];
  const usd = (dollars: number) => dollars * NANOS;
  const budgets: BudgetRecord[] = [
    budget(1, "Mara, monthly", { person: MARA.email }, { period: "2026-09", every_month: true }, usd(550), who("priya").id, "2026-08-28T10:00:00Z"),
    budget(2, "Team web, monthly", { team: "web" }, { period: "2026-09", every_month: true }, usd(1300), who("priya").id, "2026-08-28T10:05:00Z"),
    budget(3, "Team mobile, monthly", { team: "mobile" }, { period: "2026-09", every_month: true }, usd(700), who("tom").id, "2026-08-29T08:30:00Z"),
    budget(4, "Acme, monthly", { client: "acme" }, { period: "2026-09", every_month: true }, usd(1100), kai, "2026-08-31T14:00:00Z"),
    budget(5, null, { ticket: "KD-231" }, { period: null }, proposed, kai, "2026-09-01T09:25:00Z", "7f000000-0000-4000-8000-000000000001"),
    budget(6, null, { ticket: "KD-214" }, { period: null }, usd(950), kai, "2026-09-02T11:10:00Z"),
    budget(7, "Intranet, October", { project: "intranet" }, { period: "2026-10" }, usd(300), who("nadia").id, "2026-10-01T08:00:00Z"),
  ];

  const nadia = who("nadia").id;

  const db: Db = {
    organization: {
      id: ORGANIZATION_ID,
      name: "Kestrel Digital",
      slug: "kestrel",
      cost_source: "usage_at_api_rates",
      reporting_currency: CURRENCY,
      approval: true,
      sign_in_domains: ["kestrel.example"],
      created_at: "2026-05-04T08:00:00Z",
    },
    forecasts: { enabled: true, model: "text-embedding-3-small", made: forecasts },
    session: null,
    pendingLink: null,
    people,
    invites: [{ code: "kestrel-4f2a", person: who("ola").id }],
    tagNames: ["person", "ticket", "project", "team", "client", "department", "purpose"],
    tagMap: { cost_center: "person", ladder: ["ticket", "project"], version: 1, created_at: "2026-05-04T08:00:00Z" },
    projects,
    tickets,
    cadences: [
      { id: "c0de0000-0000-4000-8000-000000000001", kind: "weekly", every_days: null, project: "storefront", starts_on: "2026-06-01", set_at: "2026-06-01T08:00:00Z", set_by: "p.shah@kestrel.example", current: { starts_on: "2026-10-12", ends_on: "2026-10-18" } },
    ],
    entries,
    bills,
    // Anthropic's Enterprise Analytics API and OpenAI's Costs API, pulled this morning.
    connections: [
      { id: uuid(7, 1), provider: "anthropic_enterprise", key_hint: "…r4Qe", last_pull_at: "2026-10-12T06:00:00Z", status: "ok", message: null },
      { id: uuid(7, 2), provider: "openai", key_hint: "…9fKw", last_pull_at: "2026-10-12T06:00:00Z", status: "ok", message: null },
    ],
    periods: [
      { id: "2026-07", status: "closed", closed_at: "2026-08-06T14:10:00Z", closed_by: nadia, tag_map: { cost_center: "person", ladder: ["ticket", "project"] }, tag_map_version: 1, approval: true, bills: closedBills("2026-07") },
      { id: "2026-08", status: "closed", closed_at: "2026-09-07T09:42:00Z", closed_by: nadia, tag_map: { cost_center: "person", ladder: ["ticket", "project"] }, tag_map_version: 1, approval: true, bills: closedBills("2026-08") },
      { id: "2026-09", status: "open", closed_at: null, closed_by: null, tag_map: null },
      { id: "2026-10", status: "open", closed_at: null, closed_by: null, tag_map: null },
    ],
    adjustments: [
      ...(lateEntry?.list
        ? [{ id: uuid(6, 1), period: "2026-08", amount: lateEntry.list, tags: lateEntry.splits[0]?.tags ?? {}, reason: "Late token sheet, submitted after 2026-08 closed, on 2026-09-09", late: true, posted_by: null, posted_at: "2026-09-09T08:00:00Z" }]
        : []),
      { id: uuid(6, 2), period: "2026-08", amount: -18_400_000_000, tags: { project: "claims-portal", client: "globex" }, reason: "Pen test run twice by mistake; the provider credited one run.", late: false, posted_by: nadia, posted_at: "2026-09-15T10:20:00Z" },
    ],
    links: [],
    budgets,
    reports: [
      { id: "5e000000-0000-4000-8000-000000000001", name: "Cost per team for Acme", filter: { client: "acme" }, by: "team", created_by: kai, created_at: "2026-07-20T13:10:00Z" },
      { id: "5e000000-0000-4000-8000-000000000002", name: "Code review cost by project", filter: { purpose: "code-review" }, by: "project", created_by: nadia, created_at: "2026-08-03T09:00:00Z" },
      { id: "5e000000-0000-4000-8000-000000000003", name: "Web team by project", filter: { team: "web" }, by: "project", created_by: who("priya").id, created_at: "2026-08-12T15:30:00Z" },
    ],
    rules: {
      rules: [
        ...defaultRules(),
        { name: "Review branches are code review", when: [{ signal: "branch", test: { starts_with: "review/" } }], then: { set_tags: { purpose: "code-review" } } },
      ],
      several_keys: "most_often",
      summary_match: true,
    },
    personalRules: {
      [MARA.id]: [{ name: "Spike folder is learning", when: [{ signal: "working_directory", test: { contains: "/spikes/" } }], then: { set_tags: { purpose: "learning" } } }],
    },
    tokens: [
      { id: "a0ce0000-0000-4000-8000-000000000001", kind: "service", name: "review bot (GitHub Action)", shown_prefix: "tc_svc_7d1e", person_id: who("priya").id, needs_owner: false, created_at: "2026-06-02T09:00:00Z", last_used_at: "2026-10-12T15:02:00Z" },
      { id: "a0ce0000-0000-4000-8000-000000000002", kind: "service", name: "release-notes bot", shown_prefix: "tc_svc_a93c", person_id: who("felix").id, needs_owner: true, created_at: "2026-05-14T11:00:00Z", last_used_at: "2026-10-09T17:04:00Z" },
      { id: "a0ce0000-0000-4000-8000-000000000003", kind: "organization", name: "Telemetry push", shown_prefix: "tc_org_51b0", person_id: nadia, needs_owner: false, created_at: "2026-06-01T08:00:00Z", last_used_at: "2026-10-12T16:58:00Z" },
      { id: "a0ce0000-0000-4000-8000-000000000004", kind: "read", name: "Power BI", shown_prefix: "tc_read_2c44", person_id: who("kai").id, needs_owner: false, created_at: "2026-07-20T13:00:00Z", last_used_at: "2026-10-11T07:00:00Z" },
    ],
    ticketSystems: [
      {
        id: "a0c70000-0000-4000-8000-000000000001",
        kind: "jira",
        site: "https://kestrel.atlassian.net",
        key_hint: "x7Qa",
        mappings: [
          { external: "KD", external_name: "Storefront", project: "storefront" },
          { external: "CP", external_name: "Claims Portal", project: "claims-portal" },
        ],
        last_sync_at: "2026-10-12T16:45:00Z",
        message: null,
        status: "ok",
      },
    ],
    nextId: 1000,
  };
  return db;
}
