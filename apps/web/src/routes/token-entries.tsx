import { useMutation, useQuery } from "@tanstack/react-query";
import { getRouteApi } from "@tanstack/react-router";
import { useState, type ReactNode } from "react";
import type { NewTokenEntry, Tags, TokenEntry, TokenEntryStatus } from "@/api/contract";
import { api } from "@/api/endpoints";
import { EntryDialog, entryValue, TagEditor } from "@/components/entry-dialog";
import { PeriodSelect } from "@/components/period-select";
import { Badge, CostKindBadge, costKindLabel, StatusBadge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Loading, MoneyText, Note, PageHeader } from "@/components/ui/display";
import { Field, FormError, Input, Select, Textarea } from "@/components/ui/form";
import { DataTable, type Column } from "@/components/ui/table";
import { TagRow } from "@/components/ui/tag";
import { notify } from "@/components/ui/toast";
import { describeFilter, filterFromText, filterToText, formatDay, formatTime, formatTokens, totalTokens } from "@/lib/format";
import { formatMoney, parseAmount, sum, toDecimal } from "@/lib/money";
import { refreshAll, useMe, usePeople, usePersonName } from "@/lib/queries";
import { hasRole } from "@/lib/roles";
import { thisPeriod, today } from "@/lib/today";

const tokenEntriesRoute = getRouteApi("/app/token-entries");

const harnesses = ["Claude Code", "Codex", "Gemini CLI", "Copilot CLI", "Cursor", "Other"];

