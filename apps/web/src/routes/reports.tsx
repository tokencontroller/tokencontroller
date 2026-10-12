import { useMutation, useQuery } from "@tanstack/react-query";
import { getRouteApi } from "@tanstack/react-router";
import { useState, type ReactNode } from "react";
import type { BudgetVsActualRow, Filter, HarnessCut, Outlier, ReportResult, ReportRow, SavedReport } from "@/api/contract";
import { api } from "@/api/endpoints";
import { FilterBuilder, FilterTags } from "@/routes/budgets";
import { PeriodSelect } from "@/components/period-select";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Loading, MoneyText, Note, PageHeader, RowBar, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select } from "@/components/ui/form";
import { Menu } from "@/components/ui/menu";
import { Switch } from "@/components/ui/switch";
import { DataTable, type Column } from "@/components/ui/table";
import { TabPanel, Tabs } from "@/components/ui/tabs";
import { notify } from "@/components/ui/toast";
import { describeFilter, filterToText, formatPeriod } from "@/lib/format";
import { formatShare, toDecimal } from "@/lib/money";
import { refreshAll, useMe, usePersonName } from "@/lib/queries";
import { hasRole } from "@/lib/roles";
import { thisPeriod } from "@/lib/today";

const reportsRoute = getRouteApi("/app/reports");

interface ViewOptions {
  share: boolean;
  currency: string;
}

function ResultTable({ result, view, csvName, valueLabel }: { result: ReportResult; view: ViewOptions; csvName: string; valueLabel: (row: ReportRow) => string }) {
  const max = Math.max(1, ...result.rows.map((r) => r.share));
  const columns: Column<ReportRow>[] = [
    {
      key: "value",
      header: result.group_by.replace("_", " "),
      cell: (r) => (
        <span className="flex items-center gap-2">
          {r.value === null ? <span className="text-muted">{valueLabel(r)}</span> : valueLabel(r)}
        </span>
      ),
      csv: (r) => valueLabel(r),
    },
    { key: "bar", header: "", wide: true, className: "w-1/3", cell: (r) => <RowBar share={(r.share * 10000) / max} color={r.value === null ? "var(--color-line-strong)" : "var(--color-accent)"} /> },
    {
      key: "amount",
      header: view.share ? "Share" : "Cost",
      align: "right",
      cell: (r) => (view.share ? formatShare(r.share) : <MoneyText value={r.amount} />),
      csv: (r) => (view.share ? formatShare(r.share) : toDecimal(r.amount)),
    },
    ...(view.share ? [] : [{ key: "share", header: "Share", align: "right" as const, wide: true, cell: (r: ReportRow) => formatShare(r.share), csv: (r: ReportRow) => formatShare(r.share) }]),
  ];
  return (
    <>
      {result.estimate ? <Note tone="exception">Seat plan: every figure is an estimate at API prices. Switch to the share view for percentages of the period.</Note> : null}
      <DataTable
        columns={columns}
        rows={result.rows}
        rowKey={(r) => r.value ?? "(none)"}
        csvName={csvName}
        caption="Report"
        empty="No cost here for this period."
        totals={{ label: "Total", values: { amount: view.share ? "100%" : <MoneyText value={result.total} />, share: "100%" }, csv: { amount: view.share ? "100%" : toDecimal(result.total), share: "100%" } }}
      />
    </>
  );
}

