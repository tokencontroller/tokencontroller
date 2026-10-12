import { useMutation, useQuery } from "@tanstack/react-query";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { Link } from "@tanstack/react-router";
import type { Filter, ReportResult } from "@/api/contract";
import { api } from "@/api/endpoints";
import { FilterTags } from "@/routes/budgets";
import { Badge } from "@/components/ui/badge";
import { Button, buttonClass } from "@/components/ui/button";
import { Kpi, KpiRow, Loading, MoneyText, Note, PageHeader, RowBar, Section, ShareBar } from "@/components/ui/display";
import { addMonths, describeFilter, filterToText, formatPeriod } from "@/lib/format";
import { formatMoney, formatShare, shareOf } from "@/lib/money";
import { refreshAll, useBilling, useFirstOpenPeriod, useMe, usePeriods, usePersonName } from "@/lib/queries";
import { Field, FormError, Input } from "@/components/ui/form";
import { forgetSignUp, readSignUp, rememberSignUp, type SignUpChoices } from "@/lib/remember";
import { hasRole } from "@/lib/roles";
import { thisPeriod } from "@/lib/today";

/** Direct, project overhead and organization overhead from two groupings of one filter. */
function useCostKinds(period: string, filter: Filter) {
  const byTicket = useQuery({ queryKey: ["report", period, filterToText(filter), "ticket"], queryFn: () => api.cost({ period, filter, group_by: "ticket" }) });
  const byProject = useQuery({ queryKey: ["report", period, filterToText(filter), "project"], queryFn: () => api.cost({ period, filter, group_by: "project" }) });
  if (!byTicket.data || !byProject.data) return null;
  // Nothing recorded yet: no kind has a share.
  if (!byTicket.data.rows.length) return { direct: 0, project: 0, organization: 0, total: byProject.data.total, byProject: byProject.data };
  const noTicket = byTicket.data.rows.find((r) => r.value === null)?.share ?? 0;
  const noProject = byProject.data.rows.find((r) => r.value === null)?.share ?? 0;
  return { direct: 10000 - noTicket, project: noTicket - noProject, organization: noProject, total: byProject.data.total, byProject: byProject.data };
}

function KindBar({ kinds }: { kinds: NonNullable<ReturnType<typeof useCostKinds>> }) {
  return (
    <ShareBar
      label="Cost by cost kind"
      parts={[
        { label: "Direct, on tickets", share: kinds.direct, color: "var(--color-posted)" },
        { label: "Project overhead", share: kinds.project, color: "var(--color-spectrum-3)" },
        { label: "Organization overhead", share: kinds.organization, color: "var(--color-exception)" },
      ]}
    />
  );
}

function Bars({ result, label }: { result: ReportResult; label: (value: string | null) => string }) {
  const max = Math.max(1, ...result.rows.map((r) => r.share));
  return (
    <ul className="space-y-2.5">
      {result.rows.slice(0, 6).map((r) => (
        <li key={r.value ?? "none"} className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-3 gap-y-1 text-sm sm:grid-cols-[160px_1fr_auto]">
          <span className={`truncate ${r.value === null ? "text-muted" : ""}`}>{label(r.value)}</span>
          <span className="col-span-2 row-start-2 sm:col-span-1 sm:row-start-auto">
            <RowBar share={(r.share * 10000) / max} color={r.value === null ? "var(--color-line-strong)" : "var(--color-accent)"} />
          </span>
          <MoneyText value={r.amount} />
        </li>
      ))}
    </ul>
  );
}

function MyCost() {
  const me = useMe();
  const period = thisPeriod();
  const sheet = useQuery({ queryKey: ["token-sheet"], queryFn: api.tokenSheet });
  // The person tag holds the email.
  const kinds = useCostKinds(period, { person: me.person.email });
  const budgets = useQuery({ queryKey: ["budgets", period], queryFn: () => api.budgets(period) });
  const myBudget = budgets.data?.find((b) => filterToText(b.filter) === `person:${me.person.email}`);
  const drafts = sheet.data?.entries.filter((e) => e.status === "draft").length ?? 0;
  const rejected = sheet.data?.entries.filter((e) => e.status === "rejected").length ?? 0;
  return (
    <>
      {sheet.data?.late ? (
        <Note tone="exception">
          Your token sheet is late. <Link to="/review" className="font-medium underline">Review and submit</Link>
        </Note>
      ) : null}
      <KpiRow>
        <Kpi label={`Your cost, ${formatPeriod(period)}`} value={kinds ? formatMoney(kinds.total) : "…"} note="submitted token entries" />
        <Kpi label="On tickets" value={kinds ? formatShare(kinds.direct) : "…"} tone="posted" />
        <Kpi label="To review" value={drafts} note={rejected ? `${rejected} sent back to you` : "draft token entries"} tone={rejected ? "exception" : undefined} />
        <Kpi label="Your token budget" value={myBudget ? formatMoney(myBudget.amount) : "none"} note={myBudget ? `${formatShare(shareOf(myBudget.actual, myBudget.amount))} used` : undefined} />
      </KpiRow>
      {drafts || rejected ? (
        <div className="mb-8 flex flex-col gap-3 rounded-lg border border-line bg-sheet p-4 sm:flex-row sm:items-center sm:justify-between">
          <p className="text-sm">
            {drafts} draft token {drafts === 1 ? "entry" : "entries"}
            {sheet.data ? ` · ${formatMoney(sheet.data.total)} · ${formatShare(sheet.data.on_tickets)} on tickets` : ""}
          </p>
          <Link to="/review" className={buttonClass("primary")}>
            Review and submit
          </Link>
        </div>
      ) : null}
      {kinds ? (
        <div className="grid gap-8 lg:grid-cols-2">
          <Section title="By cost kind">
            <KindBar kinds={kinds} />
          </Section>
          <Section title="By project">
            <Bars result={kinds.byProject} label={(v) => v ?? "no project"} />
          </Section>
        </div>
      ) : null}
    </>
  );
}

