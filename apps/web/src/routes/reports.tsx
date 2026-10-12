import { useMutation, useQuery } from "@tanstack/react-query";
import { getRouteApi } from "@tanstack/react-router";
import { useState, type ReactNode } from "react";
import type { CostKind, CostReport, CostReportRow, Filter, Outlier, SavedReport, TicketActual, TokenBudgetAdjustment, TokenBudgetEntry, TokenBudgetRow } from "@/api/contract";
import { api, type ReportControls } from "@/api/endpoints";
import { describeApplies, FilterBuilder, FilterTags } from "@/routes/budgets";
import { PeriodSelect } from "@/components/period-select";
import { Badge, costKindLabel, StatusBadge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Kpi, Loading, MoneyText, Note, PageHeader, RowBar, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select } from "@/components/ui/form";
import { Menu } from "@/components/ui/menu";
import { Switch } from "@/components/ui/switch";
import { DataTable, type Column } from "@/components/ui/table";
import { TabPanel, Tabs } from "@/components/ui/tabs";
import { TagRow } from "@/components/ui/tag";
import { notify } from "@/components/ui/toast";
import { describeFilter, filterToText, formatDay, formatPeriod } from "@/lib/format";
import { formatShare, fromDecimal, nanosOf } from "@/lib/money";
import { refreshAll, useMe, usePersonName, useTagNames } from "@/lib/queries";
import { hasRole } from "@/lib/roles";
import { thisPeriod } from "@/lib/today";
import { isFlatFee } from "@/lib/cost-source";

const reportsRoute = getRouteApi("/app/reports");

/** What a report can group by besides tag names (docs: Reading reports, Agent harness reports). */
const groupings: Record<string, string> = {
  agent_harness: "agent harness",
  agent_name: "agent name",
  model: "model",
  effort: "effort",
  speed: "speed",
  day: "day",
  cost_kind: "cost kind",
};

const groupLabel = (by: string) => groupings[by] ?? by;

/** A report line's value in words: a person's name, a cost kind, a day, or the value as it is. */
function useValueLabel(by: string): (row: Pick<CostReportRow, "value" | "adjustment">) => string {
  const name = usePersonName();
  return (row) => {
    const value = row.value ?? null;
    if (value === null) return row.adjustment ? "adjustments" : `no ${groupLabel(by)}`;
    if (by === "person") return name(value);
    if (by === "cost_kind") return Object.hasOwn(costKindLabel, value) ? costKindLabel[value as CostKind] : value;
    if (by === "day") return formatDay(value);
    return value;
  };
}

/** What a report says about itself: the period, whether it is closed, the label and the exchange rate. */
function ReportFacts({ period, closed, label, currency, exchangeRate }: { period: string; closed: boolean; label: string; currency: string; exchangeRate?: string | null | undefined }) {
  const me = useMe();
  return (
    <p className="mb-3 text-[13px] text-muted">
      {formatPeriod(period)} · {closed ? "closed" : "open"} · {label}
      {exchangeRate ? ` · in ${currency} at the rate of the period's last day, 1 ${me.organization.reporting_currency} = ${exchangeRate} ${currency}` : ""}
    </p>
  );
}

interface Line extends CostReportRow {
  key: string;
  money: ReturnType<typeof fromDecimal>;
}

