import { createRootRoute, createRoute, createRouter, Outlet, redirect } from "@tanstack/react-router";
import { ApiError } from "@/api/client";
import { AppShell } from "@/components/app-shell";
import { ErrorBox } from "@/components/ui/display";
import { meQuery, queryClient } from "@/lib/queries";
import { rememberNext, takeNext } from "@/lib/remember";
import { hasRole } from "@/lib/roles";
import type { Role } from "@/api/contract";
import { ApprovalPage } from "@/routes/approval";
import { BillsPage } from "@/routes/bills";
import { ClosePage } from "@/routes/close";
import { CostSheetPage, SharedCostSheetPage } from "@/routes/cost-sheet";
import { BudgetsPage } from "@/routes/budgets";
import { OverviewPage } from "@/routes/overview";
import { PeoplePage } from "@/routes/people";
import { ProjectPage, ProjectsPage } from "@/routes/projects";
import { ReportsPage } from "@/routes/reports";
import { ReviewPage } from "@/routes/review";
import { RulesPage } from "@/routes/rules";
import { SettingsPage } from "@/routes/settings";
import { InvitePage, SignInPage, SignUpPage } from "@/routes/sign-in";
import { BillingPage } from "@/routes/billing";
import { TagsPage } from "@/routes/tags";
import { TokenEntriesPage } from "@/routes/token-entries";
import { NotFoundPage } from "@/routes/not-found";
import { FeedbackPage } from "@/routes/feedback";
import { AdminPage, AdminQuestionsPage } from "@/routes/admin";
import { AdminConversationPage, AdminSupportPage, supportStates } from "@/routes/admin-support";
import { AdminFeedbackPage, AdminReportPage, feedbackStatuses } from "@/routes/admin-feedback";
import { AdminCustomerPage, AdminCustomersPage } from "@/routes/admin-customers";
import { AdminNewsletterPage } from "@/routes/admin-newsletter";
import type { FeedbackStatus } from "@/api/contract";

const rootRoute = createRootRoute({ component: Outlet, notFoundComponent: NotFoundPage });

const signInRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/sign-in",
  validateSearch: (search: Record<string, unknown>): { sent?: string } => (typeof search.sent === "string" ? { sent: search.sent } : {}),
  component: SignInPage,
});

const signUpRoute = createRoute({ getParentRoute: () => rootRoute, path: "/signup", component: SignUpPage });

// The link in an invite mail: who invited whom, then a sign-in link.
const inviteRoute = createRoute({ getParentRoute: () => rootRoute, path: "/i/$code", component: InvitePage });

// The cost sheet link a controller hands to finance: no sign-in.
const sharedCostSheetRoute = createRoute({ getParentRoute: () => rootRoute, path: "/shared/cost-sheet/$token", component: SharedCostSheetPage });

// Everything below needs a signed-in member.
const appRoute = createRoute({
  getParentRoute: () => rootRoute,
  id: "app",
  beforeLoad: async ({ location }): Promise<void> => {
    try {
      await queryClient.query({ ...meQuery, staleTime: "static" });
    } catch (error) {
      if (error instanceof ApiError && error.status === 401) {
        // Back to the page they asked for once signed in, such as /close/2026-09.
        rememberNext(location.href);
        // eslint-disable-next-line @typescript-eslint/only-throw-error -- the router's way to redirect
        throw redirect({ to: "/sign-in" });
      }
      throw error;
    }
    // The magic link lands on the start page; go on to the page asked for.
    const next = location.pathname === "/" ? takeNext() : null;
    // eslint-disable-next-line @typescript-eslint/only-throw-error -- the router's way to redirect
    if (next && next !== "/") throw redirect({ href: next });
  },
  component: () => (
    <AppShell>
      <Outlet />
    </AppShell>
  ),
  errorComponent: ({ error }) => (
    <div className="mx-auto max-w-lg p-8">
      <ErrorBox error={error} />
    </div>
  ),
});

/** Keeps a page to the roles the docs give it. */
function requireRole(role: Role): () => Promise<void> {
  return async () => {
    const me = await queryClient.query({ ...meQuery, staleTime: "static" });
    if (!hasRole(me.person, role)) {
      // eslint-disable-next-line @typescript-eslint/only-throw-error -- the router's way to redirect
      throw redirect({ to: "/" });
    }
  };
}

export interface PeriodSearch {
  period?: string;
}

const periodSearch = (search: Record<string, unknown>): PeriodSearch =>
  typeof search.period === "string" && /^\d{4}-\d{2}$/.test(search.period) ? { period: search.period } : {};

