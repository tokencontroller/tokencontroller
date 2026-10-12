import { Dialog as BaseDialog } from "@base-ui/react/dialog";
import { useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useRouterState } from "@tanstack/react-router";
import { useState, type ReactNode } from "react";
import type { Role } from "@/api/contract";
import { api } from "@/api/endpoints";
import { useMe } from "@/lib/queries";
import { hasRole, roleLabel } from "@/lib/roles";
import { displayName, initials } from "@/lib/format";

interface NavItem {
  to: string;
  label: string;
  role: Role;
}

// Grouped by the loop: plan, record, reconcile, learn (docs: Concepts).
const groups: { title: string; items: NavItem[] }[] = [
  { title: "", items: [{ to: "/", label: "Overview", role: "agent_manager" }] },
  {
    title: "Record",
    items: [
      { to: "/review", label: "Review and submit", role: "agent_manager" },
      { to: "/token-entries", label: "Token entries", role: "agent_manager" },
    ],
  },
  {
    title: "Reconcile",
    items: [
      { to: "/approval", label: "Approval", role: "people_manager" },
      { to: "/bills", label: "Bills", role: "controller" },
      { to: "/close", label: "Close", role: "controller" },
    ],
  },
  {
    title: "Plan and learn",
    items: [
      { to: "/budgets", label: "Token budgets", role: "people_manager" },
      { to: "/reports", label: "Reports", role: "agent_manager" },
    ],
  },
  {
    title: "Organization",
    items: [
      { to: "/projects", label: "Projects and tickets", role: "people_manager" },
      { to: "/tags", label: "Tags", role: "controller" },
      { to: "/rules", label: "Rules", role: "agent_manager" },
      { to: "/people", label: "People", role: "people_manager" },
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

function Nav({ onNavigate }: { onNavigate?: () => void }) {
  const me = useMe();
  const path = useRouterState({ select: (s) => s.location.pathname });
  return (
    <nav aria-label="Main" className="flex flex-col gap-5">
      {groups.map((group) => {
        const items = group.items.filter((item) => hasRole(me.person, item.role));
        if (!items.length) return null;
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
            </ul>
          </div>
        );
      })}
    </nav>
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
  return (
    <div className="min-h-dvh lg:grid lg:grid-cols-[240px_minmax(0,1fr)]">
      <aside className="sticky top-0 hidden h-dvh flex-col gap-6 overflow-y-auto border-r border-line bg-sheet px-3 py-5 lg:flex">
        <div className="px-2">
          <Logo />
          <div className="mt-1 truncate text-xs text-muted">{me.organization.name}</div>
        </div>
        <div className="flex-1">
          <Nav />
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
                <Nav onNavigate={() => setOpen(false)} />
              </div>
              <Account />
            </BaseDialog.Popup>
          </BaseDialog.Portal>
        </BaseDialog.Root>
      </header>

      <main className="mx-auto w-full min-w-0 max-w-6xl px-4 py-6 sm:px-6 lg:px-10 lg:py-8">{children}</main>
    </div>
  );
}
