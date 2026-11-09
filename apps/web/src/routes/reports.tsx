import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { Filter } from "@/api/contract";
import { api, type ReportQuery } from "@/api/endpoints";
import { Amount, AsOf, FilterField, RangeControls, asOfParam, useRange } from "@/components/controls";
import { Button } from "@/components/ui/button";
import { CheckField } from "@/components/ui/checkbox";
import { ErrorBox, Loading, Note, PageHeader, RowBar, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select } from "@/components/ui/form";
import { DataTable } from "@/components/ui/table";
import { TabPanel, Tabs } from "@/components/ui/tabs";
import { notify } from "@/components/ui/toast";
import { deviationLabelText, describeFilter, labelText } from "@/lib/format";
import { refreshAll, useMe, usePersonName } from "@/lib/queries";

const groupings = [
  { value: "", label: "Nothing: one total" },
  { value: "person", label: "Person" },
  { value: "label", label: "Label" },
  { value: "project", label: "Project" },
  { value: "feature", label: "Feature" },
  { value: "client", label: "Client" },
  { value: "team", label: "Team" },
  { value: "department", label: "Department" },
  { value: "agent_harness", label: "Agent harness" },
  { value: "agent_name", label: "Agent name" },
  { value: "model", label: "Model" },
  { value: "day", label: "Day" },
];

// Docs: Reading reports; Deviation and forecast. Every report is the cost of
// a filter over a date range, as of a date. For one grouping the rows add up
// to the total; rows of different groupings overlap and are never summed.
export function ReportsPage(): ReactNode {
  const me = useMe();
  const [tab, setTab] = useState("cost");
  const [range, setRange] = useRange();
  const [filter, setFilter] = useState<Filter>({});
  const [by, setBy] = useState("project");
  const [approved, setApproved] = useState(false);
  const [share, setShare] = useState(false);
  const query: ReportQuery = { from: range.from, to: range.to, asOf: asOfParam(range.asOf), filter, by, approved, share };
  return (
    <>
      <PageHeader title="Reports" lead="The cost of a filter over a date range, as of a date. Hand finance a report as of the cutoff." />
      <RangeControls value={range} onChange={setRange}>
        <FilterField value={filter} onChange={setFilter} />
        <Field label="Group by" className="col-span-2 sm:w-48">
          <Select value={by} onChange={(e) => setBy(e.target.value)}>
            {groupings.map((g) => (
              <option key={g.value} value={g.value}>{g.label}</option>
            ))}
          </Select>
        </Field>
      </RangeControls>
      <div className="mb-6 flex flex-wrap gap-4">
        {me.organization.approval ? <CheckField checked={approved} onCheckedChange={setApproved} label="Approved messages only" /> : null}
        {tab === "cost" ? <CheckField checked={share} onCheckedChange={setShare} label="Show shares instead of money" /> : null}
      </div>
      <Tabs
        value={tab}
        onValueChange={setTab}
        tabs={[
          { value: "cost", label: "Cost" },
          { value: "deviation", label: "Deviation" },
          { value: "forecast", label: "Forecast" },
        ]}
      >
        <TabPanel value="cost">{tab === "cost" ? <CostTab query={query} /> : null}</TabPanel>
        <TabPanel value="deviation">{tab === "deviation" ? <DeviationTab query={query} /> : null}</TabPanel>
        <TabPanel value="forecast">{tab === "forecast" ? <ForecastTab query={query} /> : null}</TabPanel>
      </Tabs>
      <SaveReport query={query} />
    </>
  );
}

/** A row's key as people read it: a name for a person, the docs' word for a label. */
function useKeyText(by: string | undefined): (key: string | null | undefined) => string {
  const personName = usePersonName();
  return (key) => {
    if (key === null || key === undefined) return by ? `no ${by}` : "Total";
    if (by === "person") return personName(key);
    if (by === "label") return labelText[key] ?? key;
    return key;
  };
}

function CostTab({ query }: { query: ReportQuery }) {
  const report = useQuery({ queryKey: ["report-cost", query], queryFn: () => api.cost(query) });
  const keyText = useKeyText(query.by);
  if (report.error) return <ErrorBox error={report.error} />;
  if (!report.data) return <Loading />;
  const data = report.data;
  return (
    <>
      <p className="mb-3 flex flex-wrap items-baseline gap-2 text-sm">
        <span className="font-medium">{describeFilter(data.filter)}</span>
        <AsOf at={data.as_of} />
      </p>
      {data.count_errors || data.unknown_models.length ? (
        <Note tone="exception">
          Not in the total: {data.count_errors ? `${String(data.count_errors)} count errors` : ""}
          {data.count_errors && data.unknown_models.length ? "; " : ""}
          {data.unknown_models.length ? `unknown models ${data.unknown_models.join(", ")}` : ""}.
        </Note>
      ) : null}
      <DataTable
        caption="Cost report"
        stack
        csvName={`cost-${data.from}-${data.to}.csv`}
        rows={data.rows}
        rowKey={(r) => r.key ?? "none"}
        empty="No cost in this filter and range."
        totals={{ label: "Total", values: { amount: <Amount value={data.total} currency={data.currency} /> }, csv: { amount: data.total } }}
        columns={[
          { key: "key", header: query.by ? (groupings.find((g) => g.value === query.by)?.label ?? query.by) : "Filter", cell: (r) => keyText(r.key), csv: (r) => r.key ?? "" },
          { key: "bar", header: "", wide: true, cell: (r) => <RowBar share={Number(r.share) * 10_000} /> },
          { key: "messages", header: "Messages", align: "right", wide: true, cell: (r) => <span className="num">{r.messages}</span>, csv: (r) => r.messages },
          { key: "amount", header: data.view === "share" ? "Share" : "Cost", align: "right", cell: (r) => (data.view === "share" ? <span className="num">{(Number(r.amount) * 100).toFixed(1)} %</span> : <Amount value={r.amount} currency={data.currency} />), csv: (r) => r.amount },
        ]}
      />
    </>
  );
}