function OwnReports({ period, view, approvedOnly }: { period: string; view: ViewOptions; approvedOnly: boolean }) {
  const me = useMe();
  const saved = useQuery({ queryKey: ["saved-reports"], queryFn: api.savedReports });
  const names = useQuery({ queryKey: ["tag-names"], queryFn: api.tagNames });
  const name = usePersonName();
  const [current, setCurrent] = useState<{ filter: Filter; group_by: string; name: string; id: string | null }>({ filter: me.person.role === "people_manager" ? (me.person.filters[0] ?? {}) : {}, group_by: hasRole(me.person, "people_manager") ? "person" : "project", name: "", id: null });
  const [saving, setSaving] = useState(false);
  const [reportName, setReportName] = useState("");
  const result = useQuery({
    queryKey: ["report", period, filterToText(current.filter), current.group_by, view.currency, approvedOnly],
    queryFn: () => api.cost({ period, filter: current.filter, group_by: current.group_by, currency: view.currency, approved_only: approvedOnly }),
  });
  const save = useMutation({
    mutationFn: () => api.saveReport({ name: reportName, filter: current.filter, group_by: current.group_by }),
    onSuccess: async (r) => {
      await refreshAll();
      setCurrent({ ...current, name: r.name, id: r.id });
      setSaving(false);
      notify("Report saved");
    },
  });
  const remove = useMutation({ mutationFn: api.removeReport, onSuccess: refreshAll });
  const open = (r: SavedReport) => setCurrent({ filter: r.filter, group_by: r.group_by, name: r.name, id: r.id });
  return (
    <div className="grid gap-6 lg:grid-cols-[220px_1fr]">
      <aside>
        <h3 className="mb-2 text-xs font-medium text-muted">Saved reports</h3>
        <ul className="flex gap-1 overflow-x-auto lg:flex-col">
          {(saved.data ?? []).map((r) => (
            <li key={r.id} className="flex shrink-0 items-center">
              <button type="button" onClick={() => open(r)} className={`flex-1 rounded-md px-2 py-1.5 text-left text-sm ${current.id === r.id ? "bg-accent-tint font-medium" : "hover:bg-sand"}`}>
                {r.name}
              </button>
              <Menu label={`Actions for ${r.name}`} items={[{ label: "Delete", danger: true, onClick: () => remove.mutate(r.id) }]} />
            </li>
          ))}
        </ul>
      </aside>
      <div>
        <div className="mb-4 rounded-lg border border-line p-4">
          <FilterBuilder value={current.filter} onChange={(filter) => setCurrent({ ...current, filter, name: "", id: null })} />
          <div className="mt-3 flex flex-wrap items-end gap-2 border-t border-line pt-3">
            <Field label="Group by" className="w-44">
              <Select aria-label="Group by" value={current.group_by} onChange={(e) => setCurrent({ ...current, group_by: e.target.value, name: "", id: null })}>
                {(names.data ?? []).map((n) => (
                  <option key={n.name}>{n.name}</option>
                ))}
              </Select>
            </Field>
            <Button onClick={() => { setReportName(""); setSaving(true); }}>Save as report</Button>
          </div>
        </div>
        <h3 className="mb-1 text-[15px] font-semibold">{current.name || `Cost by ${current.group_by}`}</h3>
        <p className="mb-3 text-[13px] text-muted">
          {describeFilter(current.filter)} · {formatPeriod(period)}
          {approvedOnly ? " · approved only" : ""}
        </p>
        {result.isPending ? <Loading /> : result.isError ? <ErrorBox error={result.error} /> : (
          <ResultTable result={result.data} view={view} csvName={`report-${current.group_by}-${period}.csv`} valueLabel={(r) => (r.value === null ? `no ${current.group_by}` : current.group_by === "person" ? name(r.value) : r.value)} />
        )}
        <p className="mt-3 text-xs text-muted">Lines within one report add up. Reports with different filters overlap, so never add them across reports. Residual has no tags, so only the cost sheet reaches the bill total.</p>
      </div>
      <Dialog open={saving} onOpenChange={setSaving} title="Save this report" description="A report is a saved tag filter with a grouping.">
        <form className="space-y-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
          <Field label="Name">
            <Input required autoFocus value={reportName} onChange={(e) => setReportName(e.target.value)} placeholder="Cost per team for Acme" />
          </Field>
          <FormError error={save.error} />
          <div className="flex justify-end gap-2">
            <Button onClick={() => setSaving(false)}>Cancel</Button>
            <Button type="submit" variant="primary">Save</Button>
          </div>
        </form>
      </Dialog>
    </div>
  );
}

