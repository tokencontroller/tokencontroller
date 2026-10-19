import type { components } from "./generated";

// The types the screens use, as aliases of the service's OpenAPI document
// (ADR 0009). Nothing here is written by hand except `Tags` and `Money`.

type Schemas = components["schemas"];

/**
 * Tag name to tag value, such as `{ "feature": "KD-1" }`. The document
 * describes `Tags` as an object without saying its values are strings.
 */
export type Tags = Record<string, string>;
/** A filter is a set of tags and attributes; empty is the whole organization. */
export type Filter = Tags;

/** An amount as the web app adds it up: nano-units with a currency (src/lib/money.ts). */
export interface Money {
  /** Nano-units: one billionth of a unit. A JSON number or a decimal string. */
  nanos: number | string;
  /** ISO 4217 code, such as `USD`. */
  currency: string;
}

/** The service answers with tags typed as an empty object; read them as tags. */
export function tagsOf(value: unknown): Tags {
  return (value ?? {}) as Tags;
}

export type Role = Schemas["Role"];
export type Manages = Schemas["Manages"];
export type Me = Schemas["Me"];
export type Member = Schemas["MemberView"];
export type MemberChange = Schemas["MemberChange"];
export type NewMember = Schemas["NewMember"];
export type MemberLimit = Schemas["MemberLimitView"];
export type Organization = Schemas["Organization"];
export type OrganizationChange = Schemas["OrganizationChange"];
export type NewOrganization = Schemas["NewOrganization"];
export type SignUpRequest = Schemas["SignUpRequest"];
export type SignUpWithLink = Schemas["SignUpWithLink"];

export type Message = Schemas["MessageView"];
export type Session = Schemas["SessionView"];
export type Part = Schemas["PartView"];
export type PartInput = Schemas["PartInput"];
export type NewSplit = Schemas["NewSplit"];
export type Split = Schemas["SplitView"];
export type Submitted = Schemas["Submitted"];

export type TagName = Schemas["TagNameView"];
export type TagValue = Schemas["TagValueView"];
export type NewTagValue = Schemas["NewTagValue"];
export type AttributeName = Schemas["AttributeNameView"];
export type AttributeValue = Schemas["AttributeValueView"];
export type NewAttributeValue = Schemas["NewAttributeValue"];
export type NewPersonAttribute = Schemas["NewPersonAttribute"];
export type PersonAttribute = Schemas["PersonAttributeView"];

export type Grant = Schemas["GrantView"];
export type NewGrant = Schemas["NewGrant"];

export type Estimate = Schemas["EstimateView"];
export type NewEstimate = Schemas["NewEstimate"];
export type Band = Schemas["BandView"];
export type Share = Schemas["ShareView"];
export type NewShare = Schemas["NewShare"];

export type Cutoff = Schemas["CutoffView"];
export type Submission = Schemas["SubmissionView"];
export type SubmitRequest = Schemas["SubmitRequestView"];

export type PriceTable = Schemas["PriceTableView"];
export type PriceRow = Schemas["PriceRowView"];
export type NewPriceRow = Schemas["NewPriceRow"];

export type Bill = Schemas["BillView"];
export type BillInput = Schemas["BillInput"];
export type BillApi = Schemas["BillApi"];
export type BillConnection = Schemas["BillConnectionView"];
export type NewBillConnection = Schemas["NewBillConnection"];
export type Reconciliation = Schemas["ReconciliationRow"];

export type CostReport = Schemas["CostReport"];
export type CostRow = Schemas["CostRow"];
export type DeviationReport = Schemas["DeviationReport"];
export type DeviationRow = Schemas["DeviationRow"];
export type ForecastReport = Schemas["ForecastReport"];
export type ForecastRow = Schemas["ForecastRow"];
export type SavedReport = Schemas["SavedReport"];
export type NewSavedReport = Schemas["NewSavedReport"];

export type Rule = Schemas["RuleView"];
export type NewRule = Schemas["NewRule"];
export type EventView = Schemas["EventView"];

export type ApiToken = Schemas["ApiToken"];
export type ApiTokenKind = Schemas["TokenKind"];
export type CreatedApiToken = Schemas["CreatedApiToken"];
export type TicketSystem = Schemas["TicketSystemView"];
export type NewTicketSystem = Schemas["NewTicketSystem"];
export type TicketMapping = Schemas["Mapping"];
export type ExternalProject = Schemas["ExternalProject"];

/** Who is signed in, with their organization's settings. */
export interface Account {
  person: Member;
  organization: Organization;
  credential: Me["credential"];
}
