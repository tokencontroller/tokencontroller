// The parts of the API the web app uses that the service's OpenAPI document
// does not describe yet. Every type here mirrors a docs page; field names use
// the glossary words. When an endpoint lands in apps/server/openapi.json, its
// types move to `generated.ts` (see `client.ts`) and the matching entry here
// is deleted, so the generated document is the only contract left.

/** An amount of money in whole nano-units (ADR 0004), with its currency. */
export interface Money {
  /** Nano-units: one billionth of a unit. A JSON number or a decimal string. */
  nanos: number | string;
  /** ISO 4217 code, such as `USD`. */
  currency: string;
}

/** One calendar month, written `YYYY-MM`. */
export type PeriodId = string;

/** Tag name to value. One value per tag name. */
export type Tags = Record<string, string>;

/** A set of tags. A token entry matches when it has every tag in it. */
export type Filter = Tags;

export type Role = "agent_manager" | "people_manager" | "controller";

export type Plan = "solo" | "team" | "enterprise" | "self_hosted";

export type CostSource =
  | "enterprise_api_rates"
  | "console_api_key"
  | "seat_plan"
  | "cloud_marketplace"
  | "none";

export type SeveralKeys = "most_named" | "equal_split" | "first_named";

export interface Organization {
  id: string;
  name: string;
  plan: Plan;
  cost_source: CostSource;
  reporting_currency: string;
  approval: boolean;
  /** Email domains that may sign in by magic link. Empty means any. */
  sign_in_domains: string[];
  /** Summary match on or off (Rules page). */
  summary_match: boolean;
  /** What to do when prompts name several tickets. */
  several_keys: SeveralKeys;
  /** Token forecasts on or off; a self-hosted instance may run without them. */
  forecasts: boolean;
  /** Whether this service runs the Team plan billing (hosted service only). */
  billing: boolean;
  /** Members now, against the plan's limit (150 on the Team plan, 1 on Solo). */
  members: number;
  member_limit: number | null;
}

export interface Person {
  id: string;
  name: string;
  email: string;
  role: Role;
  /** Tags the person passes to their new token entries, such as `team: web`. */
  tags: Tags;
  /** The filters a people manager is assigned. Empty for other roles. */
  filters: Filter[];
  status: "active" | "invited" | "left";
}

export interface Me {
  person: Person;
  organization: Organization;
}

export interface SignUp {
  organization: string;
  name: string;
  email: string;
  /** The one question at sign-up. */
  manages: "agents" | "people";
  cost_source: CostSource;
  reporting_currency: string;
}

export interface TagName {
  name: string;
  built_in: boolean;
  /** Distinct values in open periods, most used first. */
  values: { value: string; entries: number }[];
}

export interface TagMap {
  /** The tag name that fills the cost center. Default `person`. */
  cost_center: string;
  /** Rungs of the cost object, most specific first. The organization is always the last rung and not listed. */
  ladder: string[];
}

export type CadenceEvery = "weekly" | "biweekly" | "monthly" | "custom";

export interface Cadence {
  every: CadenceEvery;
  /** Days per cadence period when `every` is `custom`. */
  days: number | null;
}

export interface Project {
  id: string;
  /** The value of the `project` tag, such as `storefront`. */
  key: string;
  name: string;
  /** Tags its token entries inherit, such as `client: acme`. */
  tags: Tags;
  ticket_key_pattern: string;
  repositories: string[];
  cadence: Cadence | null;
}

export type TicketSource = "jira" | "azure_devops" | "github_issues" | "manual";

export interface Ticket {
  key: string;
  title: string | null;
  description: string | null;
  parent: string | null;
  assignee: string | null;
  project: string;
  source: TicketSource;
  /** Cost so far on the ticket and its children, in the reporting currency. */
  cost: Money;
}

export type TokenEntryStatus = "draft" | "submitted" | "approved" | "rejected" | "locked";

export type CostKind = "direct" | "project_overhead" | "organization_overhead";

/** Billed, a list-price estimate, or an unknown model. Every figure says which. */
export type ValueLabel = "billed" | "list_estimate" | "unknown_model";

