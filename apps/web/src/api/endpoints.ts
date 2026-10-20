import { download, ok, typed } from "./client";
import type { components } from "./generated";
import type {
  ApiTokenKind,
  BillCloud,
  BillInput,
  Filter,
  MemberChange,
  NewAttributeValue,
  NewBillConnection,
  NewEstimate,
  NewGrant,
  NewMember,
  NewOrganization,
  NewPersonAttribute,
  NewPriceRow,
  NewRule,
  NewSavedReport,
  NewShare,
  NewTagValue,
  NewTicketSystem,
  OrganizationChange,
  PartInput,
  Tags,
  TicketMapping,
} from "./contract";
import { filterToText } from "@/lib/format";

// Every call the web app makes, one function each, typed by the service's
// OpenAPI document (apps/server/openapi.json).

type WireTags = components["schemas"]["Tags"];
/** Tags as the document types them: an object of strings it does not spell out. */
const wire = (tags: Tags): WireTags => tags;
const path = <T extends string>(id: T) => ({ params: { path: { id } } });

/**
 * The data of a call that can also answer a CSV file (`format=csv`). Without
 * that parameter the service answers JSON.
 */
async function json<T>(request: Promise<{ data?: T; error?: unknown; response: Response }>): Promise<Exclude<T, string>> {
  const data = await ok(request);
  if (typeof data === "string") throw new Error("The service sent a file where a list was expected.");
  return data as Exclude<T, string>;
}

/** A filter as the `filter` parameter, such as `client:acme,team:web`; none for the whole organization. */
function filterQuery(filter: Filter | undefined) {
  const text = filterToText(filter ?? {});
  return text ? { filter: text } : {};
}

/** What kind of CSV bill file is sent: the bill schema's columns, or a cloud's own export for one month. */
export interface BillCsvFile {
  cloud?: BillCloud | undefined;
  /** The month a cloud's export is the bill for, `YYYY-MM`. */
  month?: string | undefined;
}

/** A CSV file as the bills endpoints take it: the text as-is, with its content type. */
function csvBody(file: BillCsvFile, text: string) {
  return {
    params: { query: { ...(file.cloud ? { cloud: file.cloud } : {}), ...(file.month ? { month: file.month } : {}) } },
    body: text,
    bodySerializer: (body: string) => body,
    headers: { "content-type": "text/csv" },
  };
}

/** What every report and list of messages takes (docs: Reading reports). */
export interface ReportQuery {
  from: string;
  to: string;
  /** The moment up to which the log is read; left out, now. */
  asOf?: string | undefined;
  /** A tag or attribute name, or `person`, `label`, `agent_harness`, `agent_name`, `model` or `day`. */
  by?: string | undefined;
  filter?: Filter | undefined;
  /** Approved messages only, with approval on. */
  approved?: boolean | undefined;
  /** Each row's share of the total instead of money. */
  share?: boolean | undefined;
  currency?: string | undefined;
}

function reportQuery(query: ReportQuery) {
  return {
    from: query.from,
    to: query.to,
    ...(query.asOf ? { as_of: query.asOf } : {}),
    ...(query.by ? { by: query.by } : {}),
    ...filterQuery(query.filter),
    ...(query.approved ? { approved: true } : {}),
    ...(query.share ? { view: "share" } : {}),
    ...(query.currency ? { currency: query.currency } : {}),
  };
}

export interface SessionQuery {
  from?: string;
  to?: string;
  asOf?: string;
  filter?: Filter;
  unassigned?: boolean;
  sessionId?: string;
}

function sessionQuery(query: SessionQuery) {
  return {
    ...(query.from ? { from: query.from } : {}),
    ...(query.to ? { to: query.to } : {}),
    ...(query.asOf ? { as_of: query.asOf } : {}),
    ...filterQuery(query.filter),
    ...(query.unassigned ? { unassigned: true } : {}),
    ...(query.sessionId ? { session_id: query.sessionId } : {}),
  };
}

