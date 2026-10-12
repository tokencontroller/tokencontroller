import { keepPreviousData, useMutation, useQuery } from "@tanstack/react-query";
import { getRouteApi } from "@tanstack/react-router";
import { useState, type ReactNode } from "react";
import type { EntryView, NewTokenEntry, Tags, TokenEntryStatus } from "@/api/contract";
import { api } from "@/api/endpoints";
import { EntryDialog, entryValue, listTotal, TagEditor, unknownTickets, valueLabel } from "@/components/entry-dialog";
import { PeriodSelect } from "@/components/period-select";
import { Badge, StatusBadge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Loading, MoneyText, Note, PageHeader } from "@/components/ui/display";
import { Field, FormError, Input, Select, Textarea } from "@/components/ui/form";
import { DataTable, type Column } from "@/components/ui/table";
import { TagRow } from "@/components/ui/tag";
import { notify } from "@/components/ui/toast";
import { describeFilter, displayName, filterFromText, filterToText, formatDay, formatTime } from "@/lib/format";
import { formatMoney, toDecimal } from "@/lib/money";
import { refreshAll, useMe, usePeople } from "@/lib/queries";
import { hasRole } from "@/lib/roles";
import { thisPeriod, today } from "@/lib/today";

const tokenEntriesRoute = getRouteApi("/app/token-entries");

const harnesses = ["Claude Code", "Codex", "Gemini CLI", "Copilot CLI", "Cursor", "Other"];

/** A decimal string such as `12.5`, or null when the text is not one. */
function decimal(text: string): string | null {
  const clean = text.replace(/[,\s]/g, "");
  return /^\d+(\.\d{1,9})?$/.test(clean) ? clean : null;
}

const tokenFields = [
  ["input", "Input"],
  ["output", "Output"],
  ["cache_write", "Cache write"],
  ["cache_read", "Cache read"],
] as const;

