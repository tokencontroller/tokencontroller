// The check slot's pages and parts (index.ts in this folder). The links
// check that the slot's routes and the app's own keep their types.

import { useQuery } from "@tanstack/react-query";
import { getRouteApi, Link } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { call } from "@/api/client";
import { Dialog } from "@/components/ui/dialog";
import { Note, PageHeader, Section } from "@/components/ui/display";
import type { MenuEntry, PanelProps } from "@/lib/additions";

const planRoute = getRouteApi("/app/plan");
const invoiceRoute = getRouteApi("/app/plan/invoices/$id");

interface Plan {
  name: string;
}

export function PlanPage(): ReactNode {
  const { tab } = planRoute.useSearch();
  return (
    <>
      <PageHeader title="Plan" lead={tab} />
      <Link to="/plan" search={{ tab: "invoices" }}>
        Invoices
      </Link>
      <Link to="/plan/invoices/$id" params={{ id: "in_1" }}>
        The first invoice
      </Link>
      <Link to="/close" search={{ period: "2026-09" }}>
        Monthly close
      </Link>
      {/* @ts-expect-error -- not a page of the app or the slot, so links stay checked */}
      <Link to="/nope">Nowhere</Link>
    </>
  );
}

export function InvoicePage(): ReactNode {
  const { id } = invoiceRoute.useParams();
  return <PageHeader title={id} />;
}

export function useCheckMenu(): readonly MenuEntry[] {
  const plan = useQuery({ queryKey: ["plan"], queryFn: () => call<Plan>("GET", "/plan") });
  if (!plan.data) return [];
  return [
    { group: "Organization", label: "Plan", role: "controller", to: "/plan" },
    { group: "Help", label: "Ask us", role: "agent_manager", Panel: AskPanel },
  ];
}

function AskPanel({ open, onOpenChange }: PanelProps): ReactNode {
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title="Ask us">
      <p>Ask us.</p>
    </Dialog>
  );
}

export function PlanLine(): ReactNode {
  return (
    <Section title="Plan">
      <Link to="/plan">Open the plan</Link>
    </Section>
  );
}

export function PeopleNote(): ReactNode {
  return <Note>A note on People.</Note>;
}

export function OverviewNote(): ReactNode {
  return <Note>A note on Overview.</Note>;
}

export function SignUpNote(): ReactNode {
  return <p>A note on sign-up.</p>;
}
