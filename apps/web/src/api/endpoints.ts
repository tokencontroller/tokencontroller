import { ApiError, download, ok, typed } from "./client";
import type {
  AcceptInput,
  AdjustmentInput,
  ApiTokenKind,
  ApproveInput,
  BillCsvQuery,
  BillInput,
  CadenceInput,
  Filter,
  MemberChange,
  NewBillConnection,
  NewMember,
  NewOrganization,
  NewProject,
  NewTicket,
  NewTokenEntry,
  OrganizationChange,
  OrganizationRulesChange,
  PeriodId,
  ProjectChange,
  Rule,
  SaveInput,
  ShareInput,
  TagMapChange,
  Tags,
  NewTicketSystem,
  TicketMapping,
  TokenBudgetInput,
  TokenEntryStatus,
} from "./contract";
import { filterToText } from "@/lib/format";

// Every call the web app makes, one function each. `typed` calls follow the
// service's OpenAPI document; `call` paths are relative to /api/v1.

const path = <T extends string>(id: T) => ({ params: { path: { id } } });
const periodPath = (period: PeriodId) => ({ params: { path: { period } } });
/** A CSV file goes as it is. */
const csvBody = { bodySerializer: (csv: string) => csv, headers: { "content-type": "text/csv" } };

/** What every report can be shown with (docs: Reading reports). */
export interface ReportControls {
  period: PeriodId;
  /** Approved token entries only; for organizations with approval on. */
  approvedOnly?: boolean;
  /** Another currency than the reporting currency, such as `EUR`. */
  currency?: string;
  /** Each line's share of the period instead of money. */
  share?: boolean;
}

/** A report on a tag filter with a grouping, such as cost by team for `client: acme`. */
export interface CostReportQuery extends ReportControls {
  /** A tag name, or `agent_harness`, `agent_name`, `model`, `effort`, `speed`, `day` or `cost_kind`. */
  by?: string;
  filter?: Filter;
}

/** A report's controls as the service's query parameters; the defaults are left out. */
function controls(query: ReportControls) {
  return {
    period: query.period,
    ...(query.approvedOnly ? { status: "approved" as const } : {}),
    ...(query.currency ? { currency: query.currency } : {}),
    ...(query.share ? { view: "share" as const } : {}),
  };
}

/**
 * The data of a `typed` call to a report or list that the service can also
 * send as a CSV file (`format=csv`). Without that parameter it answers JSON.
 */
async function json<T>(request: Promise<{ data?: T; error?: unknown; response: Response }>): Promise<Exclude<T, string>> {
  const data = await ok(request);
  if (typeof data === "string") throw new Error("The service sent a file where a list was expected.");
  return data as Exclude<T, string>;
}

/** A tag filter as the `filter` parameter, such as `client:acme,team:web`; none for the whole organization. */
function filterQuery(filter: Filter | undefined) {
  const text = filterToText(filter ?? {});
  return text ? { filter: text } : {};
}