function FilterCard({ filter, period }: { filter: Filter; period: string }) {
  const name = usePersonName();
  const byPerson = useQuery({ queryKey: ["report", period, filterToText(filter), "person"], queryFn: () => api.cost({ period, filter, group_by: "person" }) });
  const kinds = useCostKinds(period, filter);
  return (
    <div className="rounded-lg border border-line p-4">
      <div className="mb-3 flex items-center justify-between gap-2">
        <FilterTags filter={filter} />
        <span className="text-lg font-light num">{byPerson.data ? formatMoney(byPerson.data.total) : "…"}</span>
      </div>
      {kinds ? <KindBar kinds={kinds} /> : null}
      <div className="mt-4">{byPerson.data ? <Bars result={byPerson.data} label={(v) => (v ? name(v) : "no person")} /> : <Loading />}</div>
    </div>
  );
}

function PeopleManagerView() {
  const me = useMe();
  const period = thisPeriod();
  const toApprove = useFirstOpenPeriod() ?? period;
  const waiting = useQuery({ queryKey: ["token-entries", toApprove, "submitted", "", ""], queryFn: () => api.tokenEntries({ period: toApprove, status: "submitted" }) });
  const budgets = useQuery({ queryKey: ["budget-vs-actual", period], queryFn: () => api.budgetVsActual(period) });
  const current = useQuery({ queryKey: ["token-entries", period, "", "", ""], queryFn: () => api.tokenEntries({ period }) });
  const orphans = current.data?.filter((e) => e.agent_manager === null) ?? [];
  const overruns = budgets.data?.filter((b) => Number(b.difference.nanos) > 0) ?? [];
  const filters = me.person.role === "controller" ? [] : me.person.filters;
  return (
    <>
      {orphans.length ? (
        <Note tone="exception">
          {orphans.length} token {orphans.length === 1 ? "entry has" : "entries have"} no agent manager.{" "}
          <Link to="/token-entries" className="font-medium underline">Take {orphans.length === 1 ? "it" : "them"} over</Link>
        </Note>
      ) : null}
      <KpiRow>
        <Kpi label={`Waiting for approval, ${formatPeriod(toApprove)}`} value={me.organization.approval ? (waiting.data?.length ?? "…") : "off"} note={me.organization.approval ? <Link to="/approval" className="text-accent hover:underline">Open approval</Link> : "approval is off"} />
        <Kpi label="Token budgets over" value={budgets.data ? `${overruns.length} of ${budgets.data.length}` : "…"} note={<Link to="/reports" className="text-accent hover:underline">Token budget vs actual</Link>} tone={overruns.length ? "exception" : undefined} />
      </KpiRow>
      {filters.length ? (
        <Section title={`Your filters, ${formatPeriod(period)}`} lead="Cost per person within each filter you are assigned. Filters can overlap.">
          <div className="grid gap-4 lg:grid-cols-2">
            {filters.map((f) => (
              <FilterCard key={describeFilter(f)} filter={f} period={period} />
            ))}
          </div>
        </Section>
      ) : null}
    </>
  );
}

function ControllerView() {
  const periods = usePeriods();
  const period = thisPeriod();
  const last = addMonths(period, -1);
  const recon = useQuery({ queryKey: ["reconciliation", last], queryFn: () => api.reconciliation(last) });
  const kinds = useCostKinds(period, {});
  const lastPeriod = periods.data?.find((p) => p.id === last);
  return (
    <>
      <KpiRow>
        <Kpi label={`Token entries, ${formatPeriod(period)}`} value={kinds ? formatMoney(kinds.total) : "…"} note="so far" />
        <Kpi label={`Attribution, ${formatPeriod(last)}`} value={recon.data ? formatShare(recon.data.attribution) : "…"} note="share of the bills token entries explain" tone="posted" />
        <Kpi label={`Residual, ${formatPeriod(last)}`} value={recon.data ? formatMoney(recon.data.total.residual) : "…"} note="always shown" />
        <Kpi
          label={`Close, ${formatPeriod(last)}`}
          value={lastPeriod?.status === "closed" ? <Badge tone="posted">closed</Badge> : <Badge>open</Badge>}
          note={lastPeriod?.status === "closed" ? <Link to="/close/$period" params={{ period: last }} className="text-accent hover:underline">Cost sheet</Link> : <Link to="/close" className="text-accent hover:underline">Run the close</Link>}
        />
      </KpiRow>
      {kinds ? (
        <div className="grid gap-8 lg:grid-cols-2">
          <Section title="How specific the cost is" lead="Rules and people fixing exceptions move cost toward tickets.">
            <KindBar kinds={kinds} />
          </Section>
          <Section title="By project">
            <Bars result={kinds.byProject} label={(v) => v ?? "organization overhead"} />
          </Section>
        </div>
      ) : null}
    </>
  );
}

