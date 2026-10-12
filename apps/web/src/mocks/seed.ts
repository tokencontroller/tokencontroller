// The example organization the docs use, Kestrel Digital, for development,
// tests and screenshots while the service's endpoints are being built. The
// service's own seed command loads the same organization into Postgres.
//
// Everything is generated from a fixed random seed, so every run and every
// screenshot shows the same numbers. "Today" is fixed too.

import type {
  ApiToken,
  Cadence,
  CostSource,
  Organization,
  Person,
  Project,
  Rule,
  SavedReport,
  Tags,
  Ticket,
  TicketSystem,
  TokenCounts,
  TokenEntryStatus,
  BillProvider,
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
  agent_manager: string | null;
  automatic: boolean;
  status: TokenEntryStatus;
  reject_reason: string | null;
  placed_by: string;
  candidates: string[];
  splits: { share: number; tags: Tags }[];
  late: boolean;
  /** Which bill this entry's cost appears on. */
  provider: string;
}

export interface BillRecord {
  id: string;
  provider: BillProvider;
  label: string;
  period: string;
  gross: number;
  discount: number;
  credit: number;
  net: number;
  source: "api" | "csv" | "manual";
  revision: number;
  received_at: string;
  revisions: { revision: number; net: number; received_at: string }[];
  /** Whether the provider says the figure is final. */
  final: boolean;
}

export interface BudgetRecord {
  id: string;
  filter: Tags;
  period: string;
  amount: number;
  from_forecast: boolean;
  created_by: string;
}

export interface AdjustmentRecord {
  id: string;
  period: string;
  rung: string;
  value: string;
  amount: number;
  reason: string;
  posted_by: string;
  posted_at: string;
  late_sheet: boolean;
}

export interface PeriodRecord {
  id: string;
  status: "open" | "closed";
  closed_at: string | null;
  closed_by: string | null;
  tag_map: { cost_center: string; ladder: string[] } | null;
}

export interface TicketRecord extends Omit<Ticket, "cost"> {
  open: boolean;
}

export interface Db {
  organization: Organization;
  session: string | null;
  /** The last magic link "sent", so the mock sign-in page can offer it. */
  pendingLink: { token: string; email: string; next?: string } | null;
  people: Person[];
  tagNames: string[];
  tagMap: { cost_center: string; ladder: string[] };
  projects: Project[];
  tickets: TicketRecord[];
  entries: EntryRecord[];
  bills: BillRecord[];
  connections: { id: string; provider: "anthropic_enterprise" | "anthropic_console" | "openai"; status: "ok" | "failing"; last_pull_at: string | null; message: string | null }[];
  periods: PeriodRecord[];
  adjustments: AdjustmentRecord[];
  budgets: BudgetRecord[];
  reports: SavedReport[];
  rules: Rule[];
  tokens: ApiToken[];
  ticketSystems: TicketSystem[];
  cadence: Cadence | null;
  nextId: number;
}

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

const people: Person[] = [
  { id: "mara", name: "Mara Ortner", email: "m.ortner@kestrel.example", role: "agent_manager", tags: { team: "web" }, filters: [], status: "active" },
  { id: "lukas", name: "Lukas Kern", email: "l.kern@kestrel.example", role: "agent_manager", tags: { team: "web" }, filters: [], status: "active" },
  { id: "tara", name: "Tara Reyes", email: "t.reyes@kestrel.example", role: "agent_manager", tags: { team: "data" }, filters: [], status: "active" },
  { id: "aiko", name: "Aiko Sato", email: "a.sato@kestrel.example", role: "agent_manager", tags: { team: "mobile" }, filters: [], status: "active" },
  { id: "priya", name: "Priya Nair", email: "p.nair@kestrel.example", role: "people_manager", tags: { team: "web" }, filters: [{ team: "web" }], status: "active" },
  { id: "kai", name: "Kai Berg", email: "k.berg@kestrel.example", role: "people_manager", tags: { team: "delivery" }, filters: [{ client: "acme" }], status: "active" },
  { id: "dana", name: "Dana Fischer", email: "d.fischer@kestrel.example", role: "controller", tags: { team: "finance" }, filters: [], status: "active" },
  { id: "noah", name: "Noah Weber", email: "n.weber@kestrel.example", role: "agent_manager", tags: { team: "mobile" }, filters: [], status: "invited" },
];

