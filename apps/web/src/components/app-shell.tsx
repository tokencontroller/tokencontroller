import { Dialog as BaseDialog } from "@base-ui/react/dialog";
import { useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useRouterState } from "@tanstack/react-router";
import { useState, type ReactNode } from "react";
import type { Role } from "@/api/contract";
import { api } from "@/api/endpoints";
import { additions, joinMenu, type MenuEntry } from "@/lib/additions";
import { useMe } from "@/lib/queries";
import { hasRole, roleLabel } from "@/lib/roles";
import { displayName, initials } from "@/lib/format";
import { LicenseNotice } from "./enterprise-settings";

interface NavItem {
  to: string;
  label: string;
  role: Role;
}

/**
 * Small line icons for the main menu, 16 px on a 16 px grid, drawn with the
 * text colour. One per screen; a build's own entries get the dot.
 */
const iconPaths: Record<string, string> = {
  "/": "M2.5 8.5 8 3l5.5 5.5M4 7.5V13h3v-3h2v3h3V7.5",
  "/sessions": "M3 8.5l2.5 2.5L13 3.5M3 13.5h10",
  "/submissions": "M8 2.5v7m0-7L5.5 5M8 2.5 10.5 5M3 10v3.5h10V10",
  "/reports": "M3 13.5V8m3.3 5.5V3m3.4 10.5V6.5m3.3 7V10",
  "/plans": "M8 13.5A5.5 5.5 0 1 0 8 2.5a5.5 5.5 0 0 0 0 11ZM8 10.5a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5Z",
  "/bills": "M4 2.5h8v11l-2-1.2-2 1.2-2-1.2-2 1.2v-11ZM6 6h4M6 8.5h4",
  "/price-table": "M2.5 3.5h11v9h-11v-9ZM2.5 6.5h11M6.5 6.5v6",
  "/cutoff": "M3 3.5h10v10H3v-10ZM3 6.5h10M6 2v3m4-3v3M6 10l1.5 1.5L10.5 8.5",
  "/tags": "M2.5 2.5h5.2l5.8 5.8-5.2 5.2-5.8-5.8V2.5ZM5.3 5.3h.01",
  "/attributes": "M8 7.5a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5ZM3 13.5c.5-2.5 2.6-4 5-4s4.5 1.5 5 4",
  "/grants": "M5.5 7V5a2.5 2.5 0 0 1 5 0v2M4 7h8v6.5H4V7Zm4 2.5v1.5",
  "/rules": "M3 4h10M3 8h6m-6 4h3m4.5-2 2 2-2 2",
  "/settings": "M8 10a2 2 0 1 0 0-4 2 2 0 0 0 0 4Zm0-7.5v1.5m0 8v1.5M2.5 8H4m8 0h1.5M4.1 4.1l1.1 1.1m5.6 5.6 1.1 1.1m0-7.8-1.1 1.1m-5.6 5.6-1.1 1.1",
};

function NavIcon({ to }: { to?: string }) {
  const d = to ? iconPaths[to] : undefined;
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true" className="shrink-0">
      {d ? <path d={d} fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" /> : <circle cx="8" cy="8" r="1.75" fill="currentColor" />}
    </svg>
  );
}

// Grouped as the docs are: record, account, report, organization.
const groups: { title: string; items: NavItem[] }[] = [
  { title: "", items: [{ to: "/", label: "Overview", role: "agent_manager" }] },
  {
    title: "Record",
    items: [
      { to: "/sessions", label: "Check and submit", role: "agent_manager" },
      { to: "/submissions", label: "Submissions", role: "agent_manager" },
    ],
  },
  {
    title: "Report",
    items: [
      { to: "/reports", label: "Reports", role: "agent_manager" },
      { to: "/plans", label: "Plans and estimates", role: "people_manager" },
    ],
  },
  {
    title: "Account",
    items: [
      { to: "/bills", label: "Bills", role: "org_manager" },
      { to: "/price-table", label: "Price table", role: "org_manager" },
      { to: "/cutoff", label: "Cutoff", role: "org_manager" },
    ],
  },
  {
    title: "Organization",
    items: [
      { to: "/tags", label: "Tags", role: "people_manager" },
      { to: "/attributes", label: "Attributes", role: "people_manager" },
      { to: "/grants", label: "Grants and roles", role: "people_manager" },
      { to: "/rules", label: "Rules", role: "agent_manager" },
      { to: "/settings", label: "Settings", role: "agent_manager" },
    ],
  },
];

