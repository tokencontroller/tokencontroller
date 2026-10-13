import { createRootRoute, createRoute, createRouter, Outlet, redirect } from "@tanstack/react-router";
import { ApiError } from "@/api/client";
import { AppShell } from "@/components/app-shell";
import { ErrorBox } from "@/components/ui/display";
import { addedRoutes } from "@/lib/additions";
import { meQuery, queryClient } from "@/lib/queries";
import { rememberFirstSteps, rememberNext, takeNext } from "@/lib/remember";
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
import { TagsPage } from "@/routes/tags";
import { TokenEntriesPage } from "@/routes/token-entries";
import { NotFoundPage } from "@/routes/not-found";

const rootRoute = createRootRoute({ component: Outlet, notFoundComponent: NotFoundPage });

/** Someone already signed in has no use for the sign-in and sign-up forms. */
async function startPageIfSignedIn(): Promise<void> {
  try {
    await queryClient.query({ ...meQuery, staleTime: "static" });
  } catch {
    // Not signed in, which is what these pages are for.
    return;
  }
  // eslint-disable-next-line @typescript-eslint/only-throw-error -- the router's way to redirect
  throw redirect({ to: "/" });
}

const signInRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/sign-in",
  validateSearch: (search: Record<string, unknown>): { sent?: string } => (typeof search.sent === "string" ? { sent: search.sent } : {}),
  beforeLoad: startPageIfSignedIn,
  component: SignInPage,
});

// With a token, the link from the "no account" mail: the form then creates
// the organization at once.
const signUpRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/signup",
  validateSearch: (search: Record<string, unknown>): { token?: string } => (typeof search.token === "string" ? { token: search.token } : {}),
  beforeLoad: startPageIfSignedIn,
  component: SignUpPage,
});

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
    // A sign-up link lands on /?first_steps=people: keep the pick for the
    // start page's first steps.
    const { first_steps: firstSteps } = location.search as { first_steps?: unknown };
    if (firstSteps === "agents" || firstSteps === "people") {
      rememberFirstSteps(firstSteps);
      // eslint-disable-next-line @typescript-eslint/only-throw-error -- the router's way to redirect
      throw redirect({ to: "/" });
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

/** The parent of every page for signed-in members, also those a build adds. */
export type AppRoute = typeof appRoute;

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
    // The pages of the build's additions (src/additions); none in the open build.
    ...addedRoutes({ app: appRoute, requireRole }),
  ]),
]);

export const router = createRouter({ routeTree, defaultPreload: "intent", scrollRestoration: true });

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
