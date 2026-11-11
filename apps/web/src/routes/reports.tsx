import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { Filter } from "@/api/contract";
import { api, type ReportQuery } from "@/api/endpoints";
import { Amount, AsOf, FilterField, RangeControls, asOfParam, useRange } from "@/components/controls";
import { Button } from "@/components/ui/button";
import { CheckField } from "@/components/ui/checkbox";
import { Badge } from "@/components/ui/badge";
import { Card, ErrorBox, Loading, Note, PageHeader, RowBar, Section } from "@/components/ui/display";
import { Dialog } from "@/components/ui/dialog";
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
      <PageHeader title="Reports" lead="Cost of a filter over a range, as of a date." actions={<SaveReport query={query} />} />
      <Card>
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
      {me.organization.approval || tab === "cost" ? (
        <div className="mt-4 flex flex-wrap gap-x-5 gap-y-2 border-t border-line pt-3">
          {me.organization.approval ? <CheckField checked={approved} onCheckedChange={setApproved} label="Approved messages only" /> : null}
          {tab === "cost" ? <CheckField checked={share} onCheckedChange={setShare} label="Show shares instead of money" /> : null}
        </div>
      ) : null}
      </Card>
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
      <SavedReports />
    </>
  );
}

/** The first column's header: the grouping's name, or "Filter" for one total. */
function groupLabel(by: string | undefined): string {
  return by ? (groupings.find((g) => g.value === by)?.label ?? by) : "Filter";
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
    <Section title={describeFilter(data.filter)} flush>
      <div className="flex flex-wrap items-baseline gap-x-3 px-5 pb-4">
        <span className="text-figure font-semibold num !text-left">{data.view === "share" ? "100 %" : <Amount value={data.total} currency={data.currency} />}</span>
        <AsOf at={data.as_of} />
      </div>
      {data.count_errors || data.unknown_models.length ? (
        <div className="px-5 [&>*]:mb-3"><Note tone="exception">
          Not in the total: {data.count_errors ? `${String(data.count_errors)} count errors` : ""}
          {data.count_errors && data.unknown_models.length ? "; " : ""}
          {data.unknown_models.length ? `not priced yet: ${data.unknown_models.join(", ")}` : ""}.
        </Note></div>
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
          { key: "key", header: groupLabel(query.by), cell: (r) => keyText(r.key), csv: (r) => r.key ?? "" },
          { key: "bar", header: "", wide: true, cell: (r) => <RowBar share={Number(r.share) * 10_000} /> },
          { key: "messages", header: "Messages", align: "right", wide: true, cell: (r) => <span className="num">{r.messages}</span>, csv: (r) => r.messages },
          { key: "amount", header: data.view === "share" ? "Share" : "Cost", align: "right", cell: (r) => (data.view === "share" ? <span className="num">{(Number(r.amount) * 100).toFixed(1)} %</span> : <Amount value={r.amount} currency={data.currency} />), csv: (r) => r.amount },
        ]}
      />
    </Section>
  );
}

function DeviationTab({ query }: { query: ReportQuery }) {
  const report = useQuery({ queryKey: ["report-deviation", query], queryFn: () => api.deviation(query) });
  const keyText = useKeyText(query.by);
  if (report.error) return <ErrorBox error={report.error} />;
  if (!report.data) return <Loading />;
  const data = report.data;
  return (
    <Section title={describeFilter(data.filter)} flush hint={<>Deviation is plan minus cost. Positive is favourable. <AsOf at={data.as_of} /></>}>
      <DataTable
        caption="Deviation report"
        stack
        csvName={`deviation-${data.from}-${data.to}.csv`}
        rows={data.rows}
        rowKey={(r) => r.key ?? "none"}
        empty="Nothing in this filter and range."
        columns={[
          { key: "key", header: groupLabel(query.by), cell: (r) => keyText(r.key), csv: (r) => r.key ?? "" },
          { key: "plan", header: "Plan", align: "right", cell: (r) => (r.plan ? <Amount value={r.plan} currency={data.currency} /> : <span className="text-muted">no plan</span>), csv: (r) => r.plan ?? "" },
          { key: "cost", header: "Cost", align: "right", cell: (r) => <Amount value={r.cost} currency={data.currency} />, csv: (r) => r.cost },
          {
            key: "deviation",
            header: "Deviation",
            align: "right",
            cell: (r) => (r.deviation ? <span className={r.favourable ? "text-posted" : "text-exception"}><Amount value={r.deviation} currency={data.currency} signed /></span> : null),
            csv: (r) => r.deviation ?? "",
          },
          { key: "label", header: "Label", cell: (r) => (r.label ? <Badge>{deviationLabelText[r.label] ?? r.label}</Badge> : ""), csv: (r) => r.label ?? "" },
        ]}
      />
    </Section>
  );
}

function ForecastTab({ query }: { query: ReportQuery }) {
  const report = useQuery({ queryKey: ["report-forecast", query], queryFn: () => api.forecast(query) });
  const keyText = useKeyText(query.by);
  if (report.error) return <ErrorBox error={report.error} />;
  if (!report.data) return <Loading />;
  const data = report.data;
  return (
    <Section title={describeFilter(data.filter)} flush hint="Where cost is heading in this range, from the cost per day of the three ranges of its length before it. Nothing is stored.">
      <DataTable
        caption="Forecast report"
        stack
        rows={data.rows}
        rowKey={(r) => r.key ?? "none"}
        empty="No cost in the ranges before this one."
        columns={[
          { key: "key", header: groupLabel(query.by), cell: (r) => keyText(r.key) },
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
    </Section>
  );
}

/** Saves the report on screen for the organization: a button in the page header and its dialog. */
function SaveReport({ query }: { query: ReportQuery }) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const save = useMutation({
    mutationFn: () => api.saveReport({ name, from: query.from, to: query.to, filter: query.filter ?? {}, ...(query.by ? { by: query.by } : {}) }),
    onSuccess: async () => {
      await refreshAll();
      notify("Report saved", name);
      setName("");
      setOpen(false);
    },
  });
  return (
    <>
      <Button variant="primary" onClick={() => setOpen(true)}>Save report</Button>
      <Dialog open={open} onOpenChange={setOpen} title="Save this report" description="Everyone sees it with the cost their grants cover.">
        <form
          className="flex flex-col gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            save.mutate();
          }}
        >
          <Field label="Report name"><Input value={name} onChange={(e) => setName(e.target.value)} /></Field>
          <FormError error={save.error} />
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={() => setOpen(false)}>Cancel</Button>
            <Button type="submit" variant="primary" disabled={!name.trim() || save.isPending}>Save this report</Button>
          </div>
        </form>
      </Dialog>
    </>
  );
}

/** The organization's saved reports, when there are any. */
function SavedReports() {
  const saved = useQuery({ queryKey: ["saved-reports"], queryFn: api.savedReports });
  if (!saved.data?.length) return null;
  return (
    <Section title="Saved reports" hint="A filter, a grouping and a range, kept for the organization.">
      <ul className="-mx-5 flex flex-col text-body">
        {saved.data.map((r) => (
          <li key={r.id} className="border-t border-line px-5 py-2.5 first:border-t-0">
            <span className="font-medium">{r.name}</span> <span className="text-small text-muted">· {describeFilter(r.filter)}{r.by ? ` by ${r.by}` : ""}{r.from && r.to ? ` · ${r.from} to ${r.to}` : ""}</span>
          </li>
        ))}
      </ul>
    </Section>
  );
}
