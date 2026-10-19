// The mock service's data: Kestrel Digital, the docs' example organization,
// with the worked month of the docs (March 2026): one bill of 1,000, four
// sessions of 950, a plan for Shop and KD-1, and Mara's share of Shop.
// April has a few open sessions to check and submit. The service seeds the
// same month (apps/server/src/seed).

import type {
  ApiToken,
  BillConnection,
  Cutoff,
  EventView,
  Grant,
  Member,
  Organization,
  PersonAttribute,
  PriceRow,
  Rule,
  SavedReport,
  Share,
  Submission,
  SubmitRequest,
  Tags,
  TicketSystem,
} from "@/api/contract";

export const TODAY = "2026-04-10";

export interface Tokens {
  input: number;
  output: number;
  cache_read: number;
  cache_write: number;
}

export interface MockMessage {
  message_id: string;
  at: string;
  provider: string;
  model: string;
  /** `null`: the message came without counts, a count error. */
  tokens: Tokens | null;
}

export interface MockSession {
  session_id: string;
  /** The person, by email. */
  person: string;
  agent_harness: string;
  agent_name: string | null;
  started_at: string;
  ended_at: string;
  summary: string;
  signals: Record<string, unknown>;
  facts: Record<string, unknown>;
  messages: MockMessage[];
  /** What rules found on arrival; empty when none matched. */
  ruleTags: Tags;
}

export interface MockSplit {
  id: string;
  session_id: string;
  /** Fractions in basis points, adding up to 10,000. */
  parts: { bp: number; tags: Tags }[];
  recorded_at: string;
  by: string;
}

export interface MockValue {
  id: string;
  name: string;
  key: string;
  title: string | null;
  description: string | null;
  source_id: string | null;
  source_status: string | null;
  defaults: Tags;
  recorded_at: string;
}

export interface MockEstimate {
  id: string;
  work: string;
  tags: Tags;
  /** Median with the markup inside, in nanos of US dollars. */
  median: number;
  markup_percent: number;
  bands: { p: string; up_to: string | null }[];
  estimator: string;
  from: string;
  to: string;
  recorded_at: string;
  /** From the classifier, once there is cost. */
  label: string | null;
}

export interface MockBill {
  id: string;
  provider: string;
  from: string;
  to: string;
  /** Nanos of the bill's currency. */
  amount: number;
  currency: string;
  source: "pulled" | "csv" | "manual" | "api";
  final: boolean;
  recorded_at: string;
  recorded_by: string | null;
}

export interface Db {
  organization: Organization;
  people: Member[];
  /** Who is signed in, by person id. */
  session: string | null;
  pendingLink: { token: string; email: string; url?: string; start?: string } | null;
  invites: { code: string; person: string }[];
  tagNames: { name: string; built_in: boolean }[];
  tagValues: MockValue[];
  attributeNames: { name: string; built_in: boolean }[];
  attributeValues: MockValue[];
  personAttributes: PersonAttribute[];
  sessions: MockSession[];
  splits: MockSplit[];
  prices: PriceRow[];
  bills: MockBill[];
  connections: BillConnection[];
  estimates: MockEstimate[];
  /** Tag set (as filter text) to the accepted estimate's id. */
  accepted: Record<string, string>;
  shares: Share[];
  grants: Grant[];
  cutoff: Cutoff;
  submissions: Submission[];
  submitRequests: SubmitRequest[];
  rules: Rule[];
  saved: SavedReport[];
  tokens: ApiToken[];
  ticketSystems: TicketSystem[];
  events: EventView[];
  nextId: number;
}

const SETUP = "2026-02-15T09:00:00Z";
const MODEL = "claude-sonnet-4-5";

const person = (n: number) => `6b657374-0000-4000-8001-${String(n).padStart(12, "0")}`;

export const ids = { nadia: person(1), priya: person(2), mara: person(4), tom: person(6), ola: person(9) };

function member(id: string, name: string, email: string, role: Member["role"], attributes: Tags, filters: Tags[] = [], joined = true): Member {
  return {
    id,
    name,
    email,
    role,
    attributes: attributes as Member["attributes"],
    filters: filters as Member["filters"],
    status: joined ? "joined" : "invited",
    invited_at: SETUP,
    joined_at: joined ? SETUP : null,
  };
}

const value = (name: string, key: string, title: string | null, defaults: Tags = {}, description: string | null = null): MockValue => ({
  id: `val_${name}_${key.toLowerCase()}`,
  name,
  key,
  title,
  description,
  source_id: null,
  source_status: null,
  defaults,
  recorded_at: SETUP,
});

const millions = (input: number): Tokens => ({ input: input * 1_000_000, output: 0, cache_read: 0, cache_write: 0 });

