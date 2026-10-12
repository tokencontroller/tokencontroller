import { call, download, ok, typed } from "./client";
import type {
  Adjustment,
  ApiTokenKind,
  Bill,
  BillConnection,
  BudgetVsActualRow,
  CadenceInput,
  CadenceView,
  CloseCheck,
  CostSheet,
  CsvPreview,
  Filter,
  HarnessCut,
  Interval,
  MemberChange,
  NewAdjustment,
  NewBill,
  NewMember,
  NewProject,
  NewRule,
  NewTicket,
  NewTokenBudget,
  NewTokenEntry,
  OrganizationChange,
  Outlier,
  Period,
  PeriodId,
  Project,
  ProjectChange,
  Reconciliation,
  ReportQuery,
  ReportResult,
  Rule,
  RuleSettings,
  SavedReport,
  SubmitResult,
  TagMapChange,
  TagName,
  Tags,
  Ticket,
  TicketSystem,
  TokenBudget,
  TokenEntry,
  TokenEntryStatus,
  TokenForecast,
  TokenSheet,
} from "./contract";
import { filterToText } from "@/lib/format";

// Every call the web app makes, one function each. `typed` calls follow the
// service's OpenAPI document; `call` paths are relative to /api/v1.

const enc = encodeURIComponent;
const path = <T extends string>(id: T) => ({ params: { path: { id } } });