function Logo() {
  return (
    <span className="flex items-center gap-2">
      <img src="/logo-light-small.svg" width="24" height="24" alt="" className="logo-light size-6" />
      <img src="/logo-dark-small.svg" width="24" height="24" alt="" className="logo-dark size-6" />
      <span className="text-[15px] font-semibold tracking-[-0.01em]">Token Controller</span>
    </span>
  );
}

/** The open panel: the dialog of a build's menu entry, by its label. */
type Panel = `menu:${string}`;

/** The menu entries of the build's additions (src/additions) this person may see. */
function useAddedMenu(): MenuEntry[] {
  const me = useMe();
  return additions.useMenu().filter((entry) => hasRole(me.person, entry.role));
}

function Nav({ onNavigate, onPanel }: { onNavigate?: () => void; onPanel: (panel: Panel) => void }) {
  const me = useMe();
  const path = useRouterState({ select: (s) => s.location.pathname });
  // Help is the last of the app's groups; only a build's entries fill it.
  const menu = joinMenu([...groups, { title: "Help", items: [] }], useAddedMenu());
  return (
    <nav aria-label="Main" className="flex flex-col gap-4">
      {menu.map((group) => {
        const added = group.added.map((entry) => <AddedItem key={entry.label} entry={entry} path={path} onNavigate={onNavigate} onPanel={onPanel} />);
        const items = group.items.filter((item) => hasRole(me.person, item.role));
        if (!items.length && !added.length) return null;
        return (
          <div key={group.title || "home"}>
            {group.title ? <div className="mb-1 px-2 text-caption font-medium text-muted">{group.title}</div> : null}
            <ul className="flex flex-col gap-px">
              {items.map((item) => {
                const active = item.to === "/" ? path === "/" : path.startsWith(item.to);
                return (
                  <li key={item.to}>
                    <Link
                      to={item.to}
                      onClick={onNavigate}
                      aria-current={active ? "page" : undefined}
                      className={`flex h-8 items-center gap-2.5 rounded-md px-2 text-body ${active ? "bg-accent-tint font-medium text-ink [&_svg]:text-accent" : "text-ink-soft hover:bg-hover hover:text-ink [&_svg]:text-muted"}`}
                    >
                      <NavIcon to={item.to} />
                      {item.label}
                    </Link>
                  </li>
                );
              })}
              {added}
            </ul>
          </div>
        );
      })}
    </nav>
  );
}

const itemClass = (active: boolean) =>
  `flex h-8 w-full items-center gap-2.5 rounded-md px-2 text-left text-body ${active ? "bg-accent-tint font-medium text-ink [&_svg]:text-accent" : "text-ink-soft hover:bg-hover hover:text-ink [&_svg]:text-muted"}`;

/** One of the build's menu entries: a page, or a button that opens its dialog. */
function AddedItem({ entry, path, onNavigate, onPanel }: { entry: MenuEntry; path: string; onNavigate?: (() => void) | undefined; onPanel: (panel: Panel) => void }) {
  if ("to" in entry) {
    const active = path.startsWith(entry.to);
    return (
      <li>
        <Link to={entry.to} onClick={onNavigate} aria-current={active ? "page" : undefined} className={itemClass(active)}>
          <NavIcon />
          {entry.label}
        </Link>
      </li>
    );
  }
  return (
    <li>
      <button type="button" className={itemClass(false)} onClick={() => { onNavigate?.(); onPanel(`menu:${entry.label}`); }}>
        <NavIcon />
        {entry.label}
      </button>
    </li>
  );
}