export interface TokenCounts {
  input: number;
  output: number;
  cache_write: number;
  cache_read: number;
}

export interface Split {
  /** Share of the token entry in basis points; all splits add up to 10000. */
  share: number;
  tags: Tags;
  value: Money | null;
  cost_kind: CostKind;
  cost_center: string;
  cost_object: string;
}

export interface TokenEntry {
  id: string;
  period: PeriodId;
  agent_harness: string;
  agent_name: string | null;
  model: string;
  started_at: string;
  ended_at: string;
  tokens: TokenCounts;
  list_value: Money | null;
  billed_value: Money | null;
  value_label: ValueLabel;
  summary: string;
  /** The person who answers for it. Null only when its service token belongs to nobody. */
  agent_manager: string | null;
  automatic: boolean;
  status: TokenEntryStatus;
  reject_reason: string | null;
  /** How its ticket or project was found, such as `rule: ticket key in branch`. */
  placed_by: string;
  /** Candidate tickets when no rule found one. */
  candidates: string[];
  splits: Split[];
  /** Posted after the close as an adjustment, marked late. */
  late: boolean;
}

export interface NewTokenEntry {
  agent_harness: string;
  agent_name: string | null;
  model: string;
  started_at: string;
  ended_at: string;
  tokens: TokenCounts;
  reported_cost: Money | null;
  summary: string;
  tags: Tags;
}

export interface CsvPreview<Row> {
  rows: Row[];
  /** Problems per row, by row number counted from 1. */
  problems: { row: number; message: string }[];
}

export interface TokenSheet {
  person: string;
  cadence: Cadence | null;
  /** The cadence period this sheet is for. */
  starts: string;
  ends: string;
  late: boolean;
  /** Drafts and rejected token entries, the review list. */
  entries: TokenEntry[];
  total: Money;
  /** Share of the sheet's cost on tickets, in basis points. */
  on_tickets: number;
}

export interface SubmitResult {
  entries: number;
  total: Money;
  on_tickets: number;
}

export type BillProvider =
  | "anthropic_enterprise"
  | "anthropic_console"
  | "openai"
  | "bedrock"
  | "vertex"
  | "foundry"
  | "other";

export interface Bill {
  id: string;
  provider: BillProvider;
  /** A label for `other`, such as `Cursor`. */
  label: string;
  period: PeriodId;
  gross: Money;
  discount: Money;
  credit: Money;
  net: Money;
  /** In the reporting currency at the exchange rate of the bill's date. */
  net_reporting: Money;
  exchange_rate: string | null;
  source: "api" | "csv" | "manual";
  revision: number;
  received_at: string;
  /** Earlier revisions, newest first. */
  revisions: { revision: number; net: Money; received_at: string }[];
}

export interface NewBill {
  provider: BillProvider;
  label: string;
  period: PeriodId;
  gross: Money;
  discount: Money;
  credit: Money;
  net: Money;
}

export interface BillConnection {
  id: string;
  provider: "anthropic_enterprise" | "anthropic_console" | "openai";
  status: "ok" | "failing";
  last_pull_at: string | null;
  message: string | null;
}

export interface Reconciliation {
  period: PeriodId;
  bills: {
    bill: string;
    provider: BillProvider;
    label: string;
    bill_net: Money;
    token_entries: Money;
    residual: Money;
  }[];
  total: { bill_net: Money; token_entries: Money; residual: Money };
  /** Share of the bills that token entries explain, in basis points. */
  attribution: number;
}

export interface Period {
  id: PeriodId;
  status: "open" | "closed";
  closed_at: string | null;
  closed_by: string | null;
  total: Money;
}

export interface CloseCheck {
  period: PeriodId;
  bills: { provider: BillProvider; label: string; received: boolean; final: boolean }[];
  organization_overhead: { amount: Money; entries: number };
  late_sheets: { person: string; starts: string; ends: string }[];
  unapproved: number;
  approval: boolean;
  reconciliation: Reconciliation;
  can_close: boolean;
  /** Why the close is blocked, in plain words. */
  blocked_by: string[];
}