export const api = {
  // Sign-in. The link in the mail opens a page of the service, which signs
  // the browser in and sends it to the start page.
  signIn: (email: string) => ok(typed.POST("/api/v1/sign-in", { body: { email } })),
  signUp: (email: string, organization: NewOrganization) => ok(typed.POST("/api/v1/sign-up", { body: { email, ...organization } })),
  /** The address behind the link in the "no account" mail; 410 once it no longer works. */
  signUpLink: (token: string) => ok(typed.GET("/api/v1/sign-up/link", { params: { query: { token } } })),
  signUpWithLink: (token: string, organization: NewOrganization) => ok(typed.POST("/api/v1/sign-up/link", { body: { token, ...organization } })),
  signOut: () => ok(typed.POST("/api/v1/sign-out")),
  /** Ways to sign in besides the magic link; empty on the open edition. */
  signInProviders: () => ok(typed.GET("/api/v1/sign-in/providers")),
  invite: (code: string) => ok(typed.GET("/api/v1/invites/{code}", { params: { path: { code } } })),
  health: () => ok(typed.GET("/api/v1/health")),

  // Who is signed in and the organization
  me: () => ok(typed.GET("/api/v1/me")),
  organization: () => ok(typed.GET("/api/v1/organization")),
  updateOrganization: (body: OrganizationChange) => ok(typed.PATCH("/api/v1/organization", { body })),

  // People, their attributes and grants
  members: () => ok(typed.GET("/api/v1/members")),
  memberLimit: () => ok(typed.GET("/api/v1/organization/member-limit")),
  addMember: (body: NewMember) => ok(typed.POST("/api/v1/members", { body })),
  updateMember: (id: string, body: MemberChange) => ok(typed.PATCH("/api/v1/members/{id}", { ...path(id), body })),
  resendInvite: (id: string) => ok(typed.POST("/api/v1/members/{id}/invite", path(id))),
  removeMember: (id: string) => ok(typed.DELETE("/api/v1/members/{id}", path(id))),
  attributes: () => ok(typed.GET("/api/v1/attributes")),
  addAttributeName: (name: string) => ok(typed.POST("/api/v1/attributes", { body: { name } })),
  addAttributeValue: (name: string, body: Omit<NewAttributeValue, "defaults"> & { defaults?: Tags }) =>
    ok(typed.POST("/api/v1/attributes/{name}/values", { params: { path: { name } }, body: { ...body, defaults: wire(body.defaults ?? {}) } })),
  setPersonAttribute: (id: string, body: NewPersonAttribute) => ok(typed.POST("/api/v1/people/{id}/attributes", { ...path(id), body })),
  grants: () => ok(typed.GET("/api/v1/grants")),
  addGrant: (body: Omit<NewGrant, "filter"> & { filter: Filter }) => ok(typed.POST("/api/v1/grants", { body: { ...body, filter: wire(body.filter) } })),

  // Tags
  tags: () => ok(typed.GET("/api/v1/tags")),
  addTagName: (name: string) => ok(typed.POST("/api/v1/tags", { body: { name } })),
  removeTagName: (name: string) => ok(typed.DELETE("/api/v1/tags/{name}", { params: { path: { name } } })),
  addTagValue: (name: string, body: Omit<NewTagValue, "defaults"> & { defaults?: Tags }) =>
    ok(typed.POST("/api/v1/tags/{name}/values", { params: { path: { name } }, body: { ...body, defaults: wire(body.defaults ?? {}) } })),

  // Messages, sessions, splits and submissions
  sessions: (query: SessionQuery = {}) => ok(typed.GET("/api/v1/sessions", { params: { query: sessionQuery(query) } })),
  messages: (query: SessionQuery = {}) => ok(typed.GET("/api/v1/messages", { params: { query: sessionQuery(query) } })),
  /** Supersedes the earlier split on the same session or message. */
  split: (sessionId: string, parts: { fraction: string; tags: Tags }[]) =>
    ok(typed.POST("/api/v1/splits", { body: { session_id: sessionId, parts: parts.map((p): PartInput => ({ fraction: p.fraction, tags: wire(p.tags) })) } })),
  submissions: () => ok(typed.GET("/api/v1/submissions")),
  /** Submits your messages since your last submit, up to today. */
  submit: () => ok(typed.POST("/api/v1/submissions", { body: {} })),
  approve: (id: string) => ok(typed.POST("/api/v1/submissions/{id}/approve", path(id))),
  reject: (id: string, reason: string) => ok(typed.POST("/api/v1/submissions/{id}/reject", { ...path(id), body: { reason } })),
  submitRequests: () => ok(typed.GET("/api/v1/submit-requests")),
  askToSubmit: (until: string, message?: string) => ok(typed.POST("/api/v1/submit-requests", { body: { until, ...(message ? { message } : {}) } })),

  // Reversal of any event: rules, grants, price rows, bills, splits
  reverse: (eventId: string) => ok(typed.POST("/api/v1/reversals", { body: { event_id: eventId } })),
  events: (kind: string) => ok(typed.GET("/api/v1/events", { params: { query: { kind } } })),

  // Plans and estimates
  estimate: (id: string) => ok(typed.GET("/api/v1/estimates/{id}", path(id))),
  addEstimate: (body: Omit<NewEstimate, "tags"> & { tags: Tags }) => ok(typed.POST("/api/v1/estimates", { body: { ...body, tags: wire(body.tags) } })),
  accept: (id: string) => ok(typed.POST("/api/v1/estimates/{id}/accept", path(id))),
  shares: () => ok(typed.GET("/api/v1/shares")),
  addShare: (body: NewShare) => ok(typed.POST("/api/v1/shares", { body })),

  // Cutoff and price table
  cutoff: () => ok(typed.GET("/api/v1/cutoff")),
  moveCutoff: (cutoff: string) => ok(typed.POST("/api/v1/cutoff", { body: { cutoff } })),
  priceTable: () => ok(typed.GET("/api/v1/price-table")),
  addPriceRow: (body: NewPriceRow) => ok(typed.POST("/api/v1/price-table/rows", { body })),

  // Bills and reconciliation (org managers)
  bills: () => json(typed.GET("/api/v1/bills")),
  addBill: (body: BillInput) => ok(typed.POST("/api/v1/bills", { body })),
  billConnections: () => ok(typed.GET("/api/v1/bill-connections")),
  connectBill: (body: NewBillConnection) => ok(typed.POST("/api/v1/bill-connections", { body })),
  disconnectBill: (id: string) => ok(typed.DELETE("/api/v1/bill-connections/{id}", path(id))),
  pullBills: (id: string) => ok(typed.POST("/api/v1/bill-connections/{id}/pull", path(id))),
  reconciliation: () => json(typed.GET("/api/v1/reconciliation")),
  // A CSV bill file: what it holds before anything is saved, then the upload
  previewBillCsv: (file: BillCsvFile, text: string) => ok(typed.POST("/api/v1/bills/csv/preview", { ...csvBody(file, text) })),
  uploadBillCsv: (file: BillCsvFile, text: string) => ok(typed.POST("/api/v1/bills/csv", { ...csvBody(file, text) })),

  // Reports: the cost of a filter over a range, as of a date
  cost: (query: ReportQuery) => json(typed.GET("/api/v1/reports/cost", { params: { query: reportQuery(query) } })),
  deviation: (query: ReportQuery) => json(typed.GET("/api/v1/reports/deviation", { params: { query: reportQuery(query) } })),
  forecast: (query: ReportQuery) => json(typed.GET("/api/v1/reports/forecast", { params: { query: reportQuery(query) } })),
  savedReports: () => ok(typed.GET("/api/v1/reports/saved")),
  saveReport: (body: Omit<NewSavedReport, "filter"> & { filter: Filter }) => ok(typed.POST("/api/v1/reports/saved", { body })),

  // Rules: the organization's and your own
  rules: () => ok(typed.GET("/api/v1/rules")),
  addRule: (body: NewRule) => ok(typed.POST("/api/v1/rules", { body })),
  myRules: () => ok(typed.GET("/api/v1/me/rules")),
  setMyRules: (rules: NewRule[]) => ok(typed.PUT("/api/v1/me/rules", { body: rules })),

  // Settings: API tokens, Jira, the raw export
  apiTokens: () => ok(typed.GET("/api/v1/api-tokens")),
  createApiToken: (kind: ApiTokenKind, name: string) => ok(typed.POST("/api/v1/api-tokens", { body: { kind, name } })),
  revokeApiToken: (id: string) => ok(typed.DELETE("/api/v1/api-tokens/{id}", path(id))),
  ticketSystems: () => ok(typed.GET("/api/v1/ticket-systems")),
  connectTicketSystem: (body: NewTicketSystem) => ok(typed.POST("/api/v1/ticket-systems", { body })),
  disconnectTicketSystem: (id: string) => ok(typed.DELETE("/api/v1/ticket-systems/{id}", path(id))),
  externalProjects: (id: string) => ok(typed.GET("/api/v1/ticket-systems/{id}/projects", path(id))),
  mapProjects: (id: string, mappings: TicketMapping[]) => ok(typed.PUT("/api/v1/ticket-systems/{id}/mappings", { ...path(id), body: mappings })),
  syncTickets: (id: string) => ok(typed.POST("/api/v1/ticket-systems/{id}/sync", path(id))),
  /** The whole log as a zip of JSON files. */
  rawExport: () => download("/exports/raw"),
};