function Account() {
  const me = useMe();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const signOut = async () => {
    await api.signOut();
    queryClient.clear();
    await navigate({ to: "/sign-in" });
  };
  return (
    <div className="flex items-center gap-2.5 border-t border-line pt-4">
      <span aria-hidden="true" className="flex size-8 shrink-0 items-center justify-center rounded-full bg-hover text-caption font-semibold text-ink-soft ring-1 ring-inset ring-line">
        {initials(displayName(me.person))}
      </span>
      <div className="min-w-0 flex-1">
        <div className="truncate text-body font-medium">{displayName(me.person)}</div>
        <div className="truncate text-caption text-muted">{roleLabel[me.person.role]}</div>
      </div>
      <button type="button" onClick={() => void signOut()} className="rounded-md px-2 py-1 text-caption text-muted hover:bg-hover hover:text-ink">
        Sign out
      </button>
    </div>
  );
}

export function AppShell({ children }: { children: ReactNode }): ReactNode {
  const me = useMe();
  const [open, setOpen] = useState(false);
  // The panels live here, not in the menu: on a phone the menu closes first.
  const [panel, setPanel] = useState<Panel | null>(null);
  const added = useAddedMenu();
  return (
    <div className="min-h-dvh bg-canvas lg:grid lg:grid-cols-[240px_minmax(0,1fr)]">
      <aside className="sticky top-0 hidden h-dvh flex-col gap-6 overflow-y-auto border-r border-line bg-surface px-3 py-5 lg:flex">
        <div className="px-2">
          <Logo />
          <div className="mt-1 truncate text-caption text-muted">{me.organization.name}</div>
        </div>
        <div className="flex-1">
          <Nav onPanel={setPanel} />
        </div>
        <Account />
      </aside>

      <header className="sticky top-0 z-30 flex h-14 items-center justify-between border-b border-line bg-surface px-4 lg:hidden">
        <Logo />
        <BaseDialog.Root open={open} onOpenChange={setOpen}>
          <BaseDialog.Trigger aria-label="Menu" className="-mr-2 rounded-md p-2 text-ink hover:bg-hover">
            <svg width="20" height="20" viewBox="0 0 20 20" aria-hidden="true"><path d="M3 6h14M3 10h14M3 14h14" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" /></svg>
          </BaseDialog.Trigger>
          <BaseDialog.Portal>
            <BaseDialog.Backdrop className="fixed inset-0 z-40 bg-ink/25" />
            <BaseDialog.Popup className="fixed inset-y-0 left-0 z-50 flex w-72 max-w-[85vw] flex-col gap-6 overflow-y-auto bg-surface px-3 py-5 shadow-pop transition-transform data-[ending-style]:-translate-x-full data-[starting-style]:-translate-x-full">
              <BaseDialog.Title className="px-2">
                <Logo />
                <span className="mt-1 block truncate text-caption font-normal text-muted">{me.organization.name}</span>
              </BaseDialog.Title>
              <div className="flex-1">
                <Nav onNavigate={() => setOpen(false)} onPanel={setPanel} />
              </div>
              <Account />
            </BaseDialog.Popup>
          </BaseDialog.Portal>
        </BaseDialog.Root>
      </header>

      <main className="mx-auto w-full min-w-0 max-w-6xl px-4 py-6 sm:px-6 lg:px-10 lg:py-10">
        <LicenseNotice />
        {children}
      </main>
      {added.map((entry) => ("Panel" in entry ? <entry.Panel key={entry.label} open={panel === `menu:${entry.label}`} onOpenChange={(o) => setPanel(o ? `menu:${entry.label}` : null)} /> : null))}
    </div>
  );
}
