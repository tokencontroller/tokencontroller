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
    <nav aria-label="Main" className="flex flex-col gap-5">
      {menu.map((group) => {
        const added = group.added.map((entry) => <AddedItem key={entry.label} entry={entry} path={path} onNavigate={onNavigate} onPanel={onPanel} />);
        const items = group.items.filter((item) => hasRole(me.person, item.role));
        if (!items.length && !added.length) return null;
        return (
          <div key={group.title || "home"}>
            {group.title ? <div className="mb-1 px-2 text-xs font-medium text-muted">{group.title}</div> : null}
            <ul className="flex flex-col gap-px">
              {items.map((item) => {
                const active = item.to === "/" ? path === "/" : path.startsWith(item.to);
                return (
                  <li key={item.to}>
                    <Link
                      to={item.to}
                      onClick={onNavigate}
                      aria-current={active ? "page" : undefined}
                      className={`block rounded-md px-2 py-1.5 text-sm ${active ? "bg-accent-tint font-medium text-ink" : "text-ink-soft hover:bg-sand hover:text-ink"}`}
                    >
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
  `flex w-full items-center justify-between gap-2 rounded-md px-2 py-1.5 text-left text-sm ${active ? "bg-accent-tint font-medium text-ink" : "text-ink-soft hover:bg-sand hover:text-ink"}`;

/** One of the build's menu entries: a page, or a button that opens its dialog. */
function AddedItem({ entry, path, onNavigate, onPanel }: { entry: MenuEntry; path: string; onNavigate?: (() => void) | undefined; onPanel: (panel: Panel) => void }) {
  if ("to" in entry) {
    const active = path.startsWith(entry.to);
    return (
      <li>
        <Link to={entry.to} onClick={onNavigate} aria-current={active ? "page" : undefined} className={itemClass(active)}>
          {entry.label}
        </Link>
      </li>
    );
  }
  return (
    <li>
      <button type="button" className={itemClass(false)} onClick={() => { onNavigate?.(); onPanel(`menu:${entry.label}`); }}>
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
      <span aria-hidden="true" className="flex size-8 shrink-0 items-center justify-center rounded-full bg-sand text-xs font-semibold text-ink-soft">
        {initials(displayName(me.person))}
      </span>
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm font-medium">{displayName(me.person)}</div>
        <div className="truncate text-xs text-muted">{roleLabel[me.person.role]}</div>
      </div>
      <button type="button" onClick={() => void signOut()} className="rounded-md px-2 py-1 text-xs text-muted hover:bg-sand hover:text-ink">
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
    <div className="min-h-dvh lg:grid lg:grid-cols-[240px_minmax(0,1fr)]">
      <aside className="sticky top-0 hidden h-dvh flex-col gap-6 overflow-y-auto border-r border-line bg-sheet px-3 py-5 lg:flex">
        <div className="px-2">
          <Logo />
          <div className="mt-1 truncate text-xs text-muted">{me.organization.name}</div>
        </div>
        <div className="flex-1">
          <Nav onPanel={setPanel} />
        </div>
        <Account />
      </aside>

      <header className="sticky top-0 z-30 flex h-14 items-center justify-between border-b border-line bg-page/95 px-4 backdrop-blur-none lg:hidden">
        <Logo />
        <BaseDialog.Root open={open} onOpenChange={setOpen}>
          <BaseDialog.Trigger aria-label="Menu" className="-mr-2 rounded-md p-2 text-ink hover:bg-sand">
            <svg width="20" height="20" viewBox="0 0 20 20" aria-hidden="true"><path d="M3 6h14M3 10h14M3 14h14" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" /></svg>
          </BaseDialog.Trigger>
          <BaseDialog.Portal>
            <BaseDialog.Backdrop className="fixed inset-0 z-40 bg-ink/25" />
            <BaseDialog.Popup className="fixed inset-y-0 left-0 z-50 flex w-72 max-w-[85vw] flex-col gap-6 overflow-y-auto bg-sheet px-3 py-5 shadow-pop transition-transform data-[ending-style]:-translate-x-full data-[starting-style]:-translate-x-full">
              <BaseDialog.Title className="px-2">
                <Logo />
                <span className="mt-1 block truncate text-xs font-normal text-muted">{me.organization.name}</span>
              </BaseDialog.Title>
              <div className="flex-1">
                <Nav onNavigate={() => setOpen(false)} onPanel={setPanel} />
              </div>
              <Account />
            </BaseDialog.Popup>
          </BaseDialog.Portal>
        </BaseDialog.Root>
      </header>

      <main className="mx-auto w-full min-w-0 max-w-6xl px-4 py-6 sm:px-6 lg:px-10 lg:py-8">
        <LicenseNotice />
        {children}
      </main>
      {added.map((entry) => ("Panel" in entry ? <entry.Panel key={entry.label} open={panel === `menu:${entry.label}`} onOpenChange={(o) => setPanel(o ? `menu:${entry.label}` : null)} /> : null))}
    </div>
  );
}
