import { useMutation, useQuery } from "@tanstack/react-query";
import { getRouteApi } from "@tanstack/react-router";
import { useMemo, useState, type ReactNode } from "react";
import type { TokenEntry } from "@/api/contract";
import { api } from "@/api/endpoints";
import { EntryDialog, entryValue } from "@/components/entry-dialog";
import { PeriodSelect } from "@/components/period-select";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { ErrorBox, Loading, MoneyText, Note, PageHeader } from "@/components/ui/display";
import { Input, Select } from "@/components/ui/form";
import { DataTable, type Column } from "@/components/ui/table";
import { Tag, TagRow } from "@/components/ui/tag";
import { notify } from "@/components/ui/toast";
import { describeFilter, filterFromText, filterToText, formatDay } from "@/lib/format";
import { formatMoney, sum, toDecimal } from "@/lib/money";
import { refreshAll, useFirstOpenPeriod, useMe, usePersonName } from "@/lib/queries";
import { thisPeriod } from "@/lib/today";

const approvalRoute = getRouteApi("/app/approval");

export function ApprovalPage(): ReactNode {
  const me = useMe();
  const name = usePersonName();
  const navigate = approvalRoute.useNavigate();
  const search = approvalRoute.useSearch();
  const firstOpen = useFirstOpenPeriod();
  const period = search.period ?? firstOpen ?? thisPeriod();
  const assigned = me.person.filters;
  const [scope, setScope] = useState("");
  const [extra, setExtra] = useState("");
  const filter = { ...filterFromText(scope), ...filterFromText(extra) };
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [open, setOpen] = useState<TokenEntry | null>(null);
  const entries = useQuery({
    queryKey: ["token-entries", period, "submitted", "", filterToText(filter)],
    queryFn: () => api.tokenEntries({ period, status: "submitted", filter }),
  });
  const rows = useMemo(() => entries.data ?? [], [entries.data]);
  const approve = useMutation({
    mutationFn: (ids: string[]) => api.approve(ids),
    onSuccess: async (r) => {
      setSelected(new Set());
      await refreshAll();
      notify(`${r.approved} token ${r.approved === 1 ? "entry" : "entries"} approved`);
    },
  });
  const allSelected = rows.length > 0 && rows.every((r) => selected.has(r.id));
  const toggle = (id: string, on: boolean) => {
    const next = new Set(selected);
    if (on) next.add(id);
    else next.delete(id);
    setSelected(next);
  };
  const approval = me.organization.approval;
  const columns: Column<TokenEntry>[] = [
    ...(approval
      ? [
          {
            key: "select",
            header: "",
            className: "w-8",
            cell: (e: TokenEntry) => (
              <span onClick={(ev) => ev.stopPropagation()}>
                <Checkbox label={`Select ${e.summary}`} checked={selected.has(e.id)} onCheckedChange={(on) => toggle(e.id, on)} />
              </span>
            ),
          },
        ]
      : []),
    { key: "when", header: "When", className: "whitespace-nowrap", cell: (e) => <span className="num !text-left">{formatDay(e.started_at)}</span>, csv: (e) => e.started_at },
    { key: "person", header: "Agent manager", className: "whitespace-nowrap", cell: (e) => (e.agent_manager ? name(e.agent_manager) : <Badge tone="exception">nobody</Badge>), csv: (e) => e.agent_manager ?? "" },
    { key: "agent", header: "Agent", wide: true, cell: (e) => <span>{e.agent_harness}{e.automatic ? <span className="text-muted"> · automatic</span> : null}</span>, csv: (e) => e.agent_harness },
    {
      key: "summary",
      header: "Summary",
      className: "sm:min-w-72",
      cell: (e) => (
        <span className="block">
          <span className="line-clamp-2 text-ink-soft">{e.summary}</span>
          <span className="mt-1.5 hidden sm:block">
            <TagRow tags={e.splits[0]?.tags ?? {}} />
          </span>
        </span>
      ),
      csv: (e) => e.summary,
    },
    { key: "tags", header: "Tags", csvOnly: true, cell: () => null, csv: (e) => Object.entries(e.splits[0]?.tags ?? {}).map(([k, v]) => `${k}: ${v}`).join(" · ") },
    { key: "cost", header: "Cost", align: "right", cell: (e) => <MoneyText value={entryValue(e)} label={e.value_label} />, csv: (e) => { const v = entryValue(e); return v ? toDecimal(v) : ""; } },
  ];
  const total = sum(rows.map(entryValue).filter((v) => v !== null), me.organization.reporting_currency);
  return (
    <>
      <PageHeader
        title="Approval"
        lead={approval ? "Approve submitted token entries before the period closes, one by one or in bulk after filtering. Send back what is wrong, with a reason." : "Approval is off: submitted token entries count at once. You can still send one back with a reason."}
        actions={
          approval ? (
            <Button variant="primary" disabled={!selected.size || approve.isPending} onClick={() => approve.mutate([...selected])}>
              Approve {selected.size ? selected.size : ""} selected
            </Button>
          ) : undefined
        }
      />
      {assigned.length ? (
        <p className="mb-4 flex flex-wrap items-center gap-1.5 text-[13px] text-muted">
          You approve for
          {assigned.map((f) => (
            <span key={filterToText(f)} className="flex gap-1">
              {Object.entries(f).map(([k, v]) => (
                <Tag key={k} name={k} value={v} />
              ))}
            </span>
          ))}
          and your own token entries.
        </p>
      ) : me.person.role === "controller" ? (
        <Note>As controller you can approve any token entry.</Note>
      ) : null}
      <div className="mb-4 grid gap-2 sm:flex sm:flex-wrap sm:items-end">
        <PeriodSelect value={period} onChange={(p) => void navigate({ search: { period: p } })} />
        {assigned.length > 1 ? (
          <Select aria-label="Filter" value={scope} onChange={(e) => setScope(e.target.value)} className="sm:w-56">
            <option value="">All my filters</option>
            {assigned.map((f) => (
              <option key={filterToText(f)} value={filterToText(f)}>
                {describeFilter(f)}
              </option>
            ))}
          </Select>
        ) : null}
        <Input aria-label="Narrow by tags" value={extra} onChange={(e) => setExtra(e.target.value)} placeholder="Narrow, such as project:storefront" className="sm:w-64" />
      </div>
      {entries.isPending ? <Loading /> : entries.isError ? <ErrorBox error={entries.error} /> : (
        <>
          {approval && rows.length ? (
            <div className="mb-2 flex items-center gap-2 text-[13px] text-muted">
              <Checkbox label="Select all" checked={allSelected} indeterminate={selected.size > 0 && !allSelected} onCheckedChange={(on) => setSelected(on ? new Set(rows.map((r) => r.id)) : new Set())} />
              Select all {rows.length} shown
            </div>
          ) : null}
          <DataTable stack
            columns={columns}
            rows={rows}
            rowKey={(e) => e.id}
            onRowClick={setOpen}
            csvName={`approval-${period}.csv`}
            caption="Submitted token entries"
            empty="Nothing waits for approval here."
            totals={{ label: `${rows.length} waiting`, values: { cost: formatMoney(total) }, csv: { cost: toDecimal(total) } }}
          />
        </>
      )}
      <EntryDialog entry={open} onClose={() => setOpen(null)} />
    </>
  );
}