function session(
  id: string,
  who: string,
  at: string,
  minutes: number,
  summary: string,
  messages: MockMessage[],
  ruleTags: Tags,
  signals: Record<string, unknown>,
  agent_name: string | null = null,
): MockSession {
  const end = new Date(new Date(at).getTime() + minutes * 60_000).toISOString().replace(".000", "");
  return {
    session_id: id,
    person: who,
    agent_harness: agent_name ? "github-action" : "claude-code",
    agent_name,
    started_at: at,
    ended_at: end,
    summary,
    signals,
    facts: { turns: Math.max(4, Math.round(minutes / 6)), tool_calls: Math.round(minutes * 1.5), retries: 0, duration_seconds: minutes * 60, subagents: 0, files_touched: Math.round(minutes / 10) },
    messages,
    ruleTags,
  };
}

const msg = (id: string, at: string, tokens: Tokens | null, model = MODEL): MockMessage => ({ message_id: id, at, provider: "anthropic", model, tokens });

export const emails = {
  nadia: "n.varga@kestrel.example",
  priya: "p.raman@kestrel.example",
  mara: "m.ortner@kestrel.example",
  tom: "t.lindqvist@kestrel.example",
  ola: "o.berg@kestrel.example",
};

const price = (kind: string, perMillion: string): PriceRow => ({
  id: `prc_managed_${kind}`,
  provider: "anthropic",
  model: MODEL,
  kind,
  price_per_million: perMillion,
  valid_from: "2026-01-01",
  source: "managed",
  recorded_at: SETUP,
});