const page = <P extends string>(path: P, component: () => React.ReactNode, role: Role = "agent_manager") =>
  createRoute({ getParentRoute: () => appRoute, path, beforeLoad: requireRole(role), validateSearch: periodSearch, component });

const overviewRoute = page("/", OverviewPage);
const reviewRoute = page("/review", ReviewPage);
const entriesRoute = page("/token-entries", TokenEntriesPage);
const approvalRoute = page("/approval", ApprovalPage, "people_manager");
const billsRoute = page("/bills", BillsPage, "controller");
const closeRoute = page("/close", ClosePage, "controller");
const costSheetRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/close/$period",
  beforeLoad: requireRole("controller"),
  component: CostSheetPage,
});
const budgetsRoute = page("/budgets", BudgetsPage, "people_manager");
const reportsRoute = page("/reports", ReportsPage);
const projectsRoute = page("/projects", ProjectsPage, "people_manager");
const projectRoute = createRoute({ getParentRoute: () => appRoute, path: "/projects/$name", beforeLoad: requireRole("people_manager"), validateSearch: periodSearch, component: ProjectPage });
const tagsRoute = page("/tags", TagsPage, "controller");
const rulesRoute = page("/rules", RulesPage);
const peopleRoute = page("/people", PeoplePage, "people_manager");
const settingsRoute = page("/settings", SettingsPage);
// Stripe Checkout and the customer portal come back here (`?checkout=success`).
const billingRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/billing",
  beforeLoad: requireRole("controller"),
  validateSearch: (search: Record<string, unknown>): { checkout?: string } => (typeof search.checkout === "string" ? { checkout: search.checkout } : {}),
  component: BillingPage,
});

// Every member's feedback reports and the answers; the service mails links here.
const feedbackRoute = page("/feedback", FeedbackPage);

// The admin area. The service answers 404 to everyone who is not an operator
// (except /admin/feedback for controllers on an instance without operators),
// so the pages need no role of their own.
const adminRoute = createRoute({ getParentRoute: () => appRoute, path: "/admin", component: AdminPage });
const adminSupportRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/admin/support",
  validateSearch: (search: Record<string, unknown>): { state?: string } =>
    typeof search.state === "string" && (supportStates as readonly string[]).includes(search.state) ? { state: search.state } : {},
  component: AdminSupportPage,
});
const adminConversationRoute = createRoute({ getParentRoute: () => appRoute, path: "/admin/support/$id", component: AdminConversationPage });
const adminFeedbackRoute = createRoute({
  getParentRoute: () => appRoute,
  path: "/admin/feedback",
  validateSearch: (search: Record<string, unknown>): { status?: FeedbackStatus } =>
    feedbackStatuses.includes(search.status as FeedbackStatus) ? { status: search.status as FeedbackStatus } : {},
  component: AdminFeedbackPage,
});
const adminReportRoute = createRoute({ getParentRoute: () => appRoute, path: "/admin/feedback/$id", component: AdminReportPage });
const adminCustomersRoute = createRoute({ getParentRoute: () => appRoute, path: "/admin/customers", component: AdminCustomersPage });
const adminCustomerRoute = createRoute({ getParentRoute: () => appRoute, path: "/admin/customers/$id", component: AdminCustomerPage });
const adminQuestionsRoute = createRoute({ getParentRoute: () => appRoute, path: "/admin/questions", component: AdminQuestionsPage });
const adminNewsletterRoute = createRoute({ getParentRoute: () => appRoute, path: "/admin/newsletter", component: AdminNewsletterPage });

const routeTree = rootRoute.addChildren([
  signInRoute,
  signUpRoute,
  inviteRoute,
  sharedCostSheetRoute,
  appRoute.addChildren([
    overviewRoute,
    reviewRoute,
    entriesRoute,
    approvalRoute,
    billsRoute,
    closeRoute,
    costSheetRoute,
    budgetsRoute,
    reportsRoute,
    projectsRoute,
    projectRoute,
    tagsRoute,
    rulesRoute,
    peopleRoute,
    settingsRoute,
    billingRoute,
    feedbackRoute,
    adminRoute,
    adminSupportRoute,
    adminConversationRoute,
    adminFeedbackRoute,
    adminReportRoute,
    adminCustomersRoute,
    adminCustomerRoute,
    adminQuestionsRoute,
    adminNewsletterRoute,
  ]),
]);

export const router = createRouter({ routeTree, defaultPreload: "intent", scrollRestoration: true });

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
