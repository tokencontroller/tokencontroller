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

/** One token entry as it arrives (docs: Reference > Token entry). */
export type NewTokenEntry = Schemas["NewTokenEntry"];
/** What storing a token entry gave. */
export type StoredEntry = Schemas["StoredEntryBody"];
/** What a CSV file of token entries holds, before anything is saved. */
export type EntriesCsvPreview = Schemas["CsvPreview"];

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

// Token budgets and token forecasts (docs: Account > Token budgets and
// forecasts; Report > Token budget vs actual).
/** A token budget; `applies` says in words when, such as `every month from 2026-09`. */
export type TokenBudgetView = Schemas["TokenBudgetView"];
export type TokenBudgetInput = Schemas["TokenBudgetInput"];
export type TokenBudgetReport = Schemas["TokenBudgetReport"];
export type TokenBudgetRow = Schemas["TokenBudgetRow"];
/** One token budget opened down to its tickets, token entries and adjustments. */
export type TokenBudgetDetail = Schemas["TokenBudgetDetail"];
/** Overrun, underrun, or exactly at the token budget. */
export type Outcome = Schemas["Outcome"];
export type TicketActual = Schemas["TicketActual"];
/** One token entry behind a token budget's actual. */
export type TokenBudgetEntry = Schemas["EntryView"];
/** An adjustment to a closed period that counts in a token budget's actual. */
export type TokenBudgetAdjustment = Schemas["AdjustmentLine"];
export type TokenForecastView = Schemas["TokenForecastView"];
export type ForecastMatch = Schemas["ForecastMatch"];
export type AcceptInput = Schemas["AcceptInput"];
export type TokenForecastSettings = Schemas["TokenForecastSettings"];

// Reports (docs: Report > Reading reports, Agent harness reports, Outliers).
/** A report: a tag filter with a grouping, in money or as shares of the period. */
export type CostReport = Schemas["Report"];
export type CostReportRow = Schemas["ReportRow"];
/** Which counting token entries a report shows: `all`, or `approved` only. */
export type ReportStatus = Schemas["StatusView"];
/** `money`, or `share`: each line's share of the period. */
export type MoneyOrShare = Schemas["View"];
export type SavedReport = Schemas["SavedReport"];
/** A report to save; its filter is text, such as `client:acme,team:web`. */
export type SaveInput = Schemas["SaveInput"];
export type Outliers = Schemas["Outliers"];
export type Outlier = Schemas["Outlier"];

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
export type CustomerJourney = Schemas["Journey"];
export type CustomerMember = Schemas["CustomerMember"];
export type CustomerNote = Schemas["Note"];
export type NewsletterView = Schemas["NewsletterView"];
export type Issue = Schemas["IssueView"];
export type IssueText = Schemas["IssueText"];
export type AssistantQuestion = Schemas["QuestionView"];


/** How often token sheets are due (docs: Account > Cadence and token sheets). */
export type CadenceKind = Schemas["CadenceKind"];
/** One stretch of a cadence, from `starts_on` up to and including `ends_on`. */
export type CadencePeriod = Schemas["CadencePeriod"];
/** A cadence for the organization (`project` null) or one project, by project name. */
export type CadenceView = Schemas["CadenceView"];
export type CadenceInput = Schemas["CadenceInput"];

/** One token entry as tables show it, with its splits (docs: Reference > Token entry). */
export type EntryView = Schemas["EntryView"];
/** One split of a token entry: its tags, list value and what placed it. */
export type SplitView = Schemas["SplitView"];
/** Where a token entry stands: draft, submitted, approved, rejected or locked. */
export type TokenEntryStatus = Schemas["EntryStatus"];
export type TokenCounts = Schemas["TokenCounts"];
/** One page of the token entries table. */
export type TokenEntriesPage = Schemas["TokenEntriesPage"];
/** A person's token sheet: the review list, the cadence period and their past token sheets. */
export type TokenSheet = Schemas["TokenSheet"];
export type SheetSummary = Schemas["SheetSummary"];
/** A controller asked everyone to submit before the close. */
export type AskedToSubmit = Schemas["AskedToSubmit"];
/** What a submit did. */
export type Submitted = Schemas["Submitted"];
/** One share of a token entry split by hand, in whole percent. */
export type ShareInput = Schemas["ShareInput"];
export type ApproveInput = Schemas["ApproveInput"];
export type Decided = Schemas["Decided"];
export type Reminded = Schemas["Reminded"];

export type CostKind = "direct" | "project_overhead" | "organization_overhead";

/** Billed, a list-price estimate, or an unknown model. Every figure says which. */
export type ValueLabel = "billed" | "list_estimate" | "unknown_model";

export interface CsvPreview<Row> {
  rows: Row[];
  /** Problems per row, by row number counted from 1. */
  problems: { row: number; message: string }[];
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
