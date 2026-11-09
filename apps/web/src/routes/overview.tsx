import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { api } from "@/api/endpoints";
import { Amount, AsOf } from "@/components/controls";
import { LabelBadge } from "@/components/ui/badge";
import { ErrorBox, Kpi, KpiRow, Loading, Note, PageHeader, Section } from "@/components/ui/display";
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
  return (
    <>
      <PageHeader title={name ? `Hello, ${name}` : "Hello"} lead={`${me.organization.name}: cost of what you may view, ${formatRange(range.from, range.to)}.`} />
      {cost.error ? <ErrorBox error={cost.error} /> : null}
      {!cost.data ? (
        <Loading />
      ) : (
        <>
          <KpiRow>
            <Kpi label="Cost" value={<Amount value={cost.data.total} currency={cost.data.currency} />} note={<AsOf at={cost.data.as_of} />} />
            <Kpi label="Direct" value={<Amount value={lineOf("direct")} currency={cost.data.currency} />} note="On a feature" />
            <Kpi label="Project overhead" value={<Amount value={lineOf("project_overhead")} currency={cost.data.currency} />} note="On a project, no feature" />
            <Kpi label="Organization overhead" value={<Amount value={lineOf("organization_overhead")} currency={cost.data.currency} />} note="On no project" tone={Number(lineOf("organization_overhead")) > 0 ? "exception" : undefined} />
          </KpiRow>
          {cost.data.count_errors || cost.data.unknown_models.length ? (
            <Note tone="exception">
              {cost.data.count_errors ? `${String(cost.data.count_errors)} messages came without token counts (count errors). ` : ""}
              {cost.data.unknown_models.length ? `Unknown models, with no price: ${cost.data.unknown_models.join(", ")}.` : ""} Their cost is not in these figures.
            </Note>
          ) : null}
        </>
      )}

      {orgManager && latest ? (
        <Section title="Reconciliation" lead={`The ${latest.provider} bill for ${formatRange(latest.from, latest.to)}: cost of its messages plus what no message explains.`} actions={<Link to="/bills" className="text-sm text-accent">Bills</Link>}>
          <dl className="grid max-w-xl grid-cols-[1fr_auto] gap-x-6 gap-y-1 text-sm">
            <dt className="text-muted">Bill</dt>
            <dd><Amount value={latest.bill} currency={latest.currency} /></dd>
            <dt className="text-muted">Cost of messages</dt>
            <dd><Amount value={latest.messages} currency={latest.currency} /></dd>
            <dt className="text-muted">What the bill has that no message explains</dt>
            <dd className={latest.a_message_is_wrong ? "text-exception" : ""}><Amount value={latest.unexplained} currency={latest.currency} /></dd>
          </dl>
          {latest.a_message_is_wrong ? <p className="mt-2 text-sm text-exception">Negative: a message is wrong.</p> : null}
        </Section>
      ) : null}

      <Section title="Open deviations" lead="Plan minus cost where it is negative, in the same range. Positive is favourable.">
        <DataTable
          caption="Open deviations"
          rows={deviations}
          rowKey={(r) => describeFilter(r.filter)}
          empty="No unfavourable deviations in this range."
          columns={[
            { key: "filter", header: "Filter", cell: (r) => describeFilter(r.filter) },
            { key: "plan", header: "Plan", align: "right", cell: (r) => <Amount value={r.plan} currency={deviationCurrency} /> },
            { key: "cost", header: "Cost", align: "right", cell: (r) => <Amount value={r.cost} currency={deviationCurrency} /> },
            { key: "deviation", header: "Deviation", align: "right", cell: (r) => <span className="text-exception"><Amount value={r.deviation} currency={deviationCurrency} signed /></span> },
            { key: "label", header: "Label", wide: true, cell: (r) => r.label ?? "" },
          ]}
        />
      </Section>

      <Section title="Your sessions to check" lead="Sessions since your last submit." actions={<Link to="/sessions" className="text-sm text-accent">Check and submit</Link>}>
        {open.data ? (
          <ul className="flex flex-col gap-1 text-sm">
            {toCheck.length ? (
              toCheck.map((s) => (
                <li key={s.session_id} className="flex flex-wrap items-center gap-2">
                  <span className="min-w-0 flex-1 truncate">{s.summary}</span>
                  {s.parts.map((p) => <LabelBadge key={p.fraction + p.label} label={p.label} />)}
                  <Amount value={s.cost} />
                </li>
              ))
            ) : (
              <li className="text-muted">Nothing to check.</li>
            )}
          </ul>
        ) : (
          <Loading />
        )}
      </Section>
      {additions.parts.OverviewNote ? <additions.parts.OverviewNote /> : null}
    </>
  );
}