export const api = {
  // Sign-in. The link in the mail opens a page of the service, which signs
  // the browser in and sends it to the start page.
  signIn: (email: string) => ok(typed.POST("/api/v1/sign-in", { body: { email } })),
  signUp: (email: string, organization: NewOrganization) => ok(typed.POST("/api/v1/sign-up", { body: { email, ...organization } })),
  /** The address behind the link in the "no account" mail; 410 once it no longer works. */
  signUpLink: (token: string) => ok(typed.GET("/api/v1/sign-up/link", { params: { query: { token } } })),
  /** Creates the organization with that link and signs this browser in: no second mail. */
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
  cadences: () => ok(typed.GET("/api/v1/cadences")),
  setCadence: (body: CadenceInput) => ok(typed.PUT("/api/v1/cadences", { body })),
  removeCadence: (id: string) => ok(typed.DELETE("/api/v1/cadences/{id}", path(id))),

  // People
  members: () => ok(typed.GET("/api/v1/members")),
  memberLimit: () => ok(typed.GET("/api/v1/organization/member-limit")),
  addMember: (body: NewMember) => ok(typed.POST("/api/v1/members", { body })),
  updateMember: (id: string, body: MemberChange) => ok(typed.PATCH("/api/v1/members/{id}", { ...path(id), body })),
  setFilters: (id: string, filters: Filter[]) => ok(typed.PUT("/api/v1/members/{id}/filters", { ...path(id), body: filters })),
  resendInvite: (id: string) => ok(typed.POST("/api/v1/members/{id}/invite", path(id))),
  removeMember: (id: string) => ok(typed.DELETE("/api/v1/members/{id}", path(id))),

  // Tags and the tag map
  tagNames: () => ok(typed.GET("/api/v1/tags")),
  addTagName: (name: string) => ok(typed.POST("/api/v1/tags", { body: { name } })),
  removeTagName: (name: string) => ok(typed.DELETE("/api/v1/tags/{name}", { params: { path: { name } } })),
  tagValues: (name: string) => ok(typed.GET("/api/v1/tags/{name}/values", { params: { path: { name } } })),
  mergeTagValues: (name: string, from: string[], into: string) =>
    ok(typed.POST("/api/v1/tags/{name}/merge", { params: { path: { name } }, body: { from, into } })),
  tagMap: () => ok(typed.GET("/api/v1/tag-map")),
  setTagMap: (body: TagMapChange) => ok(typed.PUT("/api/v1/tag-map", { body })),

  // Projects, clients and tickets
  projects: (archived = false) => ok(typed.GET("/api/v1/projects", { params: { query: archived ? { archived } : {} } })),
  createProject: (body: NewProject) => ok(typed.POST("/api/v1/projects", { body })),
  updateProject: (id: string, body: ProjectChange) => ok(typed.PATCH("/api/v1/projects/{id}", { ...path(id), body })),
  archiveProject: (id: string) => ok(typed.DELETE("/api/v1/projects/{id}", path(id))),
  clients: () => ok(typed.GET("/api/v1/clients")),
  tickets: (query: { project_id?: string; parent_id?: string; q?: string } = {}) =>
    ok(typed.GET("/api/v1/tickets", { params: { query: { ...query, limit: 500 } } })),
  createTicket: (body: NewTicket) => ok(typed.POST("/api/v1/tickets", { body })),

  // Token entries, review and approval
  /** The token entries table: submitted, approved and locked unless `status` names others. Never drafts. */
  /** `person` is an agent manager's id: only their token entries. */
  tokenEntries: (query: { period?: PeriodId; filter?: Filter; person?: string; status?: TokenEntryStatus[]; limit?: number; offset?: number }) =>
    json(
      typed.GET("/api/v1/reports/token-entries", {
        params: {
          query: {
            ...(query.period ? { period: query.period } : {}),
            ...filterQuery(query.filter),
            ...(query.person ? { person: query.person } : {}),
            ...(query.status?.length ? { status: query.status.join(",") } : {}),
            ...(query.limit === undefined ? {} : { limit: query.limit }),
            ...(query.offset === undefined ? {} : { offset: query.offset }),
          },
        },
      }),
    ),
  addTokenEntry: (body: NewTokenEntry) => ok(typed.POST("/api/v1/token-entries", { body })),
  /** Reads a CSV file and shows the problem in each row; saves nothing. */
  previewImport: (csv: string) => ok(typed.POST("/api/v1/token-entries/csv/preview", { ...csvBody, body: csv })),
  /** Saves every row as a draft, or nothing when a row cannot be read. */
  importEntries: (csv: string) => ok(typed.POST("/api/v1/token-entries/csv", { ...csvBody, body: csv })),
  /** Sets every tag of one of your own token entries by hand; it becomes one split again. */
  setTags: (id: string, tags: Tags) => ok(typed.PATCH("/api/v1/token-entries/{id}/tags", { ...path(id), body: { tags } })),
  /** Splits one of your own token entries by hand: 2 to 20 whole percents adding up to 100. */
  setSplits: (id: string, splits: ShareInput[]) => ok(typed.PUT("/api/v1/token-entries/{id}/splits", { ...path(id), body: { splits } })),
  /** Submitted token entries the caller may approve, within their filters. */
  approvalQueue: (period: PeriodId, filter: Filter = {}) =>
    json(typed.GET("/api/v1/token-entries/approval", { params: { query: { period, ...filterQuery(filter) } } })),
  approve: (body: ApproveInput) => ok(typed.POST("/api/v1/token-entries/approve", { body })),
  reject: (id: string, reason: string) => ok(typed.POST("/api/v1/token-entries/{id}/reject", { ...path(id), body: { reason } })),
  tokenSheet: () => json(typed.GET("/api/v1/token-sheets/current")),
  /** Submits your drafts here. With none it says there was nothing to report. */
  submit: () => ok(typed.POST("/api/v1/token-sheets/submit", { body: {} })),
  /** A controller asks everyone to submit their token sheets for a month before the close. */
  askToSubmit: (period: PeriodId) => ok(typed.POST("/api/v1/token-sheets/remind", { body: { period } })),

  // Bills, bill connections and reconciliation (controllers)
  bills: (period: PeriodId) => json(typed.GET("/api/v1/bills", { params: { query: { period } } })),
  /** Every revision of one bill, oldest first. */
  billRevisions: (id: string) => json(typed.GET("/api/v1/bills/{id}", path(id))),
  /** `changed` is false when the same figures were already stored. */
  addBill: async (body: BillInput) => {
    const request = typed.POST("/api/v1/bills", { body });
    const bill = await ok(request);
    return { bill, changed: (await request).response.status === 201 };
  },
  /** What a CSV file holds, or its first problem; saves nothing. A cloud's own export needs the period it is the bill for. */
  previewBillCsv: (csv: string, query: BillCsvQuery = {}) =>
    ok(typed.POST("/api/v1/bills/csv/preview", { ...csvBody, body: csv, params: { query } })),
  importBillCsv: (csv: string, query: BillCsvQuery = {}) => ok(typed.POST("/api/v1/bills/csv", { ...csvBody, body: csv, params: { query } })),
  billConnections: () => ok(typed.GET("/api/v1/bill-connections")),
  connectBill: (body: NewBillConnection) => ok(typed.POST("/api/v1/bill-connections", { body })),
  /** The bills stay; they stop updating. */
  disconnectBill: (id: string) => ok(typed.DELETE("/api/v1/bill-connections/{id}", path(id))),
  /** The pull starts within a minute. */
  pullBills: (id: string) => ok(typed.POST("/api/v1/bill-connections/{id}/pull", path(id))),
  reconciliation: (period: PeriodId) => json(typed.GET("/api/v1/reconciliation", { params: { query: { period } } })),

  // Periods, the close, the cost sheet, links for finance and adjustments
  /** People managers and controllers; newest first. */
  periods: () => json(typed.GET("/api/v1/periods")),
  period: (period: PeriodId) => ok(typed.GET("/api/v1/periods/{period}", periodPath(period))),
  /** Closes the period and answers with its cost sheet. */
  close: (period: PeriodId) => ok(typed.POST("/api/v1/periods/{period}/close", periodPath(period))),
  lateSheets: (period: PeriodId) => json(typed.GET("/api/v1/token-sheets/late", { params: { query: { period } } })),
  costSheet: (period: PeriodId) => json(typed.GET("/api/v1/periods/{period}/cost-sheet", periodPath(period))),
  /** The cost sheet as the service writes it in CSV, with extra tag columns. */
  costSheetCsv: (period: PeriodId, columns: string[]) =>
    ok(typed.GET("/api/v1/periods/{period}/cost-sheet", { params: { path: { period }, query: { format: "csv", ...(columns.length ? { columns: columns.join(",") } : {}) } }, parseAs: "blob" })),
  costSheetLinks: (period: PeriodId) => json(typed.GET("/api/v1/periods/{period}/cost-sheet/links", periodPath(period))),
  /** The link's address is in the answer only this once. */
  makeCostSheetLink: (period: PeriodId) => ok(typed.POST("/api/v1/periods/{period}/cost-sheet/links", periodPath(period))),
  revokeCostSheetLink: (id: string) => ok(typed.DELETE("/api/v1/cost-sheet-links/{id}", path(id))),
  /** The cost sheet behind a link, with the organization's name, without signing in. */
  sharedCostSheet: (secret: string) => json(typed.GET("/api/v1/shared/cost-sheets/{secret}", { params: { path: { secret } } })),
  sharedCostSheetCsv: (secret: string) =>
    ok(typed.GET("/api/v1/shared/cost-sheets/{secret}", { params: { path: { secret }, query: { format: "csv" } }, parseAs: "blob" })),
  /** Answers with the cost sheet, the adjustment on it. */
  postAdjustment: (period: PeriodId, body: AdjustmentInput) => ok(typed.POST("/api/v1/periods/{period}/adjustments", { ...periodPath(period), body })),

  // Token budgets and token forecasts. Token budgets are not per period:
  // `applies` says when, and the report compares them for one period.
  tokenBudgets: () => json(typed.GET("/api/v1/token-budgets")),
  addTokenBudget: (body: TokenBudgetInput) => ok(typed.POST("/api/v1/token-budgets", { body })),
  changeTokenBudget: (id: string, body: TokenBudgetInput) => ok(typed.PUT("/api/v1/token-budgets/{id}", { ...path(id), body })),
  removeTokenBudget: (id: string) => ok(typed.DELETE("/api/v1/token-budgets/{id}", path(id))),
  /** Off on an instance without an embeddings model or when a controller switched them off: 409 with the reason. */
  makeTokenForecast: (key: string) => ok(typed.POST("/api/v1/tickets/{key}/token-forecast", { params: { path: { key } } })),
  /** The latest token forecast made for a ticket, as it was made; `null` when none was. */
  tokenForecast: async (key: string) => {
    try {
      return await ok(typed.GET("/api/v1/tickets/{key}/token-forecast", { params: { path: { key } } }));
    } catch (error) {
      if (error instanceof ApiError && error.status === 404) return null;
      throw error;
    }
  },
  acceptTokenForecast: (id: string, body: AcceptInput) => ok(typed.POST("/api/v1/token-forecasts/{id}/accept", { ...path(id), body })),
  tokenForecastSettings: () => ok(typed.GET("/api/v1/token-forecasts/settings")),
  switchTokenForecasts: (enabled: boolean) => ok(typed.PUT("/api/v1/token-forecasts/settings", { body: { enabled } })),

  // Reports. Every report takes the same controls: period, status (approved
  // only, with approval on), another currency, and the share view.
  savedReports: () => json(typed.GET("/api/v1/reports/saved")),
  saveReport: (body: SaveInput) => ok(typed.POST("/api/v1/reports/saved", { body })),
  removeReport: (id: string) => ok(typed.DELETE("/api/v1/reports/saved/{id}", path(id))),
  savedReport: (id: string, query: ReportControls) => json(typed.GET("/api/v1/reports/saved/{id}", { params: { path: { id }, query: controls(query) } })),
  cost: (query: CostReportQuery) =>
    json(typed.GET("/api/v1/reports/cost", { params: { query: { ...controls(query), ...(query.by ? { by: query.by } : {}), ...filterQuery(query.filter) } } })),
  agentHarness: (query: CostReportQuery) =>
    json(typed.GET("/api/v1/reports/agent-harness", { params: { query: { ...controls(query), ...(query.by ? { by: query.by } : {}), ...filterQuery(query.filter) } } })),
  tokenBudgetReport: (query: ReportControls) => json(typed.GET("/api/v1/reports/token-budgets", { params: { query: controls(query) } })),
  tokenBudgetDetail: (id: string, query: ReportControls) => json(typed.GET("/api/v1/reports/token-budgets/{id}", { params: { path: { id }, query: controls(query) } })),
  /** Outliers have no share view: how many times the baseline a ticket cost reads without money. */
  outliers: (query: ReportControls, filter?: Filter) =>
    json(typed.GET("/api/v1/reports/outliers", { params: { query: { ...controls({ ...query, share: false }), ...filterQuery(filter) } } })),

  // Rules: the organization's (controllers change them) and your own. Each
  // list is replaced as a whole, in order.
  organizationRules: () => ok(typed.GET("/api/v1/organization/rules")),
  changeOrganizationRules: (body: OrganizationRulesChange) => ok(typed.PUT("/api/v1/organization/rules", { body })),
  myRules: () => ok(typed.GET("/api/v1/me/rules")),
  setMyRules: (rules: Rule[]) => ok(typed.PUT("/api/v1/me/rules", { body: { rules } })),

  // Settings: API tokens, ticket systems, the raw export
  apiTokens: () => ok(typed.GET("/api/v1/api-tokens")),
  createApiToken: (kind: ApiTokenKind, name: string) => ok(typed.POST("/api/v1/api-tokens", { body: { kind, name } })),
  revokeApiToken: (id: string) => ok(typed.DELETE("/api/v1/api-tokens/{id}", path(id))),
  handOverApiToken: (id: string, person_id: string) => ok(typed.POST("/api/v1/api-tokens/{id}/hand-over", { ...path(id), body: { person_id } })),
  ticketSystems: () => ok(typed.GET("/api/v1/ticket-systems")),
  connectTicketSystem: (body: NewTicketSystem) => ok(typed.POST("/api/v1/ticket-systems", { body })),
  disconnectTicketSystem: (id: string) => ok(typed.DELETE("/api/v1/ticket-systems/{id}", path(id))),
  /** The projects the ticket system has, to map onto ours. */
  externalProjects: (id: string) => ok(typed.GET("/api/v1/ticket-systems/{id}/projects", path(id))),
  mapProjects: (id: string, mappings: TicketMapping[]) => ok(typed.PUT("/api/v1/ticket-systems/{id}/mappings", { ...path(id), body: mappings })),
  syncTickets: (id: string) => ok(typed.POST("/api/v1/ticket-systems/{id}/sync", path(id))),
  /** Everything the organization has, as a zip of JSON files. Controllers only. */
  rawExport: () => download("/exports/raw"),
};