const weekly: Cadence = { every: "weekly", days: null };

const projects: Project[] = [
  { id: "p1", key: "storefront", name: "Storefront relaunch", tags: { client: "acme" }, ticket_key_pattern: "^KD-\\d+", repositories: ["kestrel/storefront"], cadence: weekly },
  { id: "p2", key: "claims-portal", name: "Claims portal", tags: { client: "globex" }, ticket_key_pattern: "^CP-\\d+", repositories: ["kestrel/claims-portal"], cadence: null },
  { id: "p3", key: "intranet", name: "Intranet", tags: {}, ticket_key_pattern: "^INT-\\d+", repositories: ["kestrel/intranet"], cadence: null },
];

function ticket(key: string, project: string, title: string, parent: string | null, assignee: string | null, open = true, description: string | null = null): TicketRecord {
  return { key, title, description, parent, assignee, project, source: key.startsWith("INT") ? "manual" : "jira", open };
}

const tickets: TicketRecord[] = [
  ticket("KD-120", "storefront", "Cart page", null, "mara", false),
  ticket("KD-131", "storefront", "Search filters", null, "lukas", false),
  ticket("KD-150", "storefront", "Checkout address form", null, "mara", false),
  ticket("KD-200", "storefront", "Checkout", null, "priya"),
  ticket("KD-214", "storefront", "Fix checkout loop", "KD-200", "mara", true, "Payment step sends the shopper back to the cart after a declined card."),
  ticket("KD-231", "storefront", "Search ranking", null, "lukas", true, "Rank products by stock and margin, not only by text match."),
  ticket("KD-240", "storefront", "Product page images", null, "lukas"),
  ticket("KD-250", "storefront", "Checkout button states", "KD-200", "mara"),
  ticket("KD-260", "storefront", "Wishlist", null, null, true, "Shoppers save products to a wishlist and get a note when the price drops."),
  ticket("CP-1", "claims-portal", "Document upload", null, "aiko"),
  ticket("CP-2", "claims-portal", "Claim status page", null, "aiko", false),
  ticket("CP-4", "claims-portal", "Upload retry", "CP-1", "aiko"),
  ticket("CP-7", "claims-portal", "Claims list filters", null, "tara"),
  ticket("CP-12", "claims-portal", "Pre-release pen test", null, "aiko"),
  ticket("INT-3", "intranet", "Wiki search", null, "tara"),
  ticket("INT-8", "intranet", "Dependency updates", null, null),
];

interface Work {
  person: string;
  harness: string;
  model: string;
  provider: string;
  /** Ticket keys or project keys this person works on, with weights. */
  work: [string, number][];
  sessionsPerDay: number;
}

const workers: Work[] = [
  { person: "mara", harness: "Claude Code", model: "claude-opus-5", provider: "anthropic", work: [["KD-214", 4], ["KD-250", 3], ["KD-150", 2], ["KD-120", 2], ["storefront", 1], ["", 1]], sessionsPerDay: 1.8 },
  { person: "lukas", harness: "Cursor", model: "claude-sonnet-5", provider: "cursor", work: [["KD-231", 4], ["KD-240", 3], ["KD-131", 2], ["storefront", 1], ["", 1]], sessionsPerDay: 1.3 },
  { person: "lukas", harness: "Claude Code", model: "claude-sonnet-5", provider: "anthropic", work: [["KD-231", 2], ["KD-240", 1]], sessionsPerDay: 0.5 },
  { person: "tara", harness: "Gemini CLI", model: "gemini-3-pro", provider: "google", work: [["CP-7", 3], ["INT-3", 3], ["intranet", 1], ["", 1]], sessionsPerDay: 1.0 },
  { person: "tara", harness: "Claude Code", model: "claude-sonnet-5", provider: "anthropic", work: [["CP-7", 2], ["INT-3", 1]], sessionsPerDay: 0.6 },
  { person: "aiko", harness: "Codex", model: "gpt-5-codex", provider: "openai", work: [["CP-1", 3], ["CP-4", 3], ["CP-2", 2], ["claims-portal", 1], ["", 1]], sessionsPerDay: 1.4 },
  { person: "aiko", harness: "Copilot CLI", model: "gpt-5", provider: "copilot", work: [["CP-4", 2], ["CP-12", 1], ["", 1]], sessionsPerDay: 0.4 },
  { person: "priya", harness: "Claude Code", model: "claude-opus-5", provider: "anthropic", work: [["KD-200", 2], ["INT-8", 2], ["", 1]], sessionsPerDay: 0.4 },
];