export const api = {
  // Sign-in. The link in the mail opens a page of the service, which signs
  // the browser in and sends it to the start page.
  signIn: (email: string) => ok(typed.POST("/api/v1/sign-in", { body: { email } })),
  signUp: (email: string, organization_name: string) => ok(typed.POST("/api/v1/sign-up", { body: { email, organization_name } })),
  signOut: () => ok(typed.POST("/api/v1/sign-out")),
  invite: (code: string) => ok(typed.GET("/api/v1/invites/{code}", { params: { path: { code } } })),
  health: () => ok(typed.GET("/api/v1/health")),

  // Who is signed in, the organization and its plan
  me: () => ok(typed.GET("/api/v1/me")),
  organization: () => ok(typed.GET("/api/v1/organization")),
  updateOrganization: (body: OrganizationChange) => ok(typed.PATCH("/api/v1/organization", { body })),
  /** 404 where billing is off: self-hosted. */
  billing: () => ok(typed.GET("/api/v1/billing")),
  checkout: (interval: Interval, country: string) => ok(typed.POST("/api/v1/billing/checkout", { body: { interval, country } })),
  billingPortal: () => ok(typed.POST("/api/v1/billing/portal")),
  ruleSettings: () => call<RuleSettings>("GET", "/rules/settings"),
  updateRuleSettings: (body: Partial<RuleSettings>) => call<RuleSettings>("PATCH", "/rules/settings", { body }),
  cadences: () => call<CadenceView[]>("GET", "/cadences"),
  setCadence: (body: CadenceInput) => call<CadenceView>("PUT", "/cadences", { body }),
  removeCadence: (id: string) => call<undefined>("DELETE", `/cadences/${enc(id)}`),

  // People
  members: () => ok(typed.GET("/api/v1/members")),
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
    call<TagName>("POST", `/tags/${enc(name)}/merge`, { body: { from, into } }),
  tagMap: () => ok(typed.GET("/api/v1/tag-map")),
  setTagMap: (body: TagMapChange) => ok(typed.PUT("/api/v1/tag-map", { body })),

  // Projects, clients and tickets. Their list filters go as a query string.
  projects: (archived = false) => call<Project[]>("GET", "/projects", { query: { archived: archived || undefined } }),
  createProject: (body: NewProject) => ok(typed.POST("/api/v1/projects", { body })),
  updateProject: (id: string, body: ProjectChange) => ok(typed.PATCH("/api/v1/projects/{id}", { ...path(id), body })),
  archiveProject: (id: string) => ok(typed.DELETE("/api/v1/projects/{id}", path(id))),
  clients: () => ok(typed.GET("/api/v1/clients")),
  tickets: (query: { project_id?: string; parent_id?: string; q?: string } = {}) =>
    call<Ticket[]>("GET", "/tickets", { query: { ...query, limit: 500 } }),
  createTicket: (body: NewTicket) => ok(typed.POST("/api/v1/tickets", { body })),

  // Token entries, review and approval
  tokenEntries: (query: { period: PeriodId; status?: TokenEntryStatus | ""; filter?: Filter; person?: string }) =>
    call<TokenEntry[]>("GET", "/token-entries", {
      query: { period: query.period, status: query.status, person: query.person, filter: filterToText(query.filter ?? {}) },
    }),
  addTokenEntry: (body: NewTokenEntry) => call<TokenEntry>("POST", "/token-entries", { body }),
  previewImport: (csv: string) =>
    call<CsvPreview<NewTokenEntry>>("POST", "/token-entries/import/preview", { body: csv, contentType: "text/csv" }),
  importEntries: (csv: string) =>
    call<{ entries: number }>("POST", "/token-entries/import", { body: csv, contentType: "text/csv" }),
  setTags: (id: string, tags: Tags) => call<TokenEntry>("PATCH", `/token-entries/${enc(id)}/tags`, { body: { tags } }),
  setSplits: (id: string, splits: { share: number; tags: Tags }[]) =>
    call<TokenEntry>("PUT", `/token-entries/${enc(id)}/splits`, { body: { splits } }),
  approve: (ids: string[]) => call<{ approved: number }>("POST", "/token-entries/approve", { body: { ids } }),
  reject: (id: string, reason: string) => call<TokenEntry>("POST", `/token-entries/${enc(id)}/reject`, { body: { reason } }),
  tokenSheet: () => call<TokenSheet>("GET", "/token-sheets/current"),
  submit: () => call<SubmitResult>("POST", "/token-sheets/submit"),
  askToSubmit: (period: PeriodId) => call<{ reminded: number }>("POST", "/token-sheets/remind", { body: { period } }),

  // Bills and reconciliation
  bills: (period: PeriodId) => call<Bill[]>("GET", "/bills", { query: { period } }),
  addBill: (body: NewBill) => call<Bill>("POST", "/bills", { body }),
  previewBillCsv: (csv: string) => call<CsvPreview<NewBill>>("POST", "/bills/import/preview", { body: csv, contentType: "text/csv" }),
  importBillCsv: (csv: string) => call<Bill[]>("POST", "/bills/import", { body: csv, contentType: "text/csv" }),
  billConnections: () => call<BillConnection[]>("GET", "/bill-connections"),
  connectBill: (provider: BillConnection["provider"], key: string) =>
    call<BillConnection>("POST", "/bill-connections", { body: { provider, key } }),
  pullBills: (id: string) => call<BillConnection>("POST", `/bill-connections/${enc(id)}/pull`),
  reconciliation: (period: PeriodId) => call<Reconciliation>("GET", "/reconciliation", { query: { period } }),

  // Periods, the close, the cost sheet and adjustments
  periods: () => call<Period[]>("GET", "/periods"),
  closeCheck: (period: PeriodId) => call<CloseCheck>("GET", `/periods/${enc(period)}/close-check`),
  close: (period: PeriodId) => call<Period>("POST", `/periods/${enc(period)}/close`),
  costSheet: (period: PeriodId) => call<CostSheet>("GET", `/periods/${enc(period)}/cost-sheet`),
  costSheetCsv: (period: PeriodId, columns: string[]) =>
    download(`/periods/${enc(period)}/cost-sheet.csv`, { columns: columns.join(",") }),
  shareCostSheet: (period: PeriodId) => call<{ url: string }>("POST", `/periods/${enc(period)}/cost-sheet/link`),
  sharedCostSheet: (token: string) => call<CostSheet & { organization: string }>("GET", `/shared/cost-sheets/${enc(token)}`),
  postAdjustment: (period: PeriodId, body: NewAdjustment) =>
    call<Adjustment>("POST", `/periods/${enc(period)}/adjustments`, { body }),

  // Token budgets and forecasts
  budgets: (period: PeriodId) => call<TokenBudget[]>("GET", "/budgets", { query: { period } }),
  addBudget: (body: NewTokenBudget) => call<TokenBudget>("POST", "/budgets", { body }),
  removeBudget: (id: string) => call<undefined>("DELETE", `/budgets/${enc(id)}`),
  forecast: (ticket: string) => call<TokenForecast>("GET", `/forecasts/${enc(ticket)}`),

  // Reports
  savedReports: () => call<SavedReport[]>("GET", "/reports/saved"),
  saveReport: (body: Omit<SavedReport, "id">) => call<SavedReport>("POST", "/reports/saved", { body }),
  removeReport: (id: string) => call<undefined>("DELETE", `/reports/saved/${enc(id)}`),
  cost: (query: ReportQuery) =>
    call<ReportResult>("GET", "/reports/cost", {
      query: {
        period: query.period,
        by: query.group_by,
        filter: filterToText(query.filter),
        currency: query.currency,
        approved_only: query.approved_only ? "true" : undefined,
      },
    }),
  budgetVsActual: (period: PeriodId) => call<BudgetVsActualRow[]>("GET", "/reports/budget-vs-actual", { query: { period } }),
  agentHarness: (period: PeriodId, by: HarnessCut) =>
    call<ReportResult>("GET", "/reports/agent-harness", { query: { period, by } }),
  outliers: (period: PeriodId) => call<Outlier[]>("GET", "/reports/outliers", { query: { period } }),

  // Rules
  rules: () => call<Rule[]>("GET", "/rules"),
  addRule: (scope: Rule["scope"], body: NewRule) => call<Rule>("POST", "/rules", { body: { ...body, scope } }),
  removeRule: (id: string) => call<undefined>("DELETE", `/rules/${enc(id)}`),
  orderRules: (ids: string[]) => call<Rule[]>("PUT", "/rules/order", { body: { ids } }),

  // Settings: API tokens, ticket systems, the raw export
  apiTokens: () => ok(typed.GET("/api/v1/api-tokens")),
  createApiToken: (kind: ApiTokenKind, name: string) => ok(typed.POST("/api/v1/api-tokens", { body: { kind, name } })),
  revokeApiToken: (id: string) => ok(typed.DELETE("/api/v1/api-tokens/{id}", path(id))),
  handOverApiToken: (id: string, person_id: string) => ok(typed.POST("/api/v1/api-tokens/{id}/hand-over", { ...path(id), body: { person_id } })),
  ticketSystems: () => call<TicketSystem[]>("GET", "/ticket-systems"),
  connectTicketSystem: (body: { kind: TicketSystem["kind"]; site: string; token: string }) =>
    call<TicketSystem>("POST", "/ticket-systems", { body }),
  syncTickets: (id: string) => call<TicketSystem>("POST", `/ticket-systems/${enc(id)}/sync`),
  rawExport: () => download("/export/raw"),

  // Connecting a computer (`tc connect`, a device sign-in)
};