function ReportTable({ report, csvName, caption }: { report: CostReport; csvName: string; caption: string }) {
  const me = useMe();
  const valueLabel = useValueLabel(report.by);
  const share = report.view === "share";
  const lines: Line[] = report.rows.map((r, i) => ({ ...r, key: `${r.adjustment ? "adjustment" : "line"}:${r.value ?? ""}:${String(i)}`, money: fromDecimal(r.amount, report.currency) }));
  const max = lines.filter((r) => !r.adjustment).reduce((most, r) => (nanosOf(r.money) > most ? nanosOf(r.money) : most), 1n);
  const unknown = lines.reduce((sum, r) => sum + r.unknown_model, 0);
  const columns: Column<Line>[] = [
    {
      key: "value",
      header: groupLabel(report.by),
      cell: (r) => (
        <span className="flex flex-wrap items-center justify-end gap-2 sm:justify-start">
          <span className={r.value == null ? "text-muted" : ""}>{valueLabel(r)}</span>
          {r.adjustment ? <Badge tone="accent">adjustment</Badge> : null}
        </span>
      ),
      csv: (r) => valueLabel(r),
    },
    { key: "line", header: "Line", csvOnly: true, cell: () => null, csv: (r) => (r.adjustment ? "adjustment" : "token entries") },
    {
      key: "bar",
      header: "",
      wide: true,
      className: "w-1/3",
      cell: (r) => (r.adjustment ? null : <RowBar share={Number((nanosOf(r.money) * 10000n) / max)} color={r.value == null ? "var(--color-line-strong)" : "var(--color-accent)"} />),
    },
    {
      key: "entries",
      header: "Token entries",
      align: "right",
      wide: true,
      cell: (r) => (r.adjustment ? null : (
        <>
          {r.token_entries}
          {r.unknown_model ? <span className="text-exception"> · {r.unknown_model} unknown model</span> : null}
        </>
      )),
      csv: (r) => r.token_entries,
    },
    { key: "unknown", header: "Unknown model", csvOnly: true, cell: () => null, csv: (r) => r.unknown_model },
    share
      ? {
          key: "share",
          header: "Share",
          align: "right",
          cell: (r) => (r.share == null ? <span className="text-muted" title="Adjustments have no share of the period.">none</span> : formatShare(r.share)),
          csv: (r) => (r.share == null ? "" : formatShare(r.share)),
        }
      : {
          key: "amount",
          header: "Cost",
          align: "right",
          cell: (r) => <MoneyText value={r.money} signed={r.adjustment} />,
          csv: (r) => r.amount,
        },
  ];
  const total = fromDecimal(report.total, report.currency);
  return (
    <>
      <ReportFacts period={report.period} closed={report.closed} label={report.label} currency={report.currency} exchangeRate={report.exchange_rate} />
      {!share && isFlatFee(me.organization.cost_source) ? <Note tone="exception">Seats and subscriptions are flat fees: every figure is an estimate at API prices. The share view shows each line's share of the period instead.</Note> : null}
      <DataTable
        stack
        columns={columns}
        rows={lines}
        rowKey={(r) => r.key}
        csvName={csvName}
        caption={caption}
        empty="No cost here for this period."
        totals={{ label: "Total", values: share ? { share: "100%" } : { amount: <MoneyText value={total} /> }, csv: share ? { share: "100%" } : { amount: report.total } }}
      />
      {unknown ? <p className="mt-2 text-xs text-exception">{unknown} token {unknown === 1 ? "entry is" : "entries are"} on an unknown model: the model is missing from the price table, so its cost is not known yet.</p> : null}
    </>
  );
}

function OwnReports({ controls }: { controls: ReportControls }) {
  const me = useMe();
  const saved = useQuery({ queryKey: ["saved-reports"], queryFn: api.savedReports });
  const names = useTagNames();
  const [current, setCurrent] = useState<{ filter: Filter; by: string; name: string; id: string | null }>({
    filter: me.person.role === "people_manager" ? (me.person.filters[0] ?? {}) : {},
    by: hasRole(me.person, "people_manager") ? "person" : "project",
    name: "",
    id: null,
  });
  const [saving, setSaving] = useState(false);
  const [reportName, setReportName] = useState("");
  const savedId = current.id;
  const result = useQuery({
    queryKey: ["report", savedId ?? "", controls, filterToText(current.filter), current.by],
    // A saved report runs by its id, so it shows what was saved; a changed one runs as a new report.
    queryFn: () => (savedId ? api.savedReport(savedId, controls) : api.cost({ ...controls, by: current.by, filter: current.filter })),
  });
  const save = useMutation({
    mutationFn: () => api.saveReport({ name: reportName.trim(), by: current.by, filter: filterToText(current.filter) }),
    onSuccess: async (r) => {
      await refreshAll();
      setCurrent({ filter: r.filter, by: r.by, name: r.name, id: r.id });
      setSaving(false);
      notify("Report saved");
    },
  });
  const remove = useMutation({
    mutationFn: api.removeReport,
    onSuccess: async (_, id) => {
      await refreshAll();
      if (current.id === id) setCurrent({ ...current, name: "", id: null });
      notify("Report removed");
    },
  });
  const open = (r: SavedReport) => setCurrent({ filter: r.filter, by: r.by, name: r.name, id: r.id });
  // Its creator or a controller can remove a saved report.
  const mayRemove = (r: SavedReport) => hasRole(me.person, "controller") || r.created_by === me.person.name;
  const options = [...(names.data ?? []).map((n) => n.name), ...Object.keys(groupings)];
  if (!options.includes(current.by)) options.unshift(current.by);
  return (
    <div className="grid gap-6 lg:grid-cols-[220px_minmax(0,1fr)]">
      <aside>
        <h3 className="mb-2 text-xs font-medium text-muted">Saved reports</h3>
        {saved.data?.length ? (
          <ul className="-mx-4 flex gap-1 overflow-x-auto px-4 lg:mx-0 lg:flex-col lg:px-0" aria-label="Saved reports">
            {saved.data.map((r) => (
              <li key={r.id} className="flex shrink-0 items-center">
                <button type="button" onClick={() => open(r)} aria-current={current.id === r.id ? "true" : undefined} className={`flex-1 rounded-md px-2 py-1.5 text-left text-sm ${current.id === r.id ? "bg-accent-tint font-medium" : "hover:bg-sand"}`}>
                  {r.name}
                </button>
                {mayRemove(r) ? <Menu label={`Actions for ${r.name}`} items={[{ label: "Remove", danger: true, onClick: () => remove.mutate(r.id) }]} /> : null}
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-sm text-muted">{saved.isPending ? "…" : "None yet. Save a tag filter with a grouping."}</p>
        )}
        <FormError error={remove.error} />
      </aside>
      <div className="min-w-0">
        <div className="mb-4 rounded-lg border border-line p-4">
          <FilterBuilder value={current.filter} onChange={(filter) => setCurrent({ ...current, filter, name: "", id: null })} />
          <div className="mt-3 flex flex-wrap items-end gap-2 border-t border-line pt-3">
            <Field label="Group by" className="w-44">
              <Select aria-label="Group by" value={current.by} onChange={(e) => setCurrent({ ...current, by: e.target.value, name: "", id: null })}>
                {options.map((option) => (
                  <option key={option} value={option}>
                    {groupLabel(option)}
                  </option>
                ))}
              </Select>
            </Field>
            <Button onClick={() => { setReportName(""); setSaving(true); }}>Save as report</Button>
          </div>
        </div>
        <h3 className="mb-1 text-[15px] font-semibold">{current.name || `Cost by ${groupLabel(current.by)}`}</h3>
        <p className="mb-1 text-[13px] text-muted">
          {describeFilter(current.filter)}
          {controls.approvedOnly ? " · approved token entries only" : ""}
        </p>
        {result.isPending ? <Loading /> : result.isError ? <ErrorBox error={result.error} /> : (
          <ReportTable report={result.data} csvName={`report-${current.by}-${controls.period}.csv`} caption="Report" />
        )}
        <p className="mt-3 text-xs text-muted">Add up lines within one report, never across reports: reports with different filters overlap. Residual has no tags, so a filtered report never reaches the bill total; only the cost sheet does.</p>
      </div>
      <Dialog open={saving} onOpenChange={setSaving} title="Save this report" description="A report is a saved tag filter with a grouping.">
        <form className="space-y-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
          <Field label="Report name">
            <Input required autoFocus value={reportName} onChange={(e) => setReportName(e.target.value)} placeholder="Cost per team for Acme" />
          </Field>
          <p className="text-[13px] text-muted">
            {describeFilter(current.filter)}, by {groupLabel(current.by)}
          </p>
          <FormError error={save.error} />
          <div className="flex justify-end gap-2">
            <Button onClick={() => setSaving(false)}>Cancel</Button>
            <Button type="submit" variant="primary" disabled={save.isPending}>Save</Button>
          </div>
        </form>
      </Dialog>
    </div>
  );
}

const outcomes = {
  overrun: { label: "overrun", tone: "exception" },
  underrun: { label: "underrun", tone: "posted" },
  at_token_budget: { label: "at token budget", tone: "neutral" },
} as const;

function OutcomeBadge({ row }: { row: TokenBudgetRow }) {
  if (!row.outcome) return <span className="text-[13px] text-exception" title="Token entries on an unknown model leave it open: their cost is not known yet.">open, unknown model</span>;
  const outcome = outcomes[row.outcome];
  return <Badge tone={outcome.tone}>{outcome.label}</Badge>;
}

const outcomeText = (row: TokenBudgetRow) => (row.outcome ? outcomes[row.outcome].label : "open, unknown model");

const budgetTitle = (row: TokenBudgetRow) => row.token_budget.name ?? describeFilter(row.token_budget.filter);

/** One token budget opened down to its tickets, token entries and adjustments. */
function BudgetDetail({ row, period, approvedOnly, onClose }: { row: TokenBudgetRow; period: string; approvedOnly: boolean; onClose: () => void }) {
  const id = row.token_budget.id;
  const detail = useQuery({ queryKey: ["token-budget-report", id, period, approvedOnly], queryFn: () => api.tokenBudgetDetail(id, period, approvedOnly) });
  const currency = row.token_budget.currency;
  const ticketColumns: Column<TicketActual>[] = [
    { key: "ticket", header: "Ticket", cell: (t) => (t.ticket ? <span className="font-mono text-[13px]">{t.ticket}</span> : <span className="text-muted">no ticket</span>), csv: (t) => t.ticket ?? "" },
    { key: "title", header: "Title", wide: true, cell: (t) => t.title ?? "", csv: (t) => t.title ?? "" },
    { key: "entries", header: "Token entries", align: "right", cell: (t) => (
      <>
        {t.token_entries}
        {t.unknown_model_entries ? <span className="text-exception"> · {t.unknown_model_entries} unknown model</span> : null}
      </>
    ), csv: (t) => t.token_entries },
    { key: "actual", header: "Actual", align: "right", cell: (t) => <MoneyText value={fromDecimal(t.actual, currency)} />, csv: (t) => t.actual },
  ];
  const entryColumns: Column<TokenBudgetEntry>[] = [
    { key: "day", header: "Day", cell: (e) => formatDay(e.started_at), csv: (e) => e.started_at },
    { key: "summary", header: "Summary", cell: (e) => <span className="block min-w-0 text-left">{e.summary}</span>, csv: (e) => e.summary },
    { key: "agent_manager", header: "Agent manager", wide: true, cell: (e) => e.agent_manager ?? "nobody", csv: (e) => e.agent_manager ?? "" },
    { key: "model", header: "Model", wide: "xl", cell: (e) => e.model, csv: (e) => e.model },
    { key: "status", header: "Status", wide: true, cell: (e) => <StatusBadge status={e.status} />, csv: (e) => e.status },
    { key: "tags", header: "Tags", csvOnly: true, cell: () => null, csv: (e) => e.splits.map((s) => describeFilter(s.tags)).join(" | ") },
    { key: "value", header: "List value", align: "right", cell: (e) => <MoneyText value={e.list_value ? fromDecimal(e.list_value, e.currency) : null} />, csv: (e) => e.list_value ?? e.label },
  ];
  const adjustmentColumns: Column<TokenBudgetAdjustment>[] = [
    { key: "tags", header: "On", cell: (a) => <TagRow tags={a.tags} hide={[]} />, csv: (a) => describeFilter(a.tags) },
    { key: "reason", header: "Reason", cell: (a) => (
      <span className="flex flex-wrap items-center justify-end gap-2 sm:justify-start">
        {a.reason}
        {a.late ? <Badge tone="accent">late</Badge> : null}
      </span>
    ), csv: (a) => a.reason },
    { key: "amount", header: "Amount", align: "right", cell: (a) => <MoneyText value={fromDecimal(a.amount, currency)} signed />, csv: (a) => a.amount },
  ];
  const amount = fromDecimal(row.amount, currency);
  return (
    <Dialog open wide onOpenChange={(open) => !open && onClose()} title={budgetTitle(row)} description={<span className="flex flex-wrap items-center gap-2"><FilterTags filter={row.token_budget.filter} /> · {row.actual_period ? formatPeriod(row.actual_period) : "all time"}</span>}>
      <div className="mb-5 grid grid-cols-2 gap-3 sm:grid-cols-3">
        <Kpi label="Token budget" value={<MoneyText value={amount} />} />
        <Kpi label="Actual" value={<MoneyText value={fromDecimal(row.actual, currency)} />} />
        <Kpi label="Difference" value={<MoneyText value={fromDecimal(row.difference, currency)} signed />} note={<OutcomeBadge row={row} />} tone={row.outcome === "overrun" ? "exception" : row.outcome === "underrun" ? "posted" : undefined} />
      </div>
      {detail.isPending ? <Loading /> : detail.isError ? <ErrorBox error={detail.error} /> : (
        <>
          <Section title="Tickets">
            <DataTable stack columns={ticketColumns} rows={detail.data.tickets} rowKey={(t) => t.ticket ?? ""} csvName={`token-budget-tickets-${period}.csv`} caption="Tickets" empty="No token entries on this token budget." />
          </Section>
          <Section title="Token entries" lead="Every token entry carries the agent's summary of what the work was.">
            <DataTable stack columns={entryColumns} rows={detail.data.token_entries} rowKey={(e) => e.id} csvName={`token-budget-token-entries-${period}.csv`} caption="Token entries" empty="No token entries on this token budget." />
          </Section>
          {detail.data.adjustments.length ? (
            <Section title="Adjustments">
              <DataTable stack columns={adjustmentColumns} rows={detail.data.adjustments} rowKey={(a) => a.id} csvName={`token-budget-adjustments-${period}.csv`} caption="Adjustments" />
            </Section>
          ) : null}
        </>
      )}
    </Dialog>
  );
}

function TokenBudgetVsActual({ period, approvedOnly }: { period: string; approvedOnly: boolean }) {
  const report = useQuery({ queryKey: ["token-budget-report", period, approvedOnly], queryFn: () => api.tokenBudgetReport(period, approvedOnly) });
  const [opened, setOpened] = useState<TokenBudgetRow | null>(null);
  if (report.isPending) return <Loading />;
  if (report.isError) return <ErrorBox error={report.error} />;
  const currency = report.data.currency;
  const columns: Column<TokenBudgetRow>[] = [
    {
      key: "budget",
      header: "Token budget",
      cell: (r) => (
        <span className="flex flex-col items-end gap-1 sm:items-start">
          <button type="button" className="text-right font-medium text-accent hover:underline sm:text-left" onClick={(e) => { e.stopPropagation(); setOpened(r); }}>
            {budgetTitle(r)}
          </button>
          {r.token_budget.name ? <FilterTags filter={r.token_budget.filter} /> : null}
        </span>
      ),
      csv: (r) => r.token_budget.name ?? "",
    },
    { key: "filter", header: "Filter", csvOnly: true, cell: () => null, csv: (r) => filterToText(r.token_budget.filter) },
    { key: "applies", header: "Applies", wide: true, cell: (r) => describeApplies(r.token_budget), csv: (r) => r.token_budget.applies },
    { key: "amount", header: "Amount", align: "right", cell: (r) => <MoneyText value={fromDecimal(r.amount, currency)} />, csv: (r) => r.amount },
    {
      key: "actual",
      header: "Actual",
      align: "right",
      cell: (r) => (
        <span className="flex flex-col items-end">
          <MoneyText value={fromDecimal(r.actual, currency)} />
          {r.unknown_model_entries ? <span className="text-xs text-exception">{r.unknown_model_entries} unknown model</span> : null}
        </span>
      ),
      csv: (r) => r.actual,
    },
    { key: "difference", header: "Difference", align: "right", cell: (r) => <MoneyText value={fromDecimal(r.difference, currency)} signed />, csv: (r) => r.difference },
    { key: "outcome", header: "Outcome", cell: (r) => <OutcomeBadge row={r} />, csv: (r) => outcomeText(r) },
    { key: "entries", header: "Token entries", csvOnly: true, cell: () => null, csv: (r) => r.token_entries },
  ];
  return (
    <>
      <p className="mb-2 text-sm text-muted">
        Actual cost against each token budget in force in {formatPeriod(period)}. Overrun is actual above token budget; underrun is actual below. Both get a look: an overrun may be a problem, and an underrun may mean someone can teach the others, or that the next token forecast should be lower.
      </p>
      <p className="mb-4 text-xs text-muted">Token budgets can overlap, so never add them up. Open a row for its tickets and token entries.</p>
      <DataTable
        stack
        columns={columns}
        rows={report.data.rows}
        rowKey={(r) => r.token_budget.id}
        onRowClick={setOpened}
        csvName={`token-budget-vs-actual-${period}.csv`}
        caption="Token budget vs actual"
        empty="No token budgets in force in this period."
      />
      {opened ? <BudgetDetail key={opened.token_budget.id} row={opened} period={period} approvedOnly={approvedOnly} onClose={() => setOpened(null)} /> : null}
    </>
  );
}

const cuts = [
  { value: "agent_harness", label: "Agent harness" },
  { value: "agent_name", label: "Agent name" },
  { value: "model", label: "Model" },
  { value: "effort", label: "Effort" },
  { value: "speed", label: "Speed" },
] as const;

function AgentHarness({ controls }: { controls: ReportControls }) {
  const [cut, setCut] = useState<(typeof cuts)[number]["value"]>("agent_harness");
  const result = useQuery({ queryKey: ["agent-harness", controls, cut], queryFn: () => api.agentHarness({ ...controls, by: cut }) });
  return (
    <>
      <div className="mb-4 flex flex-wrap gap-1" role="group" aria-label="Cut">
        {cuts.map((c) => (
          <Button key={c.value} size="sm" variant={cut === c.value ? "primary" : "secondary"} aria-pressed={cut === c.value} onClick={() => setCut(c.value)}>
            {c.label}
          </Button>
        ))}
      </div>
      {result.isPending ? <Loading /> : result.isError ? <ErrorBox error={result.error} /> : (
        <ReportTable report={result.data} csvName={`agent-harness-${cut}-${controls.period}.csv`} caption="Agent harness report" />
      )}
    </>
  );
}

function Outliers({ period }: { period: string }) {
  const result = useQuery({ queryKey: ["outliers", period], queryFn: () => api.outliers(period) });
  if (result.isPending) return <Loading />;
  if (result.isError) return <ErrorBox error={result.error} />;
  const { currency } = result.data;
  const columns: Column<Outlier>[] = [
    { key: "ticket", header: "Ticket", cell: (o) => <span className="font-mono text-[13px]">{o.ticket}</span>, csv: (o) => o.ticket },
    { key: "project", header: "Project", wide: true, cell: (o) => o.project ?? <span className="text-muted">no project</span>, csv: (o) => o.project ?? "" },
    {
      key: "amount",
      header: "Cost",
      align: "right",
      cell: (o) => (
        <span className="flex flex-col items-end">
          <MoneyText value={fromDecimal(o.amount, currency)} />
          {o.unknown_model ? <span className="text-xs text-exception">{o.unknown_model} unknown model</span> : null}
        </span>
      ),
      csv: (o) => o.amount,
    },
    {
      key: "baseline",
      header: "Baseline",
      align: "right",
      cell: (o) => (
        <span className="flex flex-col items-end">
          <MoneyText value={fromDecimal(o.baseline, currency)} />
          <span className="text-xs text-muted">
            median of {o.similar_tickets} {o.compared_with === "project" ? "in the project" : "in the organization"}
          </span>
        </span>
      ),
      csv: (o) => o.baseline,
    },
    { key: "compared_with", header: "Compared with", csvOnly: true, cell: () => null, csv: (o) => `${o.compared_with}, ${String(o.similar_tickets)} similar tickets` },
    {
      key: "ratio",
      header: "Against baseline",
      align: "right",
      cell: (o) => <Badge tone={o.direction === "high" ? "exception" : "posted"}>{`${o.ratio}× · ${o.direction}`}</Badge>,
      csv: (o) => `${o.ratio} ${o.direction}`,
    },
  ];
  return (
    <>
      <p className="mb-2 text-sm text-muted">
        Tickets furthest from your organization's own baseline for similar work, high or low: a ticket at twice the baseline is as far as one at half. The baseline is the median cost of similar past tickets: those of the same project in the twelve months before the period, or of the whole organization when the project has fewer than three.
      </p>
      <p className="mb-4 text-xs text-muted">An outlier is a question to ask, not an alert: nothing is sent and nothing is stopped. Open the token entries and read the summaries.</p>
      <ReportFacts period={result.data.period} closed={result.data.closed} label={result.data.label} currency={currency} />
      <DataTable stack columns={columns} rows={result.data.tickets} rowKey={(o) => o.ticket} csvName={`outliers-${period}.csv`} caption="Outliers" empty="No outliers this period." />
      {result.data.without_baseline ? (
        <p className="mt-2 text-xs text-muted">
          {result.data.without_baseline} {result.data.without_baseline === 1 ? "ticket has" : "tickets have"} no baseline yet: fewer than three similar past tickets.
        </p>
      ) : null}
    </>
  );
}

export function ReportsPage(): ReactNode {
  const me = useMe();
  const navigate = reportsRoute.useNavigate();
  const search = reportsRoute.useSearch();
  const period = search.period ?? thisPeriod();
  const reporting = me.organization.reporting_currency;
  const [tab, setTab] = useState("own");
  const [share, setShare] = useState(isFlatFee(me.organization.cost_source));
  const [currency, setCurrency] = useState(reporting);
  const [approvedOnly, setApprovedOnly] = useState(false);
  const approval = me.organization.approval;
  const controls: ReportControls = {
    period,
    ...(approval && approvedOnly ? { approvedOnly: true } : {}),
    ...(currency !== reporting ? { currency } : {}),
    ...(share ? { share: true } : {}),
  };
  const tabs = [
    { value: "own", label: hasRole(me.person, "people_manager") ? "Your reports" : "Your cost" },
    { value: "budgets", label: "Token budget vs actual" },
    { value: "harness", label: "Agent harness" },
    { value: "outliers", label: "Outliers" },
  ];
  return (
    <>
      <PageHeader title="Reports" lead="Cost from both sides: by person and team, and by client, project and ticket. Each person sees the reports for what their role covers." />
      <div className="mb-5 flex flex-wrap items-center gap-x-5 gap-y-3">
        <PeriodSelect value={period} onChange={(p) => void navigate({ search: { period: p } })} />
        <Select aria-label="Currency" value={currency} onChange={(e) => setCurrency(e.target.value)} className="w-28">
          {[reporting, ...["USD", "EUR", "GBP", "CHF"].filter((c) => c !== reporting)].map((c) => (
            <option key={c}>{c}</option>
          ))}
        </Select>
        <label className="flex items-center gap-2 text-sm">
          <Switch label="Share view" checked={share} onCheckedChange={setShare} />
          Share view
        </label>
        {approval ? (
          <label className="flex items-center gap-2 text-sm">
            <Switch label="Approved only" checked={approvedOnly} onCheckedChange={setApprovedOnly} />
            Approved only
          </label>
        ) : null}
      </div>
      <Tabs value={tab} onValueChange={setTab} tabs={tabs}>
        <TabPanel value="own">
          <OwnReports controls={controls} />
        </TabPanel>
        <TabPanel value="budgets">
          <Section title={`Token budget vs actual, ${formatPeriod(period)}`}>
            {currency !== reporting || share ? <p className="mb-3 text-xs text-muted">Token budgets are compared in money, in {reporting}.</p> : null}
            <TokenBudgetVsActual period={period} approvedOnly={Boolean(controls.approvedOnly)} />
          </Section>
        </TabPanel>
        <TabPanel value="harness">
          <Section title={`Agent harness reports, ${formatPeriod(period)}`} lead="Cost by agent harness, agent name, model, effort and speed.">
            <AgentHarness controls={controls} />
          </Section>
        </TabPanel>
        <TabPanel value="outliers">
          <Section title={`Outliers, ${formatPeriod(period)}`}>
            <Outliers period={period} />
          </Section>
        </TabPanel>
      </Tabs>
      <p className="mt-8 text-xs text-muted">For a spreadsheet or a BI tool, create a read token under Settings and use the reporting API.</p>
    </>
  );
}