function AddEntry({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [form, setForm] = useState({ agent_harness: "Claude Code", agent_name: "", model: "", date: today(), start: "09:00", end: "10:00", cost: "", summary: "", input: "", output: "", cache_write: "", cache_read: "" });
  const [tags, setTags] = useState<Tags>({});
  const add = useMutation({
    mutationFn: () => {
      const cost = form.cost ? decimal(form.cost) : null;
      if (form.cost && !cost) throw new Error("The cost is not a number.");
      const count = (text: string) => Number(text.replace(/[,\s]/g, "")) || 0;
      const tokens = { input: count(form.input), output: count(form.output), cache_write: count(form.cache_write), cache_read: count(form.cache_read) };
      if (!cost && !Object.values(tokens).some(Boolean)) throw new Error("Give the tokens or the cost.");
      const { ticket, project, ...rest } = tags;
      const body: NewTokenEntry = {
        agent_harness: form.agent_harness,
        agent_name: form.agent_name || null,
        model: form.model,
        started_at: `${form.date}T${form.start}:00Z`,
        ended_at: `${form.date}T${form.end}:00Z`,
        tokens,
        reported_cost: cost,
        summary: form.summary,
        ticket: ticket ?? null,
        project: project ?? null,
        tags: rest,
        // Where a person picked the ticket or project, no rule overrides it.
        placed_by: ticket || project ? "hand" : null,
      };
      return api.addTokenEntry(body);
    },
    onSuccess: async () => {
      await refreshAll();
      notify("Token entry added", "It is on your token sheet as a draft.");
      onClose();
    },
  });
  const set = (key: keyof typeof form, value: string) => setForm((f) => ({ ...f, [key]: value }));
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()} title="Add a token entry" description="For usage no agent or local tool can report. It goes on your token sheet like any other." wide>
      <form
        className="space-y-4"
        onSubmit={(e) => {
          e.preventDefault();
          add.mutate();
        }}
      >
        <div className="grid gap-3 sm:grid-cols-3">
          <Field label="Agent harness">
            <Select value={form.agent_harness} onChange={(e) => set("agent_harness", e.target.value)}>
              {harnesses.map((h) => (
                <option key={h}>{h}</option>
              ))}
            </Select>
          </Field>
          <Field label="Model">
            <Input required value={form.model} onChange={(e) => set("model", e.target.value)} placeholder="claude-opus-5" />
          </Field>
          <Field label="Agent name" hint="Optional, such as review bot.">
            <Input value={form.agent_name} onChange={(e) => set("agent_name", e.target.value)} />
          </Field>
          <Field label="Date">
            <Input type="date" required value={form.date} onChange={(e) => set("date", e.target.value)} />
          </Field>
          <Field label="Start (UTC)">
            <Input type="time" required value={form.start} onChange={(e) => set("start", e.target.value)} />
          </Field>
          <Field label="End (UTC)">
            <Input type="time" required value={form.end} onChange={(e) => set("end", e.target.value)} />
          </Field>
        </div>
        <fieldset>
          <legend className="mb-1 text-[13px] font-medium text-ink-soft">Tokens</legend>
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
            {tokenFields.map(([key, label]) => (
              <Field key={key} label={label}>
                <Input inputMode="numeric" value={form[key]} onChange={(e) => set(key, e.target.value)} placeholder="0" />
              </Field>
            ))}
          </div>
        </fieldset>
        <Field label="Reported cost in US dollars" hint="The agent's own figure, if it shows one. Leave empty to compute the list value from the tokens and the price table.">
          <Input inputMode="decimal" value={form.cost} onChange={(e) => set("cost", e.target.value)} placeholder="12.50" />
        </Field>
        <Field label="Summary" hint="What the session did. With the tags, the only text in a token entry.">
          <Textarea required value={form.summary} onChange={(e) => set("summary", e.target.value)} />
        </Field>
        <TagEditor tags={tags} onChange={setTags} />
        <FormError error={add.error} />
        <div className="flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={add.isPending}>
            Add token entry
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

// As docs Manual entry and CSV import shows it.
const csvExample = "agent_harness,model,started_at,ended_at,tokens.input,tokens.output,reported_cost,summary,ticket,tags.team\nclaude-code,claude-sonnet-5,2026-09-18T09:12:00Z,2026-09-18T09:40:00Z,41200,6100,0.42,Fixed the cart total,KD-214,web\n";

function ImportCsv({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [text, setText] = useState("");
  const preview = useMutation({ mutationFn: () => api.previewImport(text) });
  const save = useMutation({
    mutationFn: () => api.importEntries(text),
    onSuccess: async (r) => {
      await refreshAll();
      const dropped = [...new Set(r.token_entries.flatMap((e) => e.dropped_tags))];
      const count = r.token_entries.length;
      notify(`${String(count)} ${count === 1 ? "token entry" : "token entries"} imported`, dropped.length ? `They are on your token sheet as drafts. Left out tags not on your list: ${dropped.join(", ")}.` : "They are on your token sheet as drafts.");
      setText("");
      preview.reset();
      onClose();
    },
  });
  const readFile = async (file: File | undefined) => {
    if (!file) return;
    setText(await file.text());
    preview.reset();
  };
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()} title="Import CSV" description="One token entry per row. The columns are the fields of the token entry schema; tc export --csv writes the same format." wide>
      <div className="space-y-4">
        <Field label="CSV file">
          <Input type="file" accept=".csv,text/csv" onChange={(e) => void readFile(e.target.files?.[0])} className="pt-1.5" />
        </Field>
        <Field label="Or paste it">
          <Textarea aria-label="CSV text" value={text} onChange={(e) => { setText(e.target.value); preview.reset(); }} placeholder={csvExample} className="font-mono text-xs" rows={5} />
        </Field>
        {preview.data ? (
          <div className="rounded-md border border-line p-3 text-sm">
            <p className="font-medium">
              {preview.data.ready} {preview.data.ready === 1 ? "row is" : "rows are"} ready
              {preview.data.problems.length ? `, ${String(preview.data.problems.length)} cannot be read. Fix them and check the file again; nothing is saved until every row can be read.` : "."}
            </p>
            {preview.data.problems.map((p) => (
              <p key={`${String(p.line)}${p.problem}`} className="mt-1 text-[13px] text-exception">
                Line {p.line}: {p.problem}
              </p>
            ))}
          </div>
        ) : null}
        <FormError error={preview.error ?? save.error} />
        <div className="flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          {preview.data && preview.data.ready && !preview.data.problems.length ? (
            <Button variant="primary" onClick={() => save.mutate()} disabled={save.isPending}>
              Import {preview.data.ready} {preview.data.ready === 1 ? "token entry" : "token entries"}
            </Button>
          ) : (
            <Button variant="primary" onClick={() => preview.mutate()} disabled={!text.trim() || preview.isPending}>
              Check the file
            </Button>
          )}
        </div>
      </div>
    </Dialog>
  );
}