const summaries: Record<string, string[]> = {
  "KD-120": ["Built the cart line item list", "Fixed rounding in cart totals"],
  "KD-131": ["Added price and brand filters", "Wrote tests for the filter panel"],
  "KD-150": ["Validated the address form", "Added postcode lookup to checkout"],
  "KD-200": ["Planned the checkout epic", "Reviewed checkout architecture notes"],
  "KD-214": ["Fixed checkout loop", "Traced the declined card redirect", "Added a test for the payment retry"],
  "KD-231": ["Tuned search ranking weights", "Added stock to the ranking score", "Ranking tweaks"],
  "KD-240": ["Lazy loading for product images", "Resized product images on upload"],
  "KD-250": ["Checkout button states", "Disabled pay button while loading"],
  "CP-1": ["Built the upload form", "Checked file types on upload"],
  "CP-2": ["Claim status timeline", "Status page copy fixes"],
  "CP-4": ["Built upload retry", "Backoff for failed uploads"],
  "CP-7": ["Filters for the claims list", "Date range filter"],
  "CP-12": ["Pre-release pen test"],
  "INT-3": ["Indexed wiki pages for search", "Search result snippets"],
  "INT-8": ["Updated dependencies", "Fixed breaking change in the router"],
  storefront: ["Set up the storefront preview environment", "Read up on the new payment API"],
  "claims-portal": ["Explored the claims API", "Set up the claims dev database"],
  intranet: ["Looked at the intranet build"],
  "": ["Tried a new test runner", "Internal tooling script", "Learning the new framework", "Wrote a shell helper"],
};

const projectOfTicket = new Map(tickets.map((t) => [t.key, t.project]));
const projectTags = new Map(projects.map((p) => [p.key, p.tags]));
const personTags = new Map(people.map((p) => [p.id, p.tags]));

