// The parts of the API the web app uses that the service's OpenAPI document
// does not describe yet. Every type here mirrors a docs page; field names use
// the glossary words. When an endpoint lands in apps/server/openapi.json, its
// types move to `generated.ts` (see `client.ts`) and the matching entry here
// is deleted, so the generated document is the only contract left.

import type { components } from "./generated";

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

// Types the service already publishes come from the generated document.
type Schemas = components["schemas"];

export type Role = Schemas["Role"];

/** How a provider charges the organization (docs: Account > Bills). */
export type CostSource = Schemas["CostSource"];

export type Organization = Schemas["Organization"];

/** A member as the API shows them. Their `person` tag value is their email. */
export type Person = Schemas["MemberView"];

export type Me = Schemas["Me"];
export type SignUpRequest = Schemas["SignUpRequest"];

/** A rule: when every condition holds, do the action (docs: Rules). */
export type Rule = Schemas["Rule"];
export type RuleCondition = Schemas["RuleCondition"];
export type RuleAction = Schemas["RuleAction"];
export type RuleTest = Schemas["RuleTest"];
export type RuleSignal = Schemas["RuleSignal"];
export type RuleKeySource = Schemas["RuleKeySource"];
/** What to do when prompts name several tickets. */
export type SeveralKeys = Schemas["SeveralKeys"];
export type OrganizationRules = Schemas["OrganizationRules"];
export type OrganizationRulesChange = Schemas["OrganizationRulesChange"];

/** Who is signed in, with the whole organization the pages read settings from. */
export interface Account {
  person: Person;
  organization: Organization;
}

export type Billing = Schemas["Billing"];
export type Plan = Schemas["Plan"];
export type Interval = Schemas["Interval"];
export type InviteView = Schemas["InviteView"];
export type Invited = Schemas["Invited"];
export type NewMember = Schemas["NewMember"];
export type MemberChange = Schemas["MemberChange"];
export type OrganizationChange = Schemas["OrganizationChange"];

export type TagName = Schemas["TagName"];
export type TagValue = Schemas["TagValue"];
export type TagMap = Schemas["TagMap"];
export type TagMapChange = Schemas["TagMapChange"];

export type Project = Schemas["Project"];
export type NewProject = Schemas["NewProject"];
export type ProjectChange = Schemas["ProjectChange"];
export type Ticket = Schemas["Ticket"];
/** A connected ticket system (docs: Integrations). The token is never sent back. */
export type TicketSystem = Schemas["TicketSystemView"];
export type NewTicketSystem = Schemas["NewTicketSystem"];
/** One project of a ticket system mapped to a Token Controller project, by name. */
export type TicketMapping = Schemas["Mapping"];
export type ExternalProject = Schemas["ExternalProject"];
export type NewTicket = Schemas["NewTicket"];
export type TicketSource = Schemas["TicketSource"];
export type Client = Schemas["Client"];

export type ApiToken = Schemas["ApiToken"];
export type ApiTokenKind = Schemas["TokenKind"];
export type CreatedApiToken = Schemas["CreatedApiToken"];

// Customer relations: feedback reports, support inside the app, the admin area.
export type FeedbackKind = Schemas["Kind"];
export type FeedbackStatus = Schemas["Status"];
export type Diagnostics = Schemas["Diagnostics"];
export type Screenshot = Schemas["Screenshot"];
export type NewReport = Schemas["NewReport"];
export type Report = Schemas["ReportView"];
export type ReceivedReport = Schemas["ReceivedReport"];
export type ReportChange = Schemas["ReportChange"];
export type SupportView = Schemas["SupportView"];
export type AdminOverview = Schemas["Overview"];
export type Conversation = Schemas["ConversationSummary"];
export type ConversationView = Schemas["ConversationView"];
export type SupportMessage = Schemas["MessageView"];
export type CustomerSummary = Schemas["CustomerSummary"];
export type CustomerRecord = Schemas["CustomerRecord"];
export type CustomerMember = Schemas["CustomerMember"];
export type CustomerNote = Schemas["Note"];
export type NewsletterView = Schemas["NewsletterView"];
export type Issue = Schemas["IssueView"];
export type IssueText = Schemas["IssueText"];
export type AssistantQuestion = Schemas["QuestionView"];


// Cadence, as the Close and reports work defines it (docs: Account > Cadence).
export type CadenceKind = "weekly" | "every_two_weeks" | "monthly" | "custom";

/** One stretch of a cadence, from `starts_on` up to and including `ends_on`. */
export interface CadencePeriod {
  starts_on: string;
  ends_on: string;
}

/** A cadence for the organization (`project` null) or one project, by project name. */
export interface CadenceView {
  id: string;
  kind: CadenceKind;
  every_days?: number | null;
  project?: string | null;
  starts_on: string;
  set_at: string;
  set_by: string;
  current: CadencePeriod;
}

export interface CadenceInput {
  kind: CadenceKind;
  every_days?: number | null;
  project?: string | null;
  starts_on?: string | null;
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
  cadence: CadenceKind | null;
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
  tag_map: TagMapChange;
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



