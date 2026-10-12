import { useQuery } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { Link } from "@tanstack/react-router";
import type { Filter, ReportResult } from "@/api/contract";
import { api } from "@/api/endpoints";
import { FilterTags } from "@/routes/budgets";
import { Badge } from "@/components/ui/badge";
import { buttonClass } from "@/components/ui/button";
import { Kpi, KpiRow, Loading, MoneyText, Note, PageHeader, RowBar, Section, ShareBar } from "@/components/ui/display";
import { addMonths, describeFilter, filterToText, formatPeriod } from "@/lib/format";
import { formatMoney, formatShare, shareOf } from "@/lib/money";
import { useFirstOpenPeriod, useMe, usePeriods, usePersonName } from "@/lib/queries";
import { hasRole } from "@/lib/roles";
import { thisPeriod } from "@/lib/today";

/** Direct, project overhead and organization overhead from two groupings of one filter. */
function useCostKinds(period: string, filter: Filter) {
  const byTicket = useQuery({ queryKey: ["report", period, filterToText(filter), "ticket"], queryFn: () => api.cost({ period, filter, group_by: "ticket" }) });
  const byProject = useQuery({ queryKey: ["report", period, filterToText(filter), "project"], queryFn: () => api.cost({ period, filter, group_by: "project" }) });
  if (!byTicket.data || !byProject.data) return null;
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
  const kinds = useCostKinds(period, { person: me.person.id });
  const budgets = useQuery({ queryKey: ["budgets", period], queryFn: () => api.budgets(period) });
  const myBudget = budgets.data?.find((b) => filterToText(b.filter) === `person:${me.person.id}`);
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

export function OverviewPage(): ReactNode {
  const me = useMe();
  const isController = me.person.role === "controller";
  return (
    <>
      <PageHeader title={`Hello, ${me.person.name.split(" ")[0] ?? ""}`} lead={`${me.organization.name} · ${formatPeriod(thisPeriod())}`} />
      {isController ? <ControllerView /> : null}
      {hasRole(me.person, "people_manager") ? <PeopleManagerView /> : null}
      {!isController ? (
        <Section title="Your own cost">
          <MyCost />
        </Section>
      ) : null}
      {me.organization.plan === "solo" ? (
        <p className="text-xs text-muted">Solo plan: one person holds every role. Invite people with the Team plan under Settings.</p>
      ) : null}
    </>
  );
}