function BudgetVsActual({ period }: { period: string }) {
  const rows = useQuery({ queryKey: ["budget-vs-actual", period], queryFn: () => api.budgetVsActual(period) });
  if (rows.isPending) return <Loading />;
  if (rows.isError) return <ErrorBox error={rows.error} />;
  const columns: Column<BudgetVsActualRow>[] = [
    { key: "filter", header: "Filter", cell: (r) => <FilterTags filter={r.filter} />, csv: (r) => describeFilter(r.filter) },
    { key: "budget", header: "Token budget", align: "right", cell: (r) => <MoneyText value={r.amount} />, csv: (r) => toDecimal(r.amount) },
    { key: "actual", header: "Actual", align: "right", cell: (r) => <MoneyText value={r.actual} />, csv: (r) => toDecimal(r.actual) },
    { key: "difference", header: "Difference", align: "right", cell: (r) => <MoneyText value={r.difference} signed />, csv: (r) => toDecimal(r.difference) },
    {
      key: "verdict",
      header: "",
      cell: (r) => (Number(r.difference.nanos) > 0 ? <Badge tone="exception">overrun</Badge> : <Badge tone="posted">underrun</Badge>),
      csv: (r) => (Number(r.difference.nanos) > 0 ? "overrun" : "underrun"),
    },
  ];
  return (
    <>
      <p className="mb-4 text-sm text-muted">Overruns and underruns both get a look. An overrun may be a problem. An underrun may mean someone can teach the others, or that the next token forecast should be lower.</p>
      <DataTable stack columns={columns} rows={rows.data} rowKey={(r) => r.budget} csvName={`token-budget-vs-actual-${period}.csv`} caption="Token budget vs actual" empty="No token budgets in this period." />
    </>
  );
}

const cuts: { value: HarnessCut; label: string }[] = [
  { value: "agent_harness", label: "Agent harness" },
  { value: "agent_name", label: "Agent name" },
  { value: "model", label: "Model" },
  { value: "effort", label: "Effort" },
];

function AgentHarness({ period, view }: { period: string; view: ViewOptions }) {
  const [cut, setCut] = useState<HarnessCut>("agent_harness");
  const result = useQuery({ queryKey: ["agent-harness", period, cut], queryFn: () => api.agentHarness(period, cut) });
  return (
    <>
      <div className="mb-4 flex flex-wrap gap-1" role="group" aria-label="Cut">
        {cuts.map((c) => (
          <Button key={c.value} size="sm" variant={cut === c.value ? "primary" : "secondary"} onClick={() => setCut(c.value)}>
            {c.label}
          </Button>
        ))}
      </div>
      {result.isPending ? <Loading /> : result.isError ? <ErrorBox error={result.error} /> : (
        <ResultTable result={result.data} view={view} csvName={`agent-harness-${cut}-${period}.csv`} valueLabel={(r) => r.value ?? (cut === "agent_name" ? "no agent name" : "not reported")} />
      )}
    </>
  );
}

