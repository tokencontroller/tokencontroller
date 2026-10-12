// A slot that adds one of each part, for the type check only (README.md in
// src/additions). `pnpm typecheck` checks the app with this folder in the
// slot, so a change that a private build's folder would no longer fit fails
// here too. The routes are written in the object, as a build would.

import { createRoute } from "@tanstack/react-router";
import type { Additions } from "@/lib/additions";
import { InvoicePage, OverviewNote, PeopleNote, PlanLine, PlanPage, SignUpNote, useCheckMenu } from "./pages";

const seen: string[] = [];

export const additions = {
  routes: ({ app, requireRole }) => [
    createRoute({
      getParentRoute: () => app,
      path: "/plan",
      beforeLoad: requireRole("controller"),
      validateSearch: (search: Record<string, unknown>): { tab?: string } => (typeof search.tab === "string" ? { tab: search.tab } : {}),
      component: PlanPage,
    }),
    createRoute({ getParentRoute: () => app, path: "/plan/invoices/$id", beforeLoad: requireRole("controller"), component: InvoicePage }),
  ],
  useMenu: useCheckMenu,
  parts: { SettingsPlan: PlanLine, PeopleNote, OverviewNote, SignUpNote },
  onResponse: (response) => {
    seen.push(response.url);
  },
  onErrorShown: (message) => {
    seen.push(message);
  },
} satisfies Additions;