function DeviationTab({ query }: { query: ReportQuery }) {
  const report = useQuery({ queryKey: ["report-deviation", query], queryFn: () => api.deviation(query) });
  const keyText = useKeyText(query.by);
  if (report.error) return <ErrorBox error={report.error} />;
  if (!report.data) return <Loading />;
  const data = report.data;
  return (
    <>
      <p className="mb-3 text-sm text-muted">Deviation is plan minus cost. Positive is favourable. <AsOf at={data.as_of} /></p>
      <DataTable
        caption="Deviation report"
        stack
        csvName={`deviation-${data.from}-${data.to}.csv`}
        rows={data.rows}
        rowKey={(r) => r.key ?? "none"}
        empty="Nothing in this filter and range."
        columns={[
          { key: "key", header: query.by || "Filter", cell: (r) => keyText(r.key), csv: (r) => r.key ?? "" },
          { key: "plan", header: "Plan", align: "right", cell: (r) => (r.plan ? <Amount value={r.plan} currency={data.currency} /> : <span className="text-muted">no plan</span>), csv: (r) => r.plan ?? "" },
          { key: "cost", header: "Cost", align: "right", cell: (r) => <Amount value={r.cost} currency={data.currency} />, csv: (r) => r.cost },
          {
            key: "deviation",
            header: "Deviation",
            align: "right",
            cell: (r) => (r.deviation ? <span className={r.favourable ? "text-posted" : "text-exception"}><Amount value={r.deviation} currency={data.currency} signed /></span> : null),
            csv: (r) => r.deviation ?? "",
          },
          { key: "label", header: "Label", cell: (r) => (r.label ? (deviationLabelText[r.label] ?? r.label) : ""), csv: (r) => r.label ?? "" },
        ]}
      />
    </>
  );
}

function ForecastTab({ query }: { query: ReportQuery }) {
  const report = useQuery({ queryKey: ["report-forecast", query], queryFn: () => api.forecast(query) });
  const keyText = useKeyText(query.by);
  if (report.error) return <ErrorBox error={report.error} />;
  if (!report.data) return <Loading />;
  const data = report.data;
  return (
    <>
      <p className="mb-3 text-sm text-muted">Where cost is heading in this range, from the cost per day of the three ranges of its length before it. Nothing is stored.</p>
      <DataTable
        caption="Forecast report"
        stack
        rows={data.rows}
        rowKey={(r) => r.key ?? "none"}
        empty="No cost in the ranges before this one."
        columns={[
          { key: "key", header: query.by || "Filter", cell: (r) => keyText(r.key) },
          ...[0, 1, 2].map((i) => ({
            key: `past${String(i)}`,
            header: data.rows[0]?.past[i] ? `${data.rows[0].past[i].from.slice(5)} – ${data.rows[0].past[i].to.slice(5)}` : "",
            align: "right" as const,
            wide: true,
            cell: (r: (typeof data.rows)[number]) => <Amount value={r.past[i]?.cost ?? null} currency={data.currency} missing="" />,
          })),
          { key: "forecast", header: "Forecast", align: "right", cell: (r) => <Amount value={r.forecast} currency={data.currency} /> },
        ]}
      />
    </>
  );
}

function SaveReport({ query }: { query: ReportQuery }) {
  const saved = useQuery({ queryKey: ["saved-reports"], queryFn: api.savedReports });
  const [name, setName] = useState("");
  const save = useMutation({
    mutationFn: () => api.saveReport({ name, from: query.from, to: query.to, filter: query.filter ?? {}, ...(query.by ? { by: query.by } : {}) }),
    onSuccess: async () => {
      await refreshAll();
      notify("Report saved", name);
      setName("");
    },
  });
  return (
    <Section title="Saved reports" lead="A filter, a grouping and a range, kept for the organization. Everyone sees it with the cost their grants cover.">
      {saved.data?.length ? (
        <ul className="mb-3 flex flex-col gap-1 text-sm">
          {saved.data.map((r) => (
            <li key={r.id}>
              <span className="font-medium">{r.name}</span> <span className="text-muted">· {describeFilter(r.filter)}{r.by ? ` by ${r.by}` : ""}{r.from && r.to ? ` · ${r.from} to ${r.to}` : ""}</span>
            </li>
          ))}
        </ul>
      ) : null}
      <form
        className="flex max-w-md items-end gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          save.mutate();
        }}
      >
        <Field label="Report name" className="flex-1"><Input value={name} onChange={(e) => setName(e.target.value)} /></Field>
        <Button type="submit" disabled={!name.trim() || save.isPending}>Save this report</Button>
      </form>
      <FormError error={save.error} />
    </Section>
  );
}
