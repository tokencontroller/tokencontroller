import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { api } from "@/api/endpoints";
import { Amount, AsOf } from "@/components/controls";
import { Badge, LabelBadge } from "@/components/ui/badge";
import { buttonClass } from "@/components/ui/button";
import { ErrorBox, Figure, Kpi, KpiRow, Loading, Note, PageHeader, Section, linkClass } from "@/components/ui/display";
import { DataTable } from "@/components/ui/table";
import { additions } from "@/lib/additions";
import { describeFilter, formatRange, lastMonth } from "@/lib/format";
import { useMe } from "@/lib/queries";
import { hasRole } from "@/lib/roles";
import { today } from "@/lib/today";

// Docs: How it works. The cost of what you may view over the current bill's
// range, as of now; the reconciliation line of that bill; open deviations.
export function OverviewPage(): ReactNode {
  const me = useMe();
  const orgManager = hasRole(me.person, "org_manager");
  const reconciliation = useQuery({ queryKey: ["reconciliation"], queryFn: api.reconciliation, enabled: orgManager });
  const latest = reconciliation.data?.toSorted((a, b) => b.to.localeCompare(a.to))[0];
  const range = latest ? { from: latest.from, to: latest.to } : lastMonth(today());
  const cost = useQuery({ queryKey: ["overview-cost", range], queryFn: () => api.cost({ ...range, by: "label" }), enabled: !orgManager || !reconciliation.isLoading });
  const byProject = useQuery({ queryKey: ["overview-deviation", "project", range], queryFn: () => api.deviation({ ...range, by: "project" }), enabled: hasRole(me.person, "people_manager") });
  const byFeature = useQuery({ queryKey: ["overview-deviation", "feature", range], queryFn: () => api.deviation({ ...range, by: "feature" }), enabled: hasRole(me.person, "people_manager") });
  const mine = useQuery({ queryKey: ["overview-deviation", "person", range], queryFn: () => api.deviation({ ...range, filter: { person: me.person.email } }) });
  const open = useQuery({ queryKey: ["sessions", "open"], queryFn: () => api.sessions({ filter: { person: me.person.email } }) });
  const deviations = [...(byProject.data?.rows ?? []), ...(byFeature.data?.rows ?? []), ...(mine.data?.rows ?? [])].filter((r) => r.deviation !== null && r.deviation !== undefined && !r.favourable);
  // Deviation reports come in the organization's reporting currency, all three the same.
  const deviationCurrency = (mine.data ?? byProject.data ?? byFeature.data)?.currency;
  const toCheck = open.data?.sessions.filter((s) => s.submitted === "open" || s.submitted === "rejected") ?? [];
  const name = me.person.name.split(" ")[0] ?? "";
  const lineOf = (label: string) => cost.data?.rows.find((r) => r.key === label)?.amount ?? "0.00";
  const explained = latest && Number(latest.bill) > 0 ? Math.max(0, Math.min(1, Number(latest.messages) / Number(latest.bill))) : null;
  return (
    <>
      <PageHeader title={name ? `Hello, ${name}` : "Hello"} lead={`${me.organization.name} · ${formatRange(range.from, range.to)}`} meta={cost.data ? <AsOf at={cost.data.as_of} /> : null} />
      {cost.error ? <ErrorBox error={cost.error} /> : null}
      {!cost.data ? (
        <Loading />
      ) : (
        <>
          <KpiRow>
            <Kpi hero label="Cost" value={<Amount value={cost.data.total} currency={cost.data.currency} />} note="Everything you may view" />
            <Kpi label="Direct" value={<Amount value={lineOf("direct")} currency={cost.data.currency} />} note="On a feature" />
            <Kpi label="Project overhead" value={<Amount value={lineOf("project_overhead")} currency={cost.data.currency} />} note="On a project, no feature" />
            <Kpi label="Organization overhead" value={<Amount value={lineOf("organization_overhead")} currency={cost.data.currency} />} note="On no project" tone={Number(lineOf("organization_overhead")) > 0 ? "exception" : undefined} />
          </KpiRow>
          {cost.data.count_errors || cost.data.unknown_models.length ? (
            <Note tone="exception">
              {cost.data.count_errors ? `${String(cost.data.count_errors)} messages came without token counts (count errors). ` : ""}
              {cost.data.unknown_models.length ? `Not priced yet: ${cost.data.unknown_models.join(", ")}.` : ""} Their cost is not in these figures.
            </Note>
          ) : null}
        </>
      )}

      {orgManager && latest ? (
        <Section
          title="Reconciliation"
          hint={`The ${latest.provider} bill for ${formatRange(latest.from, latest.to)}: the cost of its messages plus what no message explains.`}
          actions={<Link to="/bills" className={linkClass}>Bills</Link>}
        >
          <div className="grid grid-cols-2 items-end gap-4 sm:grid-cols-[1.5fr_1fr_1fr] max-sm:[&>*:first-child]:col-span-2">
            <Figure size="lg" label="No message explains" tone={latest.a_message_is_wrong ? "exception" : undefined} value={<Amount value={latest.unexplained} currency={latest.currency} />} />
            <Figure label="Bill" value={<Amount value={latest.bill} currency={latest.currency} />} />
            <Figure label="Cost of messages" value={<Amount value={latest.messages} currency={latest.currency} />} />
          </div>
          {explained !== null ? (
            <div className="mt-4 flex h-2 overflow-hidden rounded-full bg-sand" role="img" aria-label={`Messages explain ${String(Math.round(explained * 100))} % of the bill`}>
              <span className="h-full bg-posted" style={{ width: `${String(explained * 100)}%` }} />
            </div>
          ) : null}
          {latest.a_message_is_wrong ? <p className="mt-2 text-small text-exception">Negative: a message is wrong.</p> : null}
        </Section>
      ) : null}

      <Section title="Open deviations" flush hint="Plan minus cost where it is negative, in the same range. Positive is favourable.">
        <DataTable
          caption="Open deviations"
          rows={deviations}
          rowKey={(r) => describeFilter(r.filter)}
          empty="No unfavourable deviations in this range."
          columns={[
            { key: "filter", header: "Filter", cell: (r) => <span className="font-medium">{describeFilter(r.filter)}</span> },
            { key: "plan", header: "Plan", align: "right", wide: true, cell: (r) => <Amount value={r.plan} currency={deviationCurrency} /> },
            { key: "cost", header: "Cost", align: "right", wide: true, cell: (r) => <Amount value={r.cost} currency={deviationCurrency} /> },
            { key: "deviation", header: "Deviation", align: "right", cell: (r) => <span className="font-medium text-exception"><Amount value={r.deviation} currency={deviationCurrency} signed /></span> },
            { key: "label", header: "Label", cell: (r) => (r.label ? <Badge>{r.label}</Badge> : null) },
          ]}
        />
      </Section>

      <Section title="Your sessions to check" actions={<Link to="/sessions" className={toCheck.length ? buttonClass("primary", "sm") : linkClass}>Check and submit</Link>}>
        {open.data ? (
          toCheck.length ? (
            <ul className="-mx-5 flex flex-col text-body">
              {toCheck.map((s) => (
                <li key={s.session_id} className="flex flex-wrap items-center gap-2 border-t border-line px-5 py-2.5 first:border-t-0">
                  <span className="min-w-0 flex-1 truncate">{s.summary}</span>
                  {s.parts.map((p) => <LabelBadge key={p.fraction + p.label} label={p.label} />)}
                  <span className="w-24 text-right"><Amount value={s.cost} /></span>
                </li>
              ))}
            </ul>
          ) : (
            <p className="text-small text-muted">Nothing to check.</p>
          )
        ) : (
          <Loading />
        )}
      </Section>
      {additions.parts.OverviewNote ? <additions.parts.OverviewNote /> : null}
    </>
  );
}
