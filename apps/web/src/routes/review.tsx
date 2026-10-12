import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { SubmitResult, TokenEntry } from "@/api/contract";
import { api } from "@/api/endpoints";
import { EntryDialog, entryValue } from "@/components/entry-dialog";
import { StatusBadge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { Empty, ErrorBox, Kpi, KpiRow, Loading, MoneyText, Note, PageHeader, Section } from "@/components/ui/display";
import { Select } from "@/components/ui/form";
import { DataTable, type Column } from "@/components/ui/table";
import { TagRow } from "@/components/ui/tag";
import { formatDay, formatTime } from "@/lib/format";
import { formatMoney, formatShare, toDecimal } from "@/lib/money";
import { refreshAll } from "@/lib/queries";

function TicketPicker({ entry }: { entry: TokenEntry }) {
  const tickets = useQuery({ queryKey: ["tickets"], queryFn: () => api.tickets() });
  const tags = entry.splits[0]?.tags ?? {};
  const save = useMutation({
    mutationFn: (ticket: string) => {
      const next = Object.fromEntries(Object.entries(tags).filter(([k]) => !["person", "client", "ticket"].includes(k)));
      return api.setTags(entry.id, ticket ? { ...next, ticket } : next);
    },
    onSuccess: refreshAll,
  });
  if (entry.splits.length > 1) return <span className="text-xs text-muted">split</span>;
  const candidates = new Set(entry.candidates);
  const list = [...(tickets.data ?? [])].sort((a, b) => Number(candidates.has(b.key)) - Number(candidates.has(a.key)));
  return (
    <Select
      aria-label={`Ticket for entry ${entry.id}`}
      className="h-8 min-w-36 text-[13px]"
      value={tags.ticket ?? ""}
      disabled={save.isPending}
      onClick={(e) => e.stopPropagation()}
      onChange={(e) => save.mutate(e.target.value)}
    >
      <option value="">{entry.candidates.length ? `Pick one of ${entry.candidates.length}` : "No ticket"}</option>
      {list.map((t) => (
        <option key={t.key} value={t.key}>
          {t.key}
          {candidates.has(t.key) ? " · candidate" : ""}
        </option>
      ))}
    </Select>
  );
}

export function ReviewPage(): ReactNode {
  const sheet = useQuery({ queryKey: ["token-sheet"], queryFn: api.tokenSheet });
  const [open, setOpen] = useState<TokenEntry | null>(null);
  const [result, setResult] = useState<SubmitResult | null>(null);
  const submit = useMutation({
    mutationFn: api.submit,
    onSuccess: async (r) => {
      setResult(r);
      await refreshAll();
    },
  });
  if (sheet.isPending) return <Loading />;
  if (sheet.isError) return <ErrorBox error={sheet.error} />;
  const data = sheet.data;
  const drafts = data.entries.filter((e) => e.status === "draft");
  const rejected = data.entries.filter((e) => e.status === "rejected");
  const columns: Column<TokenEntry>[] = [
    { key: "n", header: "#", cell: (e) => <span className="num text-muted">{drafts.indexOf(e) + 1}</span>, csv: (e) => drafts.indexOf(e) + 1 },
    { key: "when", header: "When", cell: (e) => <span className="num !text-left">{formatDay(e.started_at)} {formatTime(e.started_at)}</span>, csv: (e) => e.started_at },
    { key: "cost", header: "Cost", align: "right", cell: (e) => <MoneyText value={entryValue(e)} label={e.value_label} />, csv: (e) => (entryValue(e) ? toDecimal(entryValue(e) ?? { nanos: 0, currency: "USD" }) : "") },
    { key: "summary", header: "Summary", cell: (e) => <span className="line-clamp-2 text-ink-soft">{e.summary}</span>, csv: (e) => e.summary },
    { key: "ticket", header: "Ticket", cell: (e) => <TicketPicker entry={e} />, csv: (e) => e.splits[0]?.tags.ticket ?? "" },
    { key: "tags", header: "Tags", wide: true, className: "min-w-52", cell: (e) => <TagRow tags={e.splits[0]?.tags ?? {}} hide={["person", "ticket"]} />, csv: (e) => Object.entries(e.splits[0]?.tags ?? {}).map(([k, v]) => `${k}: ${v}`).join(" · ") },
    { key: "by", header: "By", wide: true, cell: (e) => <span className="text-[13px] text-muted">{e.placed_by}</span>, csv: (e) => e.placed_by },
  ];
  return (
    <>
      <PageHeader
        title="Review and submit"
        lead="Check your draft token entries, fix the exceptions, then submit your token sheet. Project, client and team follow from the ticket."
      />
      {data.late ? (
        <Note tone="exception">
          Your token sheet is late: a cadence period is over and not submitted. Nothing is blocked; submit when you can.
        </Note>
      ) : null}
      <KpiRow>
        <Kpi label="Draft token entries" value={drafts.length} />
        <Kpi label="Cost" value={formatMoney(data.total)} note="list-price estimate" />
        <Kpi label="On tickets" value={formatShare(data.on_tickets)} tone={data.on_tickets >= 9000 ? "posted" : undefined} />
        <Kpi label="Cadence period" value={<span className="text-base">{formatDay(data.starts)} to {formatDay(data.ends)}</span>} note={data.cadence ? data.cadence.every : "no cadence"} />
      </KpiRow>

      {rejected.length ? (
        <Section title="Sent back to you" lead="A people manager rejected these with a reason. Fix the tags and they go back on your token sheet.">
          <ul className="space-y-2">
            {rejected.map((e) => (
              <li key={e.id}>
                <button type="button" onClick={() => setOpen(e)} className="flex w-full flex-col gap-1 rounded-md border border-exception/30 bg-exception-tint px-3 py-2.5 text-left text-sm sm:flex-row sm:items-center sm:justify-between">
                  <span>
                    <StatusBadge status="rejected" /> <span className="ml-1 font-medium">{e.summary}</span>
                    <span className="block text-[13px] text-ink-soft">{e.reject_reason}</span>
                  </span>
                  <MoneyText value={entryValue(e)} label={e.value_label} />
                </button>
              </li>
            ))}
          </ul>
        </Section>
      ) : null}

      <Section
        title="Draft token entries"
        actions={
          <Button variant="primary" disabled={!drafts.length || submit.isPending} onClick={() => submit.mutate()}>
            Submit token sheet
          </Button>
        }
      >
        {drafts.length ? (
          <DataTable stack columns={columns} rows={drafts} rowKey={(e) => e.id} csvName="token-sheet.csv" onRowClick={setOpen} caption="Draft token entries" />
        ) : (
          <Empty title="Nothing to review">New sessions show up here as draft token entries from the Claude Code mod, the CLI or manual entry.</Empty>
        )}
        {submit.error ? <ErrorBox error={submit.error} /> : null}
      </Section>

      <EntryDialog entry={open} onClose={() => setOpen(null)} />
      <Dialog open={Boolean(result)} onOpenChange={() => setResult(null)} title="Token sheet submitted">
        {result ? (
          <p className="font-mono text-sm" data-testid="submit-result">
            submitted token sheet · {result.entries} token {result.entries === 1 ? "entry" : "entries"} · {formatMoney(result.total)} · {formatShare(result.on_tickets)} on tickets
          </p>
        ) : null}
        <p className="mt-3 text-sm text-muted">Submitted token entries count in reports at once.</p>
        <div className="mt-5 flex justify-end">
          <Button variant="primary" onClick={() => setResult(null)}>
            Done
          </Button>
        </div>
      </Dialog>
    </>
  );
}