export function createDb(): Db {
  const web = { team: "web" };
  return {
    organization: {
      id: "0e000000-0000-4000-8000-000000000001",
      name: "Kestrel Digital",
      slug: "kestrel",
      reporting_currency: "USD",
      approval: true,
      cutoff: "2026-02-28",
      sign_in_domains: ["kestrel.example"],
      created_at: SETUP,
    },
    people: [
      member(ids.nadia, "Nadia Varga", emails.nadia, "org_manager", {}),
      member(ids.priya, "Priya Raman", emails.priya, "people_manager", { team: "web", department: "engineering" }, [web]),
      member(ids.mara, "Mara Ortner", emails.mara, "agent_manager", { team: "web", department: "engineering" }),
      member(ids.tom, "Tom Lindqvist", emails.tom, "agent_manager", { team: "web", department: "engineering" }),
      member(ids.ola, "Ola Berg", emails.ola, "agent_manager", {}, [], false),
    ],
    session: null,
    pendingLink: null,
    invites: [{ code: "kestrel-4f2a", person: ids.ola }],
    tagNames: [
      { name: "project", built_in: true },
      { name: "feature", built_in: true },
      { name: "client", built_in: true },
      { name: "purpose", built_in: false },
    ],
    tagValues: [
      value("client", "acme", "Acme"),
      value("project", "shop", "Shop", { client: "acme" }, "Acme's web shop."),
      value("feature", "KD-1", "Checkout redesign", { project: "shop" }, "A new checkout flow with saved addresses and one-page payment."),
      value("feature", "KD-2", "Returns portal", { project: "shop" }, "Customers start a return and print the label themselves."),
      value("purpose", "learning", "Learning"),
    ],
    attributeNames: [
      { name: "team", built_in: true },
      { name: "department", built_in: true },
      { name: "location", built_in: true },
    ],
    attributeValues: [value("team", "web", "Web", { department: "engineering" }), value("department", "engineering", "Engineering"), value("location", "vienna", "Vienna")],
    personAttributes: [ids.priya, ids.mara, ids.tom].map((id, i) => ({ id: `pat_seed_${String(i)}`, person: id, name: "team", key: "web", from: null })),
    sessions: [
      session("seed-s1", emails.mara, "2026-03-04T10:00:00Z", 95, "Checkout: saved addresses", [msg("seed-s1-1", "2026-03-04T10:00:00Z", millions(40))], { feature: "KD-1" }, {
        branch: "KD-1-saved-addresses",
        repository: "kestrel/shop",
        pull_request: 412,
        pull_request_title: "KD-1 Saved addresses in checkout",
      }),
      session("seed-s2", emails.mara, "2026-03-12T14:00:00Z", 120, "Shared payment component for checkout and returns", [msg("seed-s2-1", "2026-03-12T14:00:00Z", millions(30))], {}, {
        branch: "payment-component",
        repository: "kestrel/shop",
        keys: ["KD-1", "KD-2"],
      }),
      session("seed-s3", emails.tom, "2026-03-18T09:00:00Z", 40, "Nightly review of open pull requests", [msg("seed-s3-1", "2026-03-18T09:00:00Z", millions(10))], { project: "shop" }, { repository: "kestrel/shop" }, "review bot"),
      session("seed-s4", emails.tom, "2026-03-25T11:00:00Z", 60, "Exploring a logging library", [msg("seed-s4-1", "2026-03-25T11:00:00Z", millions(15))], {}, { branch: "spike/logging", repository: "kestrel/tools" }),
      // April: open, to check and submit.
      session(
        "apr-s5",
        emails.mara,
        "2026-04-07T09:12:00Z",
        70,
        "Returns portal: printable label",
        [msg("apr-s5-1", "2026-04-07T09:12:00Z", { input: 1_200_000, output: 40_000, cache_read: 0, cache_write: 0 }), msg("apr-s5-2", "2026-04-07T09:40:00Z", { input: 300_000, output: 10_000, cache_read: 0, cache_write: 0 }, "claude-nova-2")],
        {},
        { branch: "returns-label", repository: "kestrel/returns" },
      ),
      session("apr-s6", emails.mara, "2026-04-08T13:05:00Z", 30, "Read the logging spike notes", [msg("apr-s6-1", "2026-04-08T13:05:00Z", { input: 500_000, output: 20_000, cache_read: 0, cache_write: 0 })], {}, { working_directory: "~/notes" }),
      session("apr-s7", emails.tom, "2026-04-09T16:20:00Z", 25, "Dependency updates", [msg("apr-s7-1", "2026-04-09T16:20:00Z", null)], {}, { branch: "deps", repository: "kestrel/tools" }),
    ],
    splits: [
      {
        id: "spl_seed_s2",
        session_id: "seed-s2",
        parts: [
          { bp: 6000, tags: { feature: "KD-1" } },
          { bp: 4000, tags: { feature: "KD-2" } },
        ],
        recorded_at: "2026-03-12T16:00:00Z",
        by: emails.mara,
      },
    ],
    prices: [price("input", "10.00"), price("output", "50.00"), price("cache_read", "1.00"), price("cache_write", "12.50")],
    bills: [
      {
        id: "bil_seed_anthropic_2026_03",
        provider: "anthropic",
        from: "2026-03-01",
        to: "2026-03-31",
        amount: 1_000 * 1e9,
        currency: "USD",
        source: "pulled",
        final: true,
        recorded_at: "2026-04-02T08:00:00Z",
        recorded_by: emails.nadia,
      },
    ],
    connections: [{ id: "con_anthropic", provider: "anthropic_console", key_hint: "sk-ant-admin…7f3c", status: "ok", last_pull_at: "2026-04-10T04:00:00Z", message: null }],
    estimates: [
      {
        id: "est_seed_shop",
        work: "Shop: checkout redesign and returns portal",
        tags: { project: "shop" },
        median: 750 * 1e9,
        markup_percent: 0,
        bands: [],
        estimator: emails.nadia,
        from: "2026-03-01",
        to: "2026-06-08",
        recorded_at: "2026-02-20T10:00:00Z",
        label: null,
      },
      {
        id: "est_seed_kd_1",
        work: "Checkout redesign",
        tags: { feature: "KD-1" },
        median: 500 * 1e9,
        markup_percent: 0,
        bands: [
          { p: "0.20", up_to: "350.00" },
          { p: "0.45", up_to: "550.00" },
          { p: "0.25", up_to: "800.00" },
          { p: "0.10", up_to: null },
        ],
        estimator: "classifier",
        from: "2026-03-01",
        to: "2026-03-31",
        recorded_at: "2026-02-21T10:00:00Z",
        label: "scope",
      },
      {
        id: "est_seed_kd_2",
        work: "Returns portal: start a return and print the label",
        tags: { feature: "KD-2" },
        median: 161 * 1e9,
        markup_percent: 15,
        bands: [
          { p: "0.30", up_to: "120.00" },
          { p: "0.50", up_to: "200.00" },
          { p: "0.20", up_to: null },
        ],
        estimator: "classifier",
        from: "2026-04-01",
        to: "2026-04-30",
        recorded_at: "2026-04-01T09:00:00Z",
        label: null,
      },
    ],
    accepted: { "project:shop": "est_seed_shop", "feature:KD-1": "est_seed_kd_1" },
    shares: [{ id: "shr_seed_mara_shop", person: emails.mara, project: "shop", fraction: "0.6", from: "2026-03-01", to: null, over: null }],
    grants: [],
    cutoff: { cutoff: "2026-02-28", event_id: "cut_seed", frozen_as_of: "2026-02-28T00:00:00Z", moved_at: "2026-03-02T09:00:00Z", moved_by: emails.nadia },
    submissions: [
      { id: "sub_seed_mara_03", person: emails.mara, from: "2026-03-01", to: "2026-03-31", messages: 2, cost: "700.00", status: "submitted", reason: null },
      { id: "sub_seed_tom_03", person: emails.tom, from: "2026-03-01", to: "2026-03-31", messages: 2, cost: "250.00", status: "approved", reason: null },
    ],
    submitRequests: [],
    rules: [
      { id: "rul_seed_branch", name: "feature key in branch", signal: "branch", pattern: "(?<key>KD-\\d+)", tag_name: "feature", key: null, order: 1, scope: "organization", recorded_at: SETUP },
      { id: "rul_seed_repo", name: "repository kestrel/shop is project Shop", signal: "repository", pattern: "^kestrel/shop$", tag_name: "project", key: "shop", order: 2, scope: "organization", recorded_at: SETUP },
    ],
    saved: [],
    tokens: [{ id: "tok_bot", name: "review bot", kind: "service", person_id: ids.tom, shown_prefix: "tc_svc_9f2c", created_at: SETUP, last_used_at: "2026-04-09T02:00:00Z", needs_owner: false }],
    ticketSystems: [],
    events: [],
    nextId: 100,
  };
}
