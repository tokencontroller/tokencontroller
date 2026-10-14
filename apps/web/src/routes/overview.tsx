import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import { Link } from "@tanstack/react-router";
import type { CostKind, CostReport, Filter, Manages, Reconciliation } from "@/api/contract";
import { api } from "@/api/endpoints";
import { FilterTags } from "@/routes/budgets";
import { Badge } from "@/components/ui/badge";
import { Button, buttonClass } from "@/components/ui/button";
import { Kpi, KpiRow, Loading, MoneyText, Note, PageHeader, RowBar, Section, ShareBar } from "@/components/ui/display";
import { addMonths, describeFilter, filterToText, formatPeriod } from "@/lib/format";
import { additions } from "@/lib/additions";
import { formatMoney, formatShare, fromDecimal, nanosOf, shareOf, sum } from "@/lib/money";
import { refreshAll, useFirstOpenPeriod, useMe, usePeriods, usePersonName } from "@/lib/queries";
import { Field, FormError, Input } from "@/components/ui/form";
import { forgetFirstSteps, readFirstSteps } from "@/lib/remember";
import { hasRole } from "@/lib/roles";
import { thisPeriod } from "@/lib/today";

/** Direct, project overhead and organization overhead as shares of one filter's cost, and its cost by project. */
function useCostKinds(period: string, filter: Filter) {
  const byKind = useQuery({ queryKey: ["report", period, filterToText(filter), "cost_kind", "share"], queryFn: () => api.cost({ period, filter, by: "cost_kind", share: true }) });
  const byProject = useQuery({ queryKey: ["report", period, filterToText(filter), "project"], queryFn: () => api.cost({ period, filter, by: "project" }) });
  if (!byKind.data || !byProject.data) return null;
  // Nothing recorded yet: no kind has a share.
  const kinds = byKind.data.rows;
  const share = (kind: CostKind) => kinds.find((r) => r.value === kind && !r.adjustment)?.share ?? 0;
  return { direct: share("direct"), project: share("project_overhead"), organization: share("organization_overhead"), total: fromDecimal(byProject.data.total, byProject.data.currency), byProject: byProject.data };
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

function Bars({ result, label }: { result: CostReport; label: (value: string | null) => string }) {
  const rows = result.rows.filter((r) => !r.adjustment).slice(0, 6).map((r) => ({ value: r.value ?? null, amount: fromDecimal(r.amount, result.currency) }));
  const max = rows.reduce((most, r) => (nanosOf(r.amount) > most ? nanosOf(r.amount) : most), 1n);
  return (
    <ul className="space-y-2.5">
      {rows.map((r) => (
        <li key={r.value ?? "none"} className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-x-3 gap-y-1 text-sm sm:grid-cols-[160px_1fr_auto]">
          <span className={`truncate ${r.value === null ? "text-muted" : ""}`}>{label(r.value)}</span>
          <span className="col-span-2 row-start-2 sm:col-span-1 sm:row-start-auto">
            <RowBar share={Number((nanosOf(r.amount) * 10000n) / max)} color={r.value === null ? "var(--color-line-strong)" : "var(--color-accent)"} />
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
  const budgets = useQuery({ queryKey: ["token-budget-report", period], queryFn: () => api.tokenBudgetReport({ period }) });
  const myBudget = budgets.data?.rows.find((r) => filterToText(r.token_budget.filter) === `person:${me.person.email}`);
  const budgetMoney = myBudget && budgets.data ? { amount: fromDecimal(myBudget.amount, budgets.data.currency), actual: fromDecimal(myBudget.actual, budgets.data.currency) } : null;
  const drafts = sheet.data?.token_entries.filter((e) => e.status === "draft").length ?? 0;
  const rejected = sheet.data?.token_entries.filter((e) => e.status === "rejected").length ?? 0;
  const onTickets = sheet.data?.share_on_tickets ?? null;
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
        <Kpi label="Your token budget" value={budgetMoney ? formatMoney(budgetMoney.amount) : "none"} note={budgetMoney ? `${formatShare(shareOf(budgetMoney.actual, budgetMoney.amount))} used` : undefined} />
      </KpiRow>
      {drafts || rejected ? (
        <div className="mb-8 flex flex-col gap-3 rounded-lg border border-line bg-sheet p-4 sm:flex-row sm:items-center sm:justify-between">
          <p className="text-sm">
            {drafts} draft token {drafts === 1 ? "entry" : "entries"}
            {sheet.data ? ` · ${formatMoney(fromDecimal(sheet.data.list_value, sheet.data.currency))}${onTickets === null ? "" : ` · ${formatShare(onTickets)} on tickets`}` : ""}
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
  const byPerson = useQuery({ queryKey: ["report", period, filterToText(filter), "person"], queryFn: () => api.cost({ period, filter, by: "person" }) });
  const kinds = useCostKinds(period, filter);
  return (
    <div className="rounded-lg border border-line p-4">
      <div className="mb-3 flex items-center justify-between gap-2">
        <FilterTags filter={filter} />
        <span className="text-lg font-light num">{byPerson.data ? formatMoney(fromDecimal(byPerson.data.total, byPerson.data.currency)) : "…"}</span>
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
  const waiting = useQuery({ queryKey: ["approval", toApprove, ""], queryFn: () => api.approvalQueue(toApprove) });
  const budgets = useQuery({ queryKey: ["token-budget-report", period], queryFn: () => api.tokenBudgetReport({ period }) });
  const current = useQuery({ queryKey: ["token-entries", period, "all"], queryFn: () => api.tokenEntries({ period, limit: 5000 }) });
  const orphans = current.data?.token_entries.filter((e) => (e.agent_manager ?? null) === null) ?? [];
  const overruns = budgets.data?.rows.filter((r) => r.outcome === "overrun") ?? [];
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
        <Kpi label="Token budgets over" value={budgets.data ? `${overruns.length} of ${budgets.data.rows.length}` : "…"} note={<Link to="/reports" className="text-accent hover:underline">Token budget vs actual</Link>} tone={overruns.length ? "exception" : undefined} />
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

/** The share of a period's bills that token entries explain. */
function attribution(recon: Reconciliation): string {
  const c = recon.reporting_currency;
  const explained = fromDecimal(recon.totals.token_entries, c);
  const gross = sum([explained, fromDecimal(recon.totals.residual, c)], c);
  return nanosOf(gross) > 0n ? formatShare(shareOf(explained, gross)) : "–";
}

function ControllerView() {
  const periods = usePeriods();
  const period = thisPeriod();
  // A new organization has no last month yet: its first close is this month's.
  const previous = addMonths(period, -1);
  const last = periods.data && !periods.data.some((p) => p.period === previous) ? period : previous;
  const recon = useQuery({ queryKey: ["reconciliation", last], queryFn: () => api.reconciliation(last) });
  const kinds = useCostKinds(period, {});
  const lastPeriod = periods.data?.find((p) => p.period === last);
  return (
    <>
      <KpiRow>
        <Kpi label={`Token entries, ${formatPeriod(period)}`} value={kinds ? formatMoney(kinds.total) : "…"} note="so far" />
        <Kpi label={`Attribution, ${formatPeriod(last)}`} value={recon.data ? attribution(recon.data) : "…"} note="share of the bills token entries explain" tone="posted" />
        <Kpi label={`Residual, ${formatPeriod(last)}`} value={recon.data ? formatMoney(fromDecimal(recon.data.totals.residual, recon.data.reporting_currency)) : "…"} note="always shown" />
        <Kpi
          label={`Close, ${formatPeriod(last)}`}
          value={lastPeriod?.closed ? <Badge tone="posted">closed</Badge> : <Badge>open</Badge>}
          note={lastPeriod?.closed ? <Link to="/close/$period" params={{ period: last }} className="text-accent hover:underline">Cost sheet</Link> : <Link to="/close" className="text-accent hover:underline">Run the close</Link>}
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

const firstSteps: Record<Manages, { label: string; to?: "/bills" | "/people" | "/close" | "/review"; href?: string }[]> = {
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

/** The first steps after sign-up, for what the founder manages, until hidden. */
function FirstSteps() {
  const me = useMe();
  const [manages, setManages] = useState(readFirstSteps);
  const controller = hasRole(me.person, "controller");
  if (!manages) {
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
    <Section title="First steps" actions={<Button size="sm" variant="ghost" onClick={() => { forgetFirstSteps(); setManages(null); }}>Hide</Button>}>
      <ol className="grid gap-2 sm:grid-cols-3">
        {firstSteps[manages].map((step, i) => (
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
  const isController = me.person.role === "controller";
  const { OverviewNote } = additions.parts;
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
      {OverviewNote ? <OverviewNote /> : null}
    </>
  );
}
