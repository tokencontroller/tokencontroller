import { call, download, ok, typed } from "./client";
import type {
  Adjustment,
  ApiTokenKind,
  Bill,
  BillConnection,
  BudgetVsActualRow,
  CadenceInput,
  CloseCheck,
  CostSheet,
  CsvPreview,
  FeedbackStatus,
  Filter,
  HarnessCut,
  Interval,
  IssueText,
  MemberChange,
  NewAdjustment,
  NewBill,
  NewMember,
  NewProject,
  NewReport,
  NewTicket,
  NewTokenBudget,
  NewTokenEntry,
  OrganizationChange,
  OrganizationRulesChange,
  Outlier,
  Period,
  PeriodId,
  ProjectChange,
  Reconciliation,
  ReportChange,
  ReportQuery,
  ReportResult,
  Rule,
  SavedReport,
  SubmitResult,
  TagMapChange,
  Tags,
  NewTicketSystem,
  TicketMapping,
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
/** A CSV file goes as it is. */
const csvBody = { bodySerializer: (csv: string) => csv, headers: { "content-type": "text/csv" } };

export const api = {
  // Sign-in. The link in the mail opens a page of the service, which signs
  // the browser in and sends it to the start page.
  signIn: (email: string) => ok(typed.POST("/api/v1/sign-in", { body: { email } })),
  signUp: (email: string, organization_name: string, name: string) =>
    ok(typed.POST("/api/v1/sign-up", { body: { email, organization_name, ...(name ? { name } : {}) } })),
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
  cadences: () => ok(typed.GET("/api/v1/cadences")),
  setCadence: (body: CadenceInput) => ok(typed.PUT("/api/v1/cadences", { body })),
  removeCadence: (id: string) => ok(typed.DELETE("/api/v1/cadences/{id}", path(id))),

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
  tokenEntries: (query: { period: PeriodId; status?: TokenEntryStatus | ""; filter?: Filter; person?: string }) =>
    call<TokenEntry[]>("GET", "/token-entries", {
      query: { period: query.period, status: query.status, person: query.person, filter: filterToText(query.filter ?? {}) },
    }),
  addTokenEntry: (body: NewTokenEntry) => ok(typed.POST("/api/v1/token-entries", { body })),
  /** Reads a CSV file and shows the problem in each row; saves nothing. */
  previewImport: (csv: string) => ok(typed.POST("/api/v1/token-entries/csv/preview", { ...csvBody, body: csv })),
  /** Saves every row as a draft, or nothing when a row cannot be read. */
  importEntries: (csv: string) => ok(typed.POST("/api/v1/token-entries/csv", { ...csvBody, body: csv })),
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

  // Rules: the organization's (controllers change them) and your own. Each
  // list is replaced as a whole, in order.
  organizationRules: () => ok(typed.GET("/api/v1/organization/rules")),
  changeOrganizationRules: (body: OrganizationRulesChange) => ok(typed.PUT("/api/v1/organization/rules", { body })),
  myRules: () => ok(typed.GET("/api/v1/me/rules")),
  setMyRules: (rules: Rule[]) => ok(typed.PUT("/api/v1/me/rules", { body: { rules } })),

  // Feedback reports and support inside the app (every member)
  feedback: () => ok(typed.GET("/api/v1/feedback")),
  sendFeedback: (body: NewReport) => ok(typed.POST("/api/v1/feedback", { body })),
  /** 404 where support is off, as on most self-hosted instances. */
  support: () => ok(typed.GET("/api/v1/support")),
  sendSupportMessage: (message: string) => ok(typed.POST("/api/v1/support/messages", { body: { message } })),

  // The admin area: 404 to everyone who is not an operator
  adminOverview: () => ok(typed.GET("/api/v1/admin")),
  supportConversations: (state?: string) => ok(typed.GET("/api/v1/admin/support", { params: { query: state ? { state } : {} } })),
  supportConversation: (id: string) => ok(typed.GET("/api/v1/admin/support/{id}", path(id))),
  replyToSupport: (id: string, text: string) => ok(typed.POST("/api/v1/admin/support/{id}/replies", { ...path(id), body: { text } })),
  closeSupport: (id: string) => ok(typed.POST("/api/v1/admin/support/{id}/close", path(id))),
  receivedFeedback: (status?: FeedbackStatus) => ok(typed.GET("/api/v1/admin/feedback", { params: { query: status ? { status } : {} } })),
  receivedReport: (id: string) => ok(typed.GET("/api/v1/admin/feedback/{id}", path(id))),
  updateReport: (id: string, body: ReportChange) => ok(typed.PATCH("/api/v1/admin/feedback/{id}", { ...path(id), body })),
  customers: () => ok(typed.GET("/api/v1/admin/customers")),
  customer: (id: string) => ok(typed.GET("/api/v1/admin/customers/{id}", path(id))),
  addCustomerNote: (id: string, text: string) => ok(typed.POST("/api/v1/admin/customers/{id}/notes", { ...path(id), body: { text } })),
  assistantQuestions: (answered?: boolean) => ok(typed.GET("/api/v1/admin/assistant/questions", { params: { query: answered === undefined ? {} : { answered } } })),
  newsletter: () => ok(typed.GET("/api/v1/admin/newsletter")),
  createIssue: (subject: string, body: string) => ok(typed.POST("/api/v1/admin/newsletter/issues", { body: { subject, body } })),
  updateIssue: (id: string, body: IssueText) => ok(typed.PATCH("/api/v1/admin/newsletter/issues/{id}", { ...path(id), body })),
  sendIssue: (id: string) => ok(typed.POST("/api/v1/admin/newsletter/issues/{id}/send", path(id))),

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