function Outliers({ period }: { period: string }) {
  const rows = useQuery({ queryKey: ["outliers", period], queryFn: () => api.outliers(period) });
  if (rows.isPending) return <Loading />;
  if (rows.isError) return <ErrorBox error={rows.error} />;
  const columns: Column<Outlier>[] = [
    { key: "ticket", header: "Ticket", cell: (o) => <span className="font-mono text-[13px]">{o.ticket}</span>, csv: (o) => o.ticket },
    { key: "title", header: "Title", cell: (o) => o.title ?? "", csv: (o) => o.title ?? "" },
    { key: "cost", header: "Cost", align: "right", cell: (o) => <MoneyText value={o.cost} />, csv: (o) => toDecimal(o.cost) },
    { key: "baseline", header: "Baseline", align: "right", wide: true, cell: (o) => <MoneyText value={o.baseline} />, csv: (o) => toDecimal(o.baseline) },
    { key: "ratio", header: "Against baseline", align: "right", cell: (o) => <Badge tone={o.ratio > 100 ? "exception" : "posted"}>{o.ratio >= 100 ? `${(o.ratio / 100).toFixed(1)}× higher` : `${Math.round(100 - o.ratio)}% lower`}</Badge>, csv: (o) => o.ratio / 100 },
  ];
  return (
    <>
      <p className="mb-4 text-sm text-muted">Tickets far from your organization's own baseline for similar work, high or low. An outlier is a question to ask, not an alert: open the token entries and read the summaries.</p>
      <DataTable stack columns={columns} rows={rows.data} rowKey={(o) => o.ticket} csvName={`outliers-${period}.csv`} caption="Outliers" empty="No outliers this period." />
    </>
  );
}

export function ReportsPage(): ReactNode {
  const me = useMe();
  const navigate = reportsRoute.useNavigate();
  const search = reportsRoute.useSearch();
  const period = search.period ?? thisPeriod();
  const [tab, setTab] = useState("own");
  const [view, setView] = useState<ViewOptions>({ share: me.organization.cost_source === "seat_plan", currency: me.organization.reporting_currency });
  const [approvedOnly, setApprovedOnly] = useState(false);
  const tabs = [
    { value: "own", label: hasRole(me.person, "people_manager") ? "Your reports" : "Your cost" },
    ...(hasRole(me.person, "people_manager") ? [{ value: "budgets", label: "Token budget vs actual" }] : []),
    { value: "harness", label: "Agent harness" },
    ...(hasRole(me.person, "people_manager") ? [{ value: "outliers", label: "Outliers" }] : []),
  ];
  return (
    <>
      <PageHeader title="Reports" lead="Cost from both sides: by person and team, and by client, project and ticket. Every number opens down to the token entries and their summaries." />
      <div className="mb-5 flex flex-wrap items-center gap-x-5 gap-y-3">
        <PeriodSelect value={period} onChange={(p) => void navigate({ search: { period: p } })} />
        <Select aria-label="Currency" value={view.currency} onChange={(e) => setView({ ...view, currency: e.target.value })} className="w-28">
          {[me.organization.reporting_currency, ...["USD", "EUR", "GBP", "CHF"].filter((c) => c !== me.organization.reporting_currency)].map((c) => (
            <option key={c}>{c}</option>
          ))}
        </Select>
        <label className="flex items-center gap-2 text-sm">
          <Switch label="Share view" checked={view.share} onCheckedChange={(share) => setView({ ...view, share })} />
          Share view
        </label>
        {me.organization.approval ? (
          <label className="flex items-center gap-2 text-sm">
            <Switch label="Approved only" checked={approvedOnly} onCheckedChange={setApprovedOnly} />
            Approved only
          </label>
        ) : null}
      </div>
      {view.currency !== me.organization.reporting_currency ? <p className="-mt-2 mb-4 text-xs text-muted">Shown in {view.currency} at the rate of the period's last day.</p> : null}
      <Tabs value={tab} onValueChange={setTab} tabs={tabs}>
        <TabPanel value="own">
          <OwnReports period={period} view={view} approvedOnly={approvedOnly} />
        </TabPanel>
        <TabPanel value="budgets">
          <Section title={`Token budget vs actual, ${formatPeriod(period)}`}>
            <BudgetVsActual period={period} />
          </Section>
        </TabPanel>
        <TabPanel value="harness">
          <AgentHarness period={period} view={view} />
        </TabPanel>
        <TabPanel value="outliers">
          <Outliers period={period} />
        </TabPanel>
      </Tabs>
      <p className="mt-8 text-xs text-muted">Figures are list-price estimates unless marked billed. For a spreadsheet or a BI tool, create a read token under Settings and use the reporting API.</p>
    </>
  );
}