/** The service asks for no name at sign-up, so a founder adds theirs here. */
function YourName() {
  const me = useMe();
  const [name, setName] = useState("");
  const save = useMutation({ mutationFn: () => api.updateMember(me.person.id, { name: name.trim() }), onSuccess: refreshAll });
  if (me.person.name.trim() || !hasRole(me.person, "controller")) return null;
  return (
    <form className="mb-6 flex max-w-md flex-col gap-2 sm:flex-row sm:items-end" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
      <Field label="Your name" hint="Shown to others on reports and token sheets." className="flex-1">
        <Input required value={name} onChange={(e) => setName(e.target.value)} autoComplete="name" />
      </Field>
      <Button type="submit" disabled={!name.trim() || save.isPending}>
        Save
      </Button>
      <FormError error={save.error} />
    </form>
  );
}

const firstSteps: Record<SignUpChoices["manages"], { label: string; to?: "/bills" | "/people" | "/close" | "/review"; href?: string }[]> = {
  people: [
    { label: "Connect the bill", to: "/bills" },
    { label: "Invite people", to: "/people" },
    { label: "Close the first period", to: "/close" },
  ],
  agents: [
    { label: "Install the Claude Code mod or the CLI", href: "https://docs.tokencontroller.com/collect/claude-code" },
    { label: "Submit a first token sheet", to: "/review" },
  ],
};

/**
 * The first steps after sign-up. The service takes only the name and the
 * email at sign-up, so the cost source and currency picked there are sent
 * here, once, by the controller who created the organization.
 */
function FirstSteps() {
  const me = useMe();
  const [choices, setChoices] = useState(readSignUp);
  const sent = useRef(false);
  const apply = useMutation({
    mutationFn: (c: SignUpChoices) => api.updateOrganization({ cost_source: c.cost_source, reporting_currency: c.reporting_currency }),
    onSuccess: async (_, c) => {
      const done = { ...c, applied: true };
      rememberSignUp(done);
      setChoices(done);
      await refreshAll();
    },
  });
  const controller = hasRole(me.person, "controller");
  useEffect(() => {
    if (!choices || choices.applied || !controller || sent.current) return;
    sent.current = true;
    apply.mutate(choices);
  }, [choices, controller, apply]);
  if (!choices) {
    return controller && !me.organization.cost_source ? (
      <Note>
        Pick a cost source so the close knows what to check against.{" "}
        <Link to="/settings" className="font-medium underline">
          Settings
        </Link>
      </Note>
    ) : null;
  }
  return (
    <Section title="First steps" actions={<Button size="sm" variant="ghost" onClick={() => { forgetSignUp(); setChoices(null); }}>Hide</Button>}>
      <ol className="grid gap-2 sm:grid-cols-3">
        {firstSteps[choices.manages].map((step, i) => (
          <li key={step.label}>
            {step.to ? (
              <Link to={step.to} className="flex h-full items-center gap-3 rounded-lg border border-line p-3 text-sm font-medium hover:bg-sand">
                <span className="num text-muted">{i + 1}</span> {step.label}
              </Link>
            ) : (
              <a href={step.href} target="_blank" rel="noreferrer" className="flex h-full items-center gap-3 rounded-lg border border-line p-3 text-sm font-medium hover:bg-sand">
                <span className="num text-muted">{i + 1}</span> {step.label}
              </a>
            )}
          </li>
        ))}
      </ol>
    </Section>
  );
}

export function OverviewPage(): ReactNode {
  const me = useMe();
  const billing = useBilling();
  const isController = me.person.role === "controller";
  return (
    <>
      <PageHeader title={me.person.name.trim() ? `Hello, ${me.person.name.trim().split(" ")[0] ?? ""}` : "Hello"} lead={`${me.organization.name} · ${formatPeriod(thisPeriod())}`} />
      <YourName />
      <FirstSteps />
      {isController ? <ControllerView /> : null}
      {hasRole(me.person, "people_manager") ? <PeopleManagerView /> : null}
      {!isController ? (
        <Section title="Your own cost">
          <MyCost />
        </Section>
      ) : null}
      {billing.data?.plan === "solo" ? (
        <p className="text-xs text-muted">
          Solo: one person holds every role.{" "}
          {isController ? (
            <Link to="/billing" className="underline">
              Invite people with the Team plan.
            </Link>
          ) : null}
        </p>
      ) : null}
    </>
  );
}