/** The tags a token entry ends up with: its person, ticket, project and what they pass on. */
export function inheritTags(person: string | null, target: string, extra: Tags = {}): Tags {
  const tags: Tags = {};
  if (person) {
    tags.person = person;
    Object.assign(tags, personTags.get(person) ?? {});
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

function placedBy(target: string, harness: string): string {
  if (!target) return "no rule matched";
  if (projectOfTicket.has(target)) return harness === "Cursor" || harness === "Gemini CLI" ? "rule: ticket key in prompts" : "rule: ticket key in branch";
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
          if (ticketRecord?.open && ticketRecord.key !== "KD-200" && period < "2026-09" && !["KD-214", "CP-1", "INT-3"].includes(target)) continue;
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
            agent_manager: worker.person,
            automatic: false,
            status: "submitted",
            reject_reason: null,
            placed_by: placedBy(target, worker.harness),
            candidates: [],
            splits: [{ share: 10000, tags: inheritTags(worker.person, target, extra) }],
            late: false,
            provider: worker.provider,
          });
        }
      }
      // The CI review bot reviews a pull request most days. Priya set it up.
      if (rand() < 0.8) {
        const target = pick(rand, [["KD-214", 2], ["KD-231", 2], ["KD-240", 1], ["CP-4", 2], ["CP-7", 1]] as [string, number][]);
        if (period < "2026-09" && !["KD-214", "CP-1"].includes(target)) continue;
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
          agent_manager: "priya",
          automatic: true,
          status: "submitted",
          reject_reason: null,
          placed_by: "rule: ticket key in pull request title",
          candidates: [],
          splits: [{ share: 10000, tags: inheritTags("priya", target, { purpose: "code-review" }) }],
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

  // Lukas has not submitted his sheet for the last week of September: late.
  for (const entry of entries) {
    if (entry.agent_manager === "lukas" && !entry.automatic && entry.started_at >= "2026-09-28") {
      entry.status = "draft";
    }
  }

  // Aiko's entry was sent back with a reason.
  const rejected = entries.find((e) => e.agent_manager === "aiko" && e.period === "2026-10" && e.splits[0]?.tags.ticket === "CP-4");
  if (rejected) {
    rejected.status = "rejected";
    rejected.reject_reason = "This was the claims list work. Please move it to CP-7.";
  }

  // Mara's drafts for today, as the Review and submit docs page shows them.
  const today: [string, number, string, string, string[]][] = [
    ["09:12", 4187, "KD-214", "rule: ticket key in branch", []],
    ["11:46", 1210, "KD-214", "rule: ticket key in branch", []],
    ["13:05", 8830, "KD-231", "rule: ticket key in pull request title", []],
    ["15:20", 975, "storefront", "rule: repository", []],
    ["16:41", 6038, "", "no ticket, 2 candidates", ["KD-231", "KD-240"]],
  ];
  const todaySummaries = ["Fixed checkout loop", "Added a test for the payment retry", "Tuned search ranking weights", "Read up on the new payment API", "Ranking edge cases for empty stock"];
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
      agent_manager: "mara",
      automatic: false,
      status: "draft",
      reject_reason: null,
      placed_by: by,
      candidates,
      splits: [{ share: 10000, tags: inheritTags("mara", target) }],
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

  // Bills: token entries plus a small residual, per provider.
  const providers: { key: string; provider: BillProvider; label: string; source: "api" | "csv"; residual: number }[] = [
    { key: "anthropic", provider: "anthropic_console", label: "Anthropic", source: "api", residual: 0.031 },
    { key: "openai", provider: "openai", label: "OpenAI", source: "api", residual: 0.024 },
    { key: "google", provider: "other", label: "Google Gemini", source: "csv", residual: 0.05 },
    { key: "cursor", provider: "other", label: "Cursor", source: "csv", residual: 0.0 },
    { key: "copilot", provider: "other", label: "GitHub Copilot", source: "csv", residual: 0.04 },
  ];
  const bills: BillRecord[] = [];
  for (const period of ["2026-07", "2026-08", "2026-09"]) {
    for (const p of providers) {
      const covered = entries
        .filter((e) => e.period === period && e.provider === p.key)
        .reduce((s, e) => s + (e.list ?? 0), 0);
      // Unknown models are billed too; Google's bill covers them.
      const net = Math.round((covered * (1 + p.residual)) / 10_000_000) * 10_000_000 + (p.key === "google" ? 1_850_000_000 : 0);
      const [y = 2026, m = 1] = period.split("-").map(Number);
      const received = new Date(Date.UTC(y, m, p.source === "api" ? 3 : 6, 6)).toISOString();
      bills.push({
        id: `bill_${period}_${p.key}`,
        provider: p.provider,
        label: p.label,
        period,
        gross: net,
        discount: 0,
        credit: 0,
        net,
        source: p.source,
        revision: 1,
        received_at: received,
        revisions: [],
        final: true,
      });
    }
  }
  // Anthropic restated September once.
  const restated = bills.find((b) => b.id === "bill_2026-09_anthropic");
  if (restated) {
    restated.revisions = [{ revision: 1, net: restated.net - 4_120_000_000, received_at: "2026-10-03T06:00:00Z" }];
    restated.revision = 2;
    restated.received_at = "2026-10-09T06:00:00Z";
  }

  const budget = (filter: Tags, period: string, dollars: number, from_forecast = false): BudgetRecord => ({
    id: `tb_${Object.values(filter).join("_")}_${period}`,
    filter,
    period,
    amount: dollars * NANOS,
    from_forecast,
    created_by: filter.client ? "kai" : "priya",
  });
  const budgets: BudgetRecord[] = [];
  for (const period of ["2026-09", "2026-10"]) {
    budgets.push(
      budget({ person: "mara" }, period, 900),
      budget({ team: "web" }, period, 3200),
      budget({ client: "acme" }, period, 2800),
      budget({ ticket: "KD-214" }, period, 560, true),
      budget({ ticket: "KD-231" }, period, 440),
      budget({ ticket: "CP-4" }, period, 380, true),
      budget({ project: "intranet" }, period, 600),
    );
  }

  const rule = (id: string, name: string, signal: Rule["signal"], match: Rule["match"], pattern: string, sets: Tags, scope: Rule["scope"] = "organization", built_in = false): Rule => ({ id, scope, name, signal, match, pattern, sets, built_in });

  const db: Db = {
    organization: {
      id: "org_kestrel",
      name: "Kestrel Digital",
      plan: "team",
      cost_source: "console_api_key" satisfies CostSource,
      reporting_currency: CURRENCY,
      approval: true,
      sign_in_domains: ["kestrel.example"],
      summary_match: true,
      several_keys: "most_named",
      forecasts: true,
      billing: true,
      members: people.length,
      member_limit: 150,
    },
    session: null,
    pendingLink: null,
    people,
    tagNames: ["person", "ticket", "project", "team", "client", "purpose"],
    tagMap: { cost_center: "person", ladder: ["ticket", "project"] },
    projects,
    tickets,
    entries,
    bills,
    connections: [{ id: "bc_1", provider: "anthropic_console", status: "ok", last_pull_at: "2026-10-12T06:00:00Z", message: null }, { id: "bc_2", provider: "openai", status: "ok", last_pull_at: "2026-10-12T06:00:00Z", message: null }],
    periods: [
      { id: "2026-07", status: "closed", closed_at: "2026-08-06T14:10:00Z", closed_by: "dana", tag_map: { cost_center: "person", ladder: ["ticket", "project"] } },
      { id: "2026-08", status: "closed", closed_at: "2026-09-07T09:42:00Z", closed_by: "dana", tag_map: { cost_center: "person", ladder: ["ticket", "project"] } },
      { id: "2026-09", status: "open", closed_at: null, closed_by: null, tag_map: null },
      { id: "2026-10", status: "open", closed_at: null, closed_by: null, tag_map: null },
    ],
    adjustments: [
      { id: "adj_1", period: "2026-08", rung: "project", value: "claims-portal", amount: -18_400_000_000, reason: "Pen test run twice by mistake; the provider credited one run.", posted_by: "dana", posted_at: "2026-09-15T10:20:00Z", late_sheet: false },
    ],
    budgets,
    reports: [
      { id: "rp_1", name: "Cost per team for Acme", filter: { client: "acme" }, group_by: "team" },
      { id: "rp_2", name: "Code review cost by project", filter: { purpose: "code-review" }, group_by: "project" },
      { id: "rp_3", name: "Web team by project", filter: { team: "web" }, group_by: "project" },
    ],
    rules: [
      rule("r_1", "Ticket key in branch", "branch", "ticket_key", "", {}, "organization", true),
      rule("r_2", "Ticket key in pull request title", "pull_request_title", "ticket_key", "", {}, "organization", true),
      rule("r_3", "Ticket key in prompts or session title", "prompts", "ticket_key", "", {}, "organization", true),
      rule("r_4", "Repository mapped to a project", "repository", "ticket_key", "", {}, "organization", true),
      rule("r_5", "Review branches are code review", "branch", "starts_with", "review/", { purpose: "code-review" }),
      rule("r_6:mara", "Spike folder is learning", "working_directory", "contains", "/spikes/", { purpose: "learning" }, "personal"),
    ],
    tokens: [
      { id: "tok_1", kind: "svc", name: "review bot (GitHub Action)", prefix: "tc_svc_7d1e", owner: "priya", created_at: "2026-06-02T09:00:00Z", last_used_at: "2026-10-12T15:02:00Z" },
      { id: "tok_2", kind: "svc", name: "release-notes bot", prefix: "tc_svc_a93c", owner: null, created_at: "2026-05-14T11:00:00Z", last_used_at: "2026-10-09T17:04:00Z" },
      { id: "tok_3", kind: "org", name: "Telemetry push", prefix: "tc_org_51b0", owner: "dana", created_at: "2026-06-01T08:00:00Z", last_used_at: "2026-10-12T16:58:00Z" },
      { id: "tok_4", kind: "read", name: "Power BI", prefix: "tc_read_2c44", owner: "kai", created_at: "2026-07-20T13:00:00Z", last_used_at: "2026-10-11T07:00:00Z" },
    ],
    ticketSystems: [
      { id: "ts_1", kind: "jira", site: "kestrel.atlassian.net", mappings: [{ external: "KD", project: "storefront" }, { external: "CP", project: "claims-portal" }], last_sync_at: "2026-10-12T16:45:00Z", status: "ok" },
    ],
    cadence: null,
    nextId: 1000,
  };
  return db;
}
