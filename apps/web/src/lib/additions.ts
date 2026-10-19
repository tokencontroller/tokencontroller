import type { ComponentType } from "react";
import type { Role } from "@/api/contract";
import { additions as slot } from "@/additions";
import type { AppRoute } from "@/router";

// What a build can add to the web app through the slot in `src/additions/`
// (see its README). The open build's slot adds nothing. A private build, such
// as the hosted service's, puts its own folder in the slot, and the type
// check holds that folder to the types here.

/** Opens and closes the dialog of a menu entry. */
export interface PanelProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/** One more entry in the main menu: a page, or a dialog it opens. */
export type MenuEntry = {
  /**
   * The menu group it joins, by title, such as "Organization" or "Help". A
   * title the menu does not have starts a group of its own after the others.
   */
  group: string;
  label: string;
  /** Who sees it: this role and the roles above it. */
  role: Role;
} & (
  | {
      /** The page it opens, such as one of the build's routes. */
      to: string;
    }
  | {
      /** The dialog it opens. The shell keeps it outside the phone menu, which closes first. */
      Panel: ComponentType<PanelProps>;
    }
);

/** What the build's routes are made with. */
export interface RouteKit {
  /** The parent of every page for signed-in members. */
  app: AppRoute;
  /** Keeps a page to a role and the roles above it, as the app's own pages are. */
  requireRole: (role: Role) => () => Promise<void>;
}

/** Parts a build adds to the app's own pages. A part that is not set shows nothing. */
export interface PageParts {
  /** Settings, for org managers: a line about the organization's plan. */
  SettingsPlan?: ComponentType;
  /** Grants and roles, under the page header. */
  PeopleNote?: ComponentType;
  /** Overview, at the end of the page. */
  OverviewNote?: ComponentType;
  /** Sign-up, above the form. */
  SignUpNote?: ComponentType;
}

export interface Additions {
  /**
   * More pages for signed-in members, made with `createRoute`. The type asks
   * only for a list: with a route type here, `createRoute` would take `any`
   * for every part of a route it is not given, such as its search, and the
   * app's own links would stop type-checking. The router checks that each
   * one is a route.
   */
  routes: (kit: RouteKit) => readonly unknown[];
  /**
   * The menu entries to show now. It is a hook, so it can ask the service
   * first, such as to leave an entry out while the build's endpoint answers 404.
   */
  useMenu: () => readonly MenuEntry[];
  parts: PageParts;
  /**
   * Gets every answer from the service: those of `typed`, `call` and
   * `download` in `src/api/client.ts`. It must not read the body or throw.
   */
  onResponse: (response: Response) => void;
  /** Gets every error message the app shows, from `ErrorBox` and `FormError`. */
  onErrorShown: (message: string) => void;
}

/** The build's additions. The app reads them from here, never from the slot. */
export const additions: Additions = slot;

/**
 * The build's pages with their own types, so links to them type-check as the
 * app's own do.
 */
export const addedRoutes: (kit: RouteKit) => ReturnType<typeof slot.routes> = slot.routes;

/** A menu group: the app's own items, then the build's entries for it. */
export interface MenuGroup<Item> {
  title: string;
  items: Item[];
  added: MenuEntry[];
}

/**
 * Puts the build's menu entries into the app's groups by title. An entry
 * whose title the menu does not have starts a group of its own, after the
 * app's groups, in the order the entries come.
 */
export function joinMenu<Item>(own: { title: string; items: Item[] }[], added: readonly MenuEntry[]): MenuGroup<Item>[] {
  const titles = own.map((group) => group.title);
  for (const entry of added) if (!titles.includes(entry.group)) titles.push(entry.group);
  return titles.map((title) => ({
    title,
    items: own.find((group) => group.title === title)?.items ?? [],
    added: added.filter((entry) => entry.group === title),
  }));
}