/** The token entries table shows the statuses that count unless one is picked. Drafts stay on the token sheet. */
const statuses: { value: TokenEntryStatus | ""; label: string }[] = [
  { value: "", label: "All that count" },
  { value: "submitted", label: "Submitted" },
  { value: "approved", label: "Approved" },
  { value: "rejected", label: "Rejected" },
  { value: "locked", label: "Locked" },
];

/** Token entries per page of the table. */
const PAGE = 200;

/** What placed a token entry's splits, and an unknown ticket where there is one. */
function PlacedBy({ entry }: { entry: EntryView }) {
  return (
    <span className="mt-1.5 flex flex-wrap items-center gap-1 text-xs text-muted max-sm:justify-end">
      {[...new Set(entry.splits.map((s) => s.placed_by))].join(", ")}
      {unknownTickets(entry).map((key) => (
        <Badge key={key} tone="exception">
          unknown ticket {key}
        </Badge>
      ))}
    </span>
  );
}

const tagText = (entry: EntryView) =>
  entry.splits
    .map((s) =>
      Object.entries(s.tags)
        .filter(([k]) => k !== "person")
        .map(([k, v]) => `${k}: ${v}`)
        .join(" · "),
    )
    .join(" | ");

export function TokenEntriesPage(): ReactNode {
  const me = useMe();
  const search = tokenEntriesRoute.useSearch();
  const navigate = tokenEntriesRoute.useNavigate();
  const period = search.period ?? thisPeriod();
  const [status, setStatus] = useState<TokenEntryStatus | "">("");
  // The person tag holds the agent manager's email.
  const [person, setPerson] = useState("");
  const [filterText, setFilterText] = useState("");
  const [limit, setLimit] = useState(PAGE);
  const [open, setOpen] = useState<EntryView | null>(null);
  const [adding, setAdding] = useState(false);
  const [importing, setImporting] = useState(false);
  const filter = filterFromText(filterText);
  const query = { ...filter, ...(person ? { person } : {}) };
  const people = usePeople();
  const entries = useQuery({
    queryKey: ["token-entries", period, status, filterToText(query), limit],
    queryFn: () => api.tokenEntries({ period, filter: query, status: status ? [status] : [], limit }),
    placeholderData: keepPreviousData,
  });
  const rows = entries.data?.token_entries ?? [];
  const matching = entries.data?.total ?? 0;
  const orphans = rows.filter((e) => (e.agent_manager ?? null) === null);
  const columns: Column<EntryView>[] = [
    { key: "when", header: "When", className: "whitespace-nowrap", cell: (e) => <span className="num !text-left">{formatDay(e.started_at)} {formatTime(e.started_at)}</span>, csv: (e) => e.started_at },
    { key: "person", header: "Agent manager", className: "whitespace-nowrap", cell: (e) => e.agent_manager ?? <Badge tone="exception">nobody</Badge>, csv: (e) => e.agent_manager ?? "" },
    {
      key: "agent",
      header: "Agent",
      wide: true,
      className: "whitespace-nowrap",
      cell: (e) => (
        <span className="block leading-tight">
          {e.agent_harness}
          {e.agent_name ? <span className="text-muted"> · {e.agent_name}</span> : null}
          {e.automatic ? <span className="text-muted"> · automatic</span> : null}
          <span className="mt-0.5 block font-mono text-[11px] text-muted">{e.model}</span>
        </span>
      ),
      csv: (e) => e.agent_harness,
    },
    { key: "agent_name", header: "Agent name", csvOnly: true, cell: () => null, csv: (e) => e.agent_name ?? "" },
    { key: "model", header: "Model", csvOnly: true, cell: () => null, csv: (e) => e.model },
    { key: "provider", header: "Provider", csvOnly: true, cell: () => null, csv: (e) => e.provider },
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
          <PlacedBy entry={e} />
        </span>
      ),
      csv: (e) => e.summary,
    },
    { key: "tags", header: "Tags", csvOnly: true, cell: () => null, csv: tagText },
    { key: "by", header: "Placed by", csvOnly: true, cell: () => null, csv: (e) => e.splits.map((s) => (s.unknown_ticket ? `${s.placed_by}, unknown ticket ${s.unknown_ticket}` : s.placed_by)).join(" | ") },
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
    { key: "status", header: "Status", cell: (e) => <StatusBadge status={e.status} />, csv: (e) => e.status },
  ];
  const total = listTotal(rows, me.organization.reporting_currency);
  return (
    <>
      <PageHeader
        title="Token entries"
        lead={hasRole(me.person, "controller") ? "Every token entry in the organization." : hasRole(me.person, "people_manager") ? "Your own token entries and those within your filters." : "Your own token entries. Drafts are on your token sheet until you submit them."}
        actions={
          <>
            <Button onClick={() => setImporting(true)}>Import CSV</Button>
            <Button variant="primary" onClick={() => setAdding(true)}>
              Add
            </Button>
          </>
        }
      />
      {orphans.length && hasRole(me.person, "people_manager") ? (
        <Note tone="exception">
          {orphans.length} token {orphans.length === 1 ? "entry has" : "entries have"} no agent manager. Open {orphans.length === 1 ? "it" : "one"} to see how to hand it over.
        </Note>
      ) : null}
      <div className="mb-4 grid gap-2 sm:flex sm:flex-wrap sm:items-end">
        <PeriodSelect value={period} onChange={(p) => void navigate({ search: { period: p } })} />
        <Select aria-label="Status" value={status} onChange={(e) => setStatus(e.target.value as TokenEntryStatus | "")} className="sm:w-40">
          {statuses.map((s) => (
            <option key={s.value} value={s.value}>
              {s.label}
            </option>
          ))}
        </Select>
        {hasRole(me.person, "people_manager") ? (
          <Select aria-label="Agent manager" value={person} onChange={(e) => setPerson(e.target.value)} className="sm:w-48">
            <option value="">Everyone</option>
            {(people.data ?? [])
              .filter((p) => p.status !== "removed")
              .map((p) => (
                <option key={p.id} value={p.email}>
                  {displayName(p)}
                </option>
              ))}
          </Select>
        ) : null}
        <Input aria-label="Tag filter" value={filterText} onChange={(e) => setFilterText(e.target.value)} placeholder="Filter, such as client:acme,team:web" className="sm:w-72" />
      </div>
      {Object.keys(filter).length ? <p className="mb-3 text-[13px] text-muted">Showing {describeFilter(filter)}.</p> : null}
      {entries.isPending ? <Loading /> : entries.isError ? <ErrorBox error={entries.error} /> : (
        <>
          <DataTable stack
            columns={columns}
            rows={rows}
            rowKey={(e) => e.id}
            csvName={`token-entries-${period}.csv`}
            onRowClick={setOpen}
            rowClassName={(e) => ((e.agent_manager ?? null) === null ? "bg-exception-tint" : "")}
            caption="Token entries"
            empty="No token entries match."
            totals={{ label: `${String(rows.length)} token ${rows.length === 1 ? "entry" : "entries"}`, values: { cost: formatMoney(total) }, csv: { cost: toDecimal(total) } }}
          />
          {matching > rows.length ? (
            <div className="mt-3 flex flex-wrap items-center justify-between gap-2 text-[13px] text-muted">
              <span>
                Showing {rows.length} of {matching}. The CSV holds what is shown.
              </span>
              <Button size="sm" onClick={() => setLimit(limit + PAGE)} disabled={entries.isFetching}>
                Show more
              </Button>
            </div>
          ) : null}
        </>
      )}
      <EntryDialog entry={open} onClose={() => setOpen(null)} />
      <AddEntry open={adding} onClose={() => setAdding(false)} />
      <ImportCsv open={importing} onClose={() => setImporting(false)} />
    </>
  );
}
