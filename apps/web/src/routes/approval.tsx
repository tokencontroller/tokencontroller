import { useMutation, useQuery } from "@tanstack/react-query";
import { getRouteApi } from "@tanstack/react-router";
import { useMemo, useState, type ReactNode } from "react";
import type { EntryView } from "@/api/contract";
import { api } from "@/api/endpoints";
import { EntryDialog, entryValue, listTotal, valueLabel } from "@/components/entry-dialog";
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
import { formatMoney, toDecimal } from "@/lib/money";
import { refreshAll, useFirstOpenPeriod, useMe } from "@/lib/queries";
import { thisPeriod } from "@/lib/today";

const approvalRoute = getRouteApi("/app/approval");

export function ApprovalPage(): ReactNode {
  const me = useMe();
  const navigate = approvalRoute.useNavigate();
  const search = approvalRoute.useSearch();
  const firstOpen = useFirstOpenPeriod();
  const period = search.period ?? firstOpen ?? thisPeriod();
  const assigned = me.person.filters;
  const [scope, setScope] = useState("");
  const [extra, setExtra] = useState("");
  const filter = { ...filterFromText(scope), ...filterFromText(extra) };
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [open, setOpen] = useState<EntryView | null>(null);
  const entries = useQuery({
    queryKey: ["approval", period, filterToText(filter)],
    queryFn: () => api.approvalQueue(period, filter),
  });
  const rows = useMemo(() => entries.data ?? [], [entries.data]);
  const approve = useMutation({
    mutationFn: (ids: string[]) => api.approve({ token_entry_ids: ids }),
    onSuccess: async (r) => {
      setSelected(new Set());
      await refreshAll();
      notify(
        `${String(r.approved)} token ${r.approved === 1 ? "entry" : "entries"} approved`,
        r.skipped ? `${String(r.skipped)} skipped: no longer submitted, or not within your filters.` : undefined,
      );
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
  const columns: Column<EntryView>[] = [
    ...(approval
      ? [
          {
            key: "select",
            header: "",
            className: "w-8",
            cell: (e: EntryView) => (
              <span onClick={(ev) => ev.stopPropagation()}>
                <Checkbox label={`Select ${e.summary}`} checked={selected.has(e.id)} onCheckedChange={(on) => toggle(e.id, on)} />
              </span>
            ),
          },
        ]
      : []),
    { key: "when", header: "When", className: "whitespace-nowrap", cell: (e) => <span className="num !text-left">{formatDay(e.started_at)}</span>, csv: (e) => e.started_at },
    { key: "person", header: "Agent manager", className: "whitespace-nowrap", cell: (e) => (e.agent_manager ?? <Badge tone="exception">nobody</Badge>), csv: (e) => e.agent_manager ?? "" },
    { key: "agent", header: "Agent", wide: true, cell: (e) => <span>{e.agent_harness}{e.agent_name ? <span className="text-muted"> · {e.agent_name}</span> : null}{e.automatic ? <span className="text-muted"> · automatic</span> : null}</span>, csv: (e) => e.agent_harness },
    {
      key: "summary",
      header: "Summary",
      className: "sm:min-w-72",
      cell: (e) => (
        <span className="block">
          <span className="line-clamp-2 text-ink-soft">{e.summary}</span>
          <span className="mt-1.5 hidden sm:block">
            {e.splits.length > 1 ? <span className="mb-1 block text-xs text-muted">{e.splits.length} splits</span> : null}
            <TagRow tags={e.splits[0]?.tags ?? {}} />
          </span>
        </span>
      ),
      csv: (e) => e.summary,
    },
    { key: "tags", header: "Tags", csvOnly: true, cell: () => null, csv: (e) => e.splits.map((s) => Object.entries(s.tags).filter(([k]) => k !== "person").map(([k, v]) => `${k}: ${v}`).join(" · ")).join(" | ") },
    {
      key: "cost",
      header: "List value",
      align: "right",
      cell: (e) => <MoneyText value={entryValue(e)} label={valueLabel(e)} />,
      csv: (e) => {
        const v = entryValue(e);
        return v ? toDecimal(v) : "unknown model";
      },
    },
  ];
  const total = listTotal(rows, me.organization.reporting_currency);
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
      {me.person.role === "controller" ? (
        <Note>As controller you can approve any token entry.</Note>
      ) : assigned.length ? (
        <p className="mb-4 flex flex-wrap items-center gap-1.5 text-[13px] text-muted">
          You approve within
          {assigned.map((f) => (
            <span key={filterToText(f)} className="flex gap-1">
              {Object.entries(f).map(([k, v]) => (
                <Tag key={k} name={k} value={v} />
              ))}
            </span>
          ))}
          except your own token entries.
        </p>
      ) : (
        <Note tone="exception">No filters are assigned to you yet. A controller assigns them under People.</Note>
      )}
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
            totals={{ label: `${String(rows.length)} waiting`, values: { cost: formatMoney(total) }, csv: { cost: toDecimal(total) } }}
          />
          {approve.error ? <ErrorBox error={approve.error} /> : null}
        </>
      )}
      <EntryDialog entry={open} onClose={() => setOpen(null)} />
    </>
  );
}
