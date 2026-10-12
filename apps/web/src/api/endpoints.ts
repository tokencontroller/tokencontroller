import { call, download } from "./client";
import type {
  Adjustment,
  ApiToken,
  ApiTokenKind,
  Bill,
  BillConnection,
  BudgetVsActualRow,
  Cadence,
  CheckoutSession,
  CliAuthorization,
  CloseCheck,
  CostSheet,
  CsvPreview,
  Filter,
  HarnessCut,
  Me,
  NewAdjustment,
  NewApiTokenResult,
  NewBill,
  NewRule,
  NewTokenBudget,
  NewTokenEntry,
  Organization,
  Outlier,
  Period,
  PeriodId,
  Person,
  Project,
  Reconciliation,
  ReportQuery,
  ReportResult,
  Role,
  Rule,
  SavedReport,
  SignUp,
  SubmitResult,
  TagMap,
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

// Every call the web app makes, one function each. Paths are relative to /api/v1.

const enc = encodeURIComponent;

export const api = {
  // Sign-in and the organization
  me: () => call<Me>("GET", "/me"),
  /** `next` is the page the link opens after sign-in, such as /cli/WDJB-MJHT. */
  signIn: (email: string, next?: string) => call<undefined>("POST", "/sign-in", { body: { email, ...(next ? { next } : {}) } }),
  verify: (token: string) => call<Me>("POST", "/sign-in/verify", { body: { token } }),
  signOut: () => call<undefined>("POST", "/sign-out"),
  signUp: (body: SignUp) => call<undefined>("POST", "/sign-up", { body }),
  updateOrganization: (body: Partial<Omit<Organization, "id" | "plan" | "billing" | "members" | "member_limit">>) =>
    call<Organization>("PATCH", "/organization", { body }),
  organizationCadence: () => call<Cadence | null>("GET", "/organization/cadence"),
  setOrganizationCadence: (cadence: Cadence | null) => call<Cadence | null>("PUT", "/organization/cadence", { body: cadence }),

  // People
  members: () => call<Person[]>("GET", "/members"),
  invite: (body: { name: string; email: string; role: Role; tags: Tags }) => call<Person>("POST", "/members", { body }),
  updateMember: (id: string, body: Partial<Pick<Person, "role" | "tags" | "filters">>) =>
    call<Person>("PATCH", `/members/${enc(id)}`, { body }),
  removeMember: (id: string) => call<undefined>("DELETE", `/members/${enc(id)}`),

  // Tags and the tag map
  tagNames: () => call<TagName[]>("GET", "/tag-names"),
  addTagName: (name: string) => call<TagName>("POST", "/tag-names", { body: { name } }),
  removeTagName: (name: string) => call<undefined>("DELETE", `/tag-names/${enc(name)}`),
  mergeTagValues: (name: string, from: string[], into: string) =>
    call<TagName>("POST", `/tag-names/${enc(name)}/merge`, { body: { from, into } }),
  tagMap: () => call<TagMap>("GET", "/tag-map"),
  setTagMap: (body: TagMap) => call<TagMap>("PUT", "/tag-map", { body }),

  // Projects and tickets
  projects: () => call<Project[]>("GET", "/projects"),
  createProject: (body: Omit<Project, "id">) => call<Project>("POST", "/projects", { body }),
  updateProject: (key: string, body: Partial<Omit<Project, "id" | "key">>) =>
    call<Project>("PATCH", `/projects/${enc(key)}`, { body }),
  tickets: (project?: string) => call<Ticket[]>("GET", "/tickets", { query: { project } }),
  createTicket: (body: Pick<Ticket, "key" | "title" | "description" | "parent" | "assignee" | "project">) =>
    call<Ticket>("POST", "/tickets", { body }),

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
  takeOver: (id: string) => call<TokenEntry>("POST", `/token-entries/${enc(id)}/take-over`),
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

  // Settings: tokens, ticket systems, raw export, plan
  apiTokens: () => call<ApiToken[]>("GET", "/api-tokens"),
  createApiToken: (kind: ApiTokenKind, name: string) => call<NewApiTokenResult>("POST", "/api-tokens", { body: { kind, name } }),
  revokeApiToken: (id: string) => call<undefined>("DELETE", `/api-tokens/${enc(id)}`),
  moveApiToken: (id: string, person: string) => call<ApiToken>("POST", `/api-tokens/${enc(id)}/owner`, { body: { person } }),
  ticketSystems: () => call<TicketSystem[]>("GET", "/ticket-systems"),
  connectTicketSystem: (body: { kind: TicketSystem["kind"]; site: string; token: string }) =>
    call<TicketSystem>("POST", "/ticket-systems", { body }),
  syncTickets: (id: string) => call<TicketSystem>("POST", `/ticket-systems/${enc(id)}/sync`),
  rawExport: () => download("/export/raw"),
  checkout: (interval: "month" | "year") => call<CheckoutSession>("POST", "/billing/checkout", { body: { interval } }),
  billingPortal: () => call<CheckoutSession>("POST", "/billing/portal"),

  // Connecting a computer (`tc connect`, a device sign-in)
  cliAuthorization: (code: string) => call<CliAuthorization>("GET", `/cli/authorizations/by-code/${enc(code)}`),
  confirmCli: (code: string) => call<CliAuthorization>("POST", `/cli/authorizations/by-code/${enc(code)}/confirm`),
  declineCli: (code: string) => call<CliAuthorization>("POST", `/cli/authorizations/by-code/${enc(code)}/decline`),
};