function AddEntry({ open, onClose }: { open: boolean; onClose: () => void }) {
  const me = useMe();
  const [form, setForm] = useState({ agent_harness: "Claude Code", agent_name: "", model: "", date: today(), start: "09:00", end: "10:00", cost: "", summary: "" });
  const [tags, setTags] = useState<Tags>({});
  const add = useMutation({
    mutationFn: () => {
      const cost = form.cost ? parseAmount(form.cost, me.organization.reporting_currency) : null;
      if (form.cost && !cost) throw new Error("The cost is not a number.");
      const body: NewTokenEntry = {
        agent_harness: form.agent_harness,
        agent_name: form.agent_name || null,
        model: form.model,
        started_at: `${form.date}T${form.start}:00Z`,
        ended_at: `${form.date}T${form.end}:00Z`,
        tokens: { input: 0, output: 0, cache_write: 0, cache_read: 0 },
        reported_cost: cost,
        summary: form.summary,
        tags,
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
        <Field label={`Cost in ${me.organization.reporting_currency}`} hint="Leave empty to compute the list value from tokens and the price table.">
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

const csvExample = "agent_harness,model,started_at,ended_at,input,output,cache_write,cache_read,reported_cost,summary,ticket,project\nClaude Code,claude-opus-5,2026-10-09T09:00:00Z,2026-10-09T10:10:00Z,1200,38000,210000,5900000,41.87,Fixed checkout loop,KD-214,\n";

function ImportCsv({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [text, setText] = useState("");
  const preview = useMutation({ mutationFn: () => api.previewImport(text) });
  const save = useMutation({
    mutationFn: () => api.importEntries(text),
    onSuccess: async (r) => {
      await refreshAll();
      notify(`${r.entries} token entries imported`, "They are on your token sheet as drafts.");
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
              Read {preview.data.rows.length} {preview.data.rows.length === 1 ? "row" : "rows"}
              {preview.data.problems.length ? `, ${preview.data.problems.length} with problems` : ", all fine"}.
            </p>
            <ul className="mt-2 space-y-1 text-[13px]">
              {preview.data.rows.slice(0, 5).map((row, i) => (
                <li key={i} className="truncate text-ink-soft">
                  {row.started_at.slice(0, 10)} · {row.agent_harness} · {row.model} · {row.reported_cost ? formatMoney(row.reported_cost) : "list value"} · {row.summary}
                </li>
              ))}
            </ul>
            {preview.data.problems.map((p) => (
              <p key={`${p.row}${p.message}`} className="mt-1 text-[13px] text-exception">
                Row {p.row}: {p.message}
              </p>
            ))}
          </div>
        ) : null}
        <FormError error={preview.error ?? save.error} />
        <div className="flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          {preview.data && !preview.data.problems.length ? (
            <Button variant="primary" onClick={() => save.mutate()} disabled={save.isPending}>
              Import {preview.data.rows.length} token entries
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

const statuses: (TokenEntryStatus | "")[] = ["", "draft", "submitted", "approved", "rejected", "locked"];

export function TokenEntriesPage(): ReactNode {
  const me = useMe();
  const search = tokenEntriesRoute.useSearch();
  const navigate = tokenEntriesRoute.useNavigate();
  const period = search.period ?? thisPeriod();
  const [status, setStatus] = useState<TokenEntryStatus | "">("");
  const [person, setPerson] = useState("");
  const [filterText, setFilterText] = useState("");
  const [open, setOpen] = useState<TokenEntry | null>(null);
  const [adding, setAdding] = useState(false);
  const [importing, setImporting] = useState(false);
  const filter = filterFromText(filterText);
  const people = usePeople();
  const name = usePersonName();
  const entries = useQuery({
    queryKey: ["token-entries", period, status, person, filterToText(filter)],
    queryFn: () => api.tokenEntries({ period, status, person, filter }),
  });
  const currency = me.organization.reporting_currency;
  const rows = entries.data ?? [];
  const orphans = rows.filter((e) => e.agent_manager === null);
  const columns: Column<TokenEntry>[] = [
    { key: "when", header: "When", className: "whitespace-nowrap", cell: (e) => <span className="num !text-left">{formatDay(e.started_at)} {formatTime(e.started_at)}</span>, csv: (e) => e.started_at },
    { key: "person", header: "Agent manager", className: "whitespace-nowrap", cell: (e) => (e.agent_manager ? name(e.agent_manager) : <Badge tone="exception">nobody</Badge>), csv: (e) => e.agent_manager ?? "" },
    {
      key: "agent",
      header: "Agent",
      wide: true,
      className: "whitespace-nowrap",
      cell: (e) => (
        <span className="block leading-tight">
          {e.agent_harness}
          {e.agent_name ? <span className="text-muted"> · {e.agent_name}</span> : null}
          <span className="mt-0.5 block font-mono text-[11px] text-muted">{e.model}</span>
        </span>
      ),
      csv: (e) => e.agent_harness,
    },
    { key: "agent_name", header: "Agent name", csvOnly: true, cell: () => null, csv: (e) => e.agent_name ?? "" },
    { key: "model", header: "Model", csvOnly: true, cell: () => null, csv: (e) => e.model },
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
    { key: "tags", header: "Tags", csvOnly: true, cell: () => null, csv: (e) => e.splits.map((s) => Object.entries(s.tags).map(([k, v]) => `${k}: ${v}`).join(" · ")).join(" | ") },
    { key: "kind", header: "Cost kind", wide: true, cell: (e) => (e.splits.length > 1 ? <Badge>split</Badge> : e.splits[0] ? <CostKindBadge kind={e.splits[0].cost_kind} /> : null), csv: (e) => e.splits.map((s) => costKindLabel[s.cost_kind]).join(" | ") },
    { key: "tokens", header: "Tokens", align: "right", wide: "xl", cell: (e) => formatTokens(totalTokens(e.tokens)), csv: (e) => totalTokens(e.tokens) },
    { key: "cost", header: "Cost", align: "right", cell: (e) => <MoneyText value={entryValue(e)} label={e.value_label} />, csv: (e) => { const v = entryValue(e); return v ? toDecimal(v) : "unknown model"; } },
    { key: "status", header: "Status", cell: (e) => <StatusBadge status={e.status} />, csv: (e) => e.status },
  ];
  const total = sum(rows.map(entryValue).filter((v) => v !== null), currency);
  return (
    <>
      <PageHeader
        title="Token entries"
        lead={hasRole(me.person, "controller") ? "Every token entry in the organization." : hasRole(me.person, "people_manager") ? "Your own token entries and those within your filters." : "Your own token entries."}
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
          {orphans.length} token {orphans.length === 1 ? "entry has" : "entries have"} no agent manager. Open {orphans.length === 1 ? "it" : "one"} to take it over.
        </Note>
      ) : null}
      <div className="mb-4 grid gap-2 sm:flex sm:flex-wrap sm:items-end">
        <PeriodSelect value={period} onChange={(p) => void navigate({ search: { period: p } })} />
        <Select aria-label="Status" value={status} onChange={(e) => setStatus(e.target.value as TokenEntryStatus | "")} className="sm:w-40">
          {statuses.map((s) => (
            <option key={s} value={s}>
              {s ? `${s.charAt(0).toUpperCase()}${s.slice(1)}` : "Any status"}
            </option>
          ))}
        </Select>
        {hasRole(me.person, "people_manager") ? (
          <Select aria-label="Agent manager" value={person} onChange={(e) => setPerson(e.target.value)} className="sm:w-48">
            <option value="">Everyone</option>
            {(people.data ?? []).map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </Select>
        ) : null}
        <Input aria-label="Tag filter" value={filterText} onChange={(e) => setFilterText(e.target.value)} placeholder="Filter, such as client:acme,team:web" className="sm:w-72" />
      </div>
      {Object.keys(filter).length ? <p className="mb-3 text-[13px] text-muted">Showing {describeFilter(filter)}.</p> : null}
      {entries.isPending ? <Loading /> : entries.isError ? <ErrorBox error={entries.error} /> : (
        <DataTable stack
          columns={columns}
          rows={rows}
          rowKey={(e) => e.id}
          csvName={`token-entries-${period}.csv`}
          onRowClick={setOpen}
          rowClassName={(e) => (e.agent_manager === null ? "bg-exception-tint" : "")}
          caption="Token entries"
          empty="No token entries match."
          totals={{ label: `${rows.length} token entries`, values: { cost: formatMoney(total) }, csv: { cost: toDecimal(total) } }}
        />
      )}
      <EntryDialog entry={open} onClose={() => setOpen(null)} />
      <AddEntry open={adding} onClose={() => setAdding(false)} />
      <ImportCsv open={importing} onClose={() => setImporting(false)} />
    </>
  );
}
