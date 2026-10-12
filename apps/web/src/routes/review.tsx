import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { CadencePeriod, EntryView, SheetSummary, Submitted, TokenSheet } from "@/api/contract";
import { api } from "@/api/endpoints";
import { cadenceKindText } from "@/components/cadence-select";
import { EntryDialog, entryValue, namedTags, unknownTickets, valueLabel } from "@/components/entry-dialog";
import { Badge, StatusBadge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { Empty, ErrorBox, Kpi, KpiRow, Loading, MoneyText, Note, PageHeader, Section } from "@/components/ui/display";
import { Select } from "@/components/ui/form";
import { DataTable, type Column } from "@/components/ui/table";
import { TagRow } from "@/components/ui/tag";
import { formatDate, formatDateTime, formatDay, formatPeriod, formatTime } from "@/lib/format";
import { formatMoney, formatShare, fromDecimal, toDecimal } from "@/lib/money";
import { refreshAll, useProjects, useTickets } from "@/lib/queries";

/** Sets the ticket of a token entry with one split, right in the review list. */
function TicketPicker({ entry }: { entry: EntryView }) {
  const tickets = useTickets();
  const projects = useProjects();
  const tags = entry.splits[0]?.tags ?? {};
  const save = useMutation({
    mutationFn: (ticket: string) => {
      // The ticket brings its project and what they pass on; the other tags stay.
      const rest = Object.fromEntries(Object.entries(namedTags(tags, projects.data)).filter(([k]) => k !== "ticket" && k !== "project"));
      return api.setTags(entry.id, ticket ? { ...rest, ticket } : rest);
    },
    onSuccess: refreshAll,
  });
  if (entry.splits.length > 1) return <span className="text-xs text-muted">{entry.splits.length} splits</span>;
  return (
    <span className="block" onClick={(e) => e.stopPropagation()}>
      <Select
        aria-label={`Ticket for “${entry.summary}”`}
        className="h-8 min-w-36 text-[13px]"
        value={tags.ticket ?? ""}
        disabled={save.isPending}
        onChange={(e) => save.mutate(e.target.value)}
      >
        <option value="">No ticket</option>
        {(tickets.data ?? []).map((t) => (
          <option key={t.key} value={t.key}>
            {t.key}
          </option>
        ))}
      </Select>
      {save.error ? <span className="mt-1 block text-xs text-exception">{save.error.message}</span> : null}
    </span>
  );
}

/** What placed a token entry, and an unknown ticket where there is one. */
function PlacedBy({ entry }: { entry: EntryView }) {
  const placed = [...new Set(entry.splits.map((s) => s.placed_by))].join(", ");
  return (
    <span className="flex flex-wrap items-center gap-1 text-[13px] text-muted">
      {placed}
      {unknownTickets(entry).map((key) => (
        <Badge key={key} tone="exception">
          unknown ticket {key}
        </Badge>
      ))}
    </span>
  );
}

const span = (p: CadencePeriod) => `${formatDay(p.starts_on)} to ${formatDay(p.ends_on)}`;

function andList(items: string[]): string {
  return items.length > 1 ? `${items.slice(0, -1).join(", ")} and ${items.at(-1) ?? ""}` : (items[0] ?? "");
}

/** The cadence period running now, when it is due, or that there is no cadence. */
function CadenceKpi({ sheet }: { sheet: TokenSheet }) {
  if (!sheet.period_start || !sheet.period_end) return <Kpi label="Cadence period" value="none" note="No cadence: submit whenever you like." />;
  const kind = sheet.cadence && sheet.cadence in cadenceKindText ? cadenceKindText[sheet.cadence as keyof typeof cadenceKindText] : "Cadence";
  return (
    <Kpi
      label="Cadence period"
      value={<span className="text-base">{span({ starts_on: sheet.period_start, ends_on: sheet.period_end })}</span>}
      note={`${kind} · due by the end of ${formatDay(sheet.period_end)}`}
    />
  );
}

function PastSheets({ sheets }: { sheets: SheetSummary[] }) {
  const [all, setAll] = useState(false);
  const shown = all ? sheets : sheets.slice(0, 6);
  const columns: Column<SheetSummary>[] = [
    { key: "period", header: "Cadence period", cell: (s) => span({ starts_on: s.period_start, ends_on: s.period_end }), csv: (s) => `${s.period_start} to ${s.period_end}` },
    { key: "entries", header: "Token entries", align: "right", cell: (s) => (s.token_entries ? s.token_entries : <span className="text-muted">nothing to report</span>), csv: (s) => s.token_entries },
    { key: "submitted", header: "Last submitted", cell: (s) => <span className="num !text-left">{formatDate(s.last_submitted_at)} {formatTime(s.last_submitted_at)}</span>, csv: (s) => s.last_submitted_at },
  ];
  return (
    <Section
      title="Your token sheets"
      lead="One per cadence period, newest first."
      actions={
        sheets.length > shown.length || all ? (
          <Button size="sm" variant="ghost" onClick={() => setAll(!all)}>
            {all ? "Show fewer" : `Show all ${String(sheets.length)}`}
          </Button>
        ) : undefined
      }
    >
      <DataTable stack columns={columns} rows={shown} rowKey={(s) => s.period_start} caption="Your token sheets" empty="You have not submitted a token sheet yet." />
    </Section>
  );
}

function SubmitResult({ result }: { result: Submitted }) {
  const list = formatMoney(fromDecimal(result.list_value, result.currency));
  const share = result.share_on_tickets_percent ?? null;
  return (
    <>
      <p className="font-mono text-sm" data-testid="submit-result">
        submitted token sheet · {result.submitted} token {result.submitted === 1 ? "entry" : "entries"} · {list}
        {share === null ? "" : ` · ${String(share)}% on tickets`}
      </p>
      {result.token_sheets.length ? <p className="mt-3 text-sm">For {andList(result.token_sheets.map(span))}.</p> : null}
      {result.late ? (
        <p className="mt-3 text-sm">
          {result.late} token {result.late === 1 ? "entry was" : "entries were"} in a closed month: posted to that month as a late adjustment, and locked.
        </p>
      ) : null}
      <p className="mt-3 text-sm text-muted">
        {result.submitted ? "Submitted token entries count in reports at once." : "Nothing to report: your token sheet is not late."}
      </p>
    </>
  );
}

export function ReviewPage(): ReactNode {
  const sheet = useQuery({ queryKey: ["token-sheet"], queryFn: api.tokenSheet });
  const [open, setOpen] = useState<EntryView | null>(null);
  const [result, setResult] = useState<Submitted | null>(null);
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
  const drafts = data.token_entries.filter((e) => e.status === "draft");
  const rejected = data.token_entries.filter((e) => e.status === "rejected");
  const unknown = data.token_entries.filter((e) => e.status !== "draft" && e.status !== "rejected");
  const unknownModels = drafts.filter((e) => !e.list_value).length;
  const share = data.share_on_tickets ?? null;
  const asked = data.asked_to_submit ?? null;
  const columns: Column<EntryView>[] = [
    { key: "n", header: "#", cell: (e) => <span className="num text-muted">{drafts.indexOf(e) + 1}</span>, csv: (e) => drafts.indexOf(e) + 1 },
    { key: "when", header: "When", cell: (e) => <span className="num !text-left">{formatDay(e.started_at)} {formatTime(e.started_at)}</span>, csv: (e) => e.started_at },
    {
      key: "cost",
      header: "List value",
      align: "right",
      cell: (e) => <MoneyText value={entryValue(e)} label={valueLabel(e)} />,
      csv: (e) => {
        const value = entryValue(e);
        return value ? toDecimal(value) : "unknown model";
      },
    },
    { key: "summary", header: "Summary", cell: (e) => <span className="line-clamp-2 text-ink-soft">{e.summary}</span>, csv: (e) => e.summary },
    { key: "ticket", header: "Ticket", cell: (e) => <TicketPicker entry={e} />, csv: (e) => e.splits.map((s) => s.tags.ticket ?? "").join(" | ") },
    { key: "tags", header: "Tags", wide: true, className: "min-w-52", cell: (e) => <TagRow tags={e.splits[0]?.tags ?? {}} hide={["person", "ticket"]} />, csv: (e) => e.splits.map((s) => Object.entries(s.tags).filter(([k]) => k !== "person").map(([k, v]) => `${k}: ${v}`).join(" · ")).join(" | ") },
    { key: "by", header: "By", wide: true, cell: (e) => <PlacedBy entry={e} />, csv: (e) => e.splits.map((s) => (s.unknown_ticket ? `${s.placed_by}, unknown ticket ${s.unknown_ticket}` : s.placed_by)).join(" | ") },
  ];
  return (
    <>
      <PageHeader
        title="Review and submit"
        lead="Check your draft token entries, fix the exceptions, then submit your token sheet. Project, client and team follow from the ticket."
      />
      {asked ? (
        <Note>
          {asked.asked_by ?? "A controller"} asked everyone to submit their token sheets for {formatPeriod(asked.period)} before the close.{" "}
          <span className="text-muted">Asked {formatDateTime(asked.asked_at)}.</span>
        </Note>
      ) : null}
      {data.late ? (
        <Note tone="exception">
          Your token sheet is late: a cadence period is over and not submitted. Nothing is blocked; submit when you can.
        </Note>
      ) : null}
      <KpiRow>
        <Kpi label="Draft token entries" value={drafts.length} />
        <Kpi
          label="List value"
          value={formatMoney(fromDecimal(data.list_value, data.currency))}
          note={unknownModels ? `list-price estimate, without ${String(unknownModels)} unknown ${unknownModels === 1 ? "model" : "models"}` : "list-price estimate"}
        />
        <Kpi label="On tickets" value={share === null ? "–" : formatShare(share)} tone={share !== null && share >= 9000 ? "posted" : undefined} note="of the drafts' list value" />
        <CadenceKpi sheet={data} />
      </KpiRow>

      {rejected.length ? (
        <Section title="Sent back to you" lead="A people manager rejected these with a reason. Fix the tags and they go back on your token sheet as drafts.">
          <ul className="space-y-2">
            {rejected.map((e) => (
              <li key={e.id}>
                <button type="button" onClick={() => setOpen(e)} className="flex w-full flex-col gap-1 rounded-md border border-exception/30 bg-exception-tint px-3 py-2.5 text-left text-sm sm:flex-row sm:items-center sm:justify-between">
                  <span>
                    <StatusBadge status="rejected" /> <span className="ml-1 font-medium">{e.summary}</span>
                    <span className="block text-[13px] text-ink-soft">{e.rejected_reason}</span>
                  </span>
                  <MoneyText value={entryValue(e)} label={valueLabel(e)} />
                </button>
              </li>
            ))}
          </ul>
        </Section>
      ) : null}

      {unknown.length ? (
        <Section title="Unknown ticket" lead="The ticket key matched a pattern, but the ticket sync did not find the ticket. Rules and the summary match ran again without it. Pick the right ticket.">
          <ul className="space-y-2">
            {unknown.map((e) => (
              <li key={e.id} className="flex flex-col gap-2 rounded-md border border-line px-3 py-2.5 text-sm sm:flex-row sm:items-center sm:justify-between">
                <button type="button" onClick={() => setOpen(e)} className="min-w-0 text-left">
                  {unknownTickets(e).map((key) => (
                    <Badge key={key} tone="exception">
                      unknown ticket {key}
                    </Badge>
                  ))}{" "}
                  <span className="ml-1 font-medium">{e.summary}</span>
                  <span className="block text-[13px] text-muted">
                    {formatDay(e.started_at)} · {e.status}
                  </span>
                </button>
                <span className="flex items-center justify-between gap-3">
                  {e.status === "approved" ? <span className="text-xs text-muted">approved: a people manager can send it back to you</span> : <TicketPicker entry={e} />}
                  <MoneyText value={entryValue(e)} label={valueLabel(e)} />
                </span>
              </li>
            ))}
          </ul>
        </Section>
      ) : null}

      <Section
        title="Draft token entries"
        actions={
          <Button variant="primary" disabled={submit.isPending} onClick={() => submit.mutate()}>
            Submit token sheet
          </Button>
        }
      >
        {drafts.length ? (
          <DataTable stack columns={columns} rows={drafts} rowKey={(e) => e.id} csvName="token-sheet.csv" onRowClick={setOpen} caption="Draft token entries" />
        ) : (
          <Empty title="Nothing to review">
            New sessions show up here as draft token entries from the Claude Code mod, the CLI or manual entry. With nothing to report, submit anyway so your token sheet is not late.
          </Empty>
        )}
        {submit.error ? <ErrorBox error={submit.error} /> : null}
      </Section>

      <PastSheets sheets={data.token_sheets} />

      <EntryDialog entry={open} onClose={() => setOpen(null)} />
      <Dialog open={Boolean(result)} onOpenChange={() => setResult(null)} title="Token sheet submitted">
        {result ? <SubmitResult result={result} /> : null}
        <div className="mt-5 flex justify-end">
          <Button variant="primary" onClick={() => setResult(null)}>
            Done
          </Button>
        </div>
      </Dialog>
    </>
  );
}