export interface CostSheetLine {
  /** The ladder rung, such as `project` or `ticket`. */
  rung: string;
  value: string;
  label: string | null;
  amount: Money;
  children: CostSheetLine[];
}

export interface Adjustment {
  id: string;
  period: PeriodId;
  rung: string;
  value: string;
  amount: Money;
  reason: string;
  posted_by: string;
  posted_at: string;
  late_sheet: boolean;
}

export interface NewAdjustment {
  rung: string;
  value: string;
  amount: Money;
  reason: string;
}

export interface CostSheet {
  period: PeriodId;
  closed_at: string;
  closed_by: string;
  tag_map: TagMap;
  lines: CostSheetLine[];
  by_kind: Record<CostKind, Money>;
  adjustments: Adjustment[];
  reconciliation: Reconciliation;
  approval: boolean;
}

export interface TokenBudget {
  id: string;
  filter: Filter;
  period: PeriodId;
  amount: Money;
  actual: Money;
  from_forecast: boolean;
  created_by: string;
}

export interface NewTokenBudget {
  filter: Filter;
  period: PeriodId;
  amount: Money;
  from_forecast: boolean;
}

export interface TokenForecast {
  ticket: string;
  matches: { ticket: string; title: string | null; cost: Money; confidence: number }[];
  /** The median cost of the matches. */
  proposal: Money;
}

export interface SavedReport {
  id: string;
  name: string;
  filter: Filter;
  group_by: string;
}

export interface ReportQuery {
  period: PeriodId;
  filter: Filter;
  group_by: string;
  currency?: string;
  approved_only?: boolean;
}

export interface ReportRow {
  /** Null is the line for entries without the tag. */
  value: string | null;
  amount: Money;
  /** Share of the report's total, in basis points. */
  share: number;
  label: ValueLabel;
}

export interface ReportResult {
  period: PeriodId;
  currency: string;
  group_by: string;
  rows: ReportRow[];
  total: Money;
  /** True on a seat plan: every figure is an estimate at API prices. */
  estimate: boolean;
}

export interface BudgetVsActualRow {
  budget: string;
  filter: Filter;
  amount: Money;
  actual: Money;
  /** Actual minus token budget. */
  difference: Money;
}

export type HarnessCut = "agent_harness" | "agent_name" | "model" | "effort";

export interface Outlier {
  ticket: string;
  title: string | null;
  cost: Money;
  baseline: Money;
  /** Cost divided by baseline, times 100. */
  ratio: number;
}

export type RuleSignal =
  | "branch"
  | "pull_request_title"
  | "prompts"
  | "repository"
  | "working_directory"
  | "subagent_type";

export interface Rule {
  id: string;
  scope: "organization" | "personal";
  name: string;
  signal: RuleSignal;
  /** `ticket_key` finds a key by the project's pattern; the others compare text. */
  match: "ticket_key" | "starts_with" | "contains" | "equals";
  pattern: string;
  /** Tags the rule sets. Empty for `ticket_key`, which sets the ticket it finds. */
  sets: Tags;
  built_in: boolean;
}

export interface NewRule {
  name: string;
  signal: RuleSignal;
  match: Rule["match"];
  pattern: string;
  sets: Tags;
}

export type ApiTokenKind = "svc" | "read" | "org";

export interface ApiToken {
  id: string;
  kind: ApiTokenKind;
  name: string;
  /** The first characters, such as `tc_svc_4f2a`. */
  prefix: string;
  owner: string | null;
  created_at: string;
  last_used_at: string | null;
}

export interface NewApiTokenResult {
  token: ApiToken;
  /** The full secret, shown once. */
  secret: string;
}

export interface TicketSystem {
  id: string;
  kind: "jira" | "azure_devops" | "github_issues";
  site: string;
  /** External project to Token Controller project key. */
  mappings: { external: string; project: string }[];
  last_sync_at: string | null;
  status: "ok" | "failing";
}

export interface CheckoutSession {
  url: string;
}

/** A computer asking to connect with `tc connect`, found by the code the terminal shows. */
export interface CliAuthorization {
  user_code: string;
  device: string;
  organization: string;
  status: "pending" | "confirmed" | "declined" | "expired";
  expires_at: string;
}
