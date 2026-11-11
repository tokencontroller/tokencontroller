import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { PriceRow } from "@/api/contract";
import { api } from "@/api/endpoints";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { Empty, ErrorBox, Loading, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select } from "@/components/ui/form";
import { DataTable } from "@/components/ui/table";
import { notify } from "@/components/ui/toast";
import { formatDateTime, formatTokens } from "@/lib/format";
import { refreshAll, usePersonName } from "@/lib/queries";
import { today } from "@/lib/today";

/** The four kinds every model has; a provider may send others, priced the same way. */
const kinds = ["input", "output", "cache_read", "cache_write"];
const kindTitle: Record<string, string> = { input: "Input", output: "Output", cache_read: "Cache read", cache_write: "Cache write" };

/** The rows of one model with one source, contract and valid-from day, by token kind. */
interface ModelRow {
  id: string;
  provider: string;
  model: string;
  contract: string | null;
  valid_from: string;
  source: string;
  rows: PriceRow[];
}

function byModel(rows: PriceRow[]): ModelRow[] {
  const out = new Map<string, ModelRow>();
  for (const r of rows) {
    const id = [r.provider, r.model, r.contract ?? "", r.valid_from, r.source].join("|");
    const group = out.get(id) ?? { id, provider: r.provider, model: r.model, contract: r.contract ?? null, valid_from: r.valid_from, source: r.source, rows: [] };
    group.rows.push(r);
    out.set(id, group);
  }
  return [...out.values()].sort((a, b) => a.provider.localeCompare(b.provider) || a.model.localeCompare(b.model) || a.valid_from.localeCompare(b.valid_from));
}

const priceOf = (g: ModelRow, kind: string) => g.rows.find((r) => r.kind === kind)?.price_per_million;

// Docs: Price table. The only thing that turns token counts into cost. The
// managed rows are read-only; your own rows and overrides win from their
// valid-from day, and a row for a contract wins for that contract's
// messages. Messages without counts and models without a price come first,
// never zero.
export function PriceTablePage(): ReactNode {
  const table = useQuery({ queryKey: ["price-table"], queryFn: api.priceTable });
  const messages = useQuery({ queryKey: ["messages", "all"], queryFn: () => api.messages() });
  const personName = usePersonName();
  const [adding, setAdding] = useState(false);
  const remove = useMutation({
    mutationFn: async (group: ModelRow) => {
      for (const row of group.rows) await api.reverse(row.id);
    },
    onSuccess: async () => {
      await refreshAll();
      notify("Row reversed");
    },
  });
  const missing = messages.data?.messages.filter((m) => m.cost_label !== "priced") ?? [];
  const groups = byModel(table.data?.rows ?? []);
  const others = [...new Set((table.data?.rows ?? []).map((r) => r.kind).filter((k) => !kinds.includes(k)))];
  return (
    <>
      <PageHeader title="Price table" lead="Per million tokens, US dollars." actions={<Button variant="primary" onClick={() => setAdding(true)}>Add a row</Button>} />
      <AddRowDialog open={adding} onOpenChange={setAdding} />
      {missing.length ? (
        <Section title={`${String(missing.length)} messages without a cost`} flush hint="Count errors and models not priced yet have no cost, never zero. A bill's check shows them until the counts or the row arrive.">
          <DataTable
            caption="Messages without a cost"
            stack
            rows={missing}
            rowKey={(m) => m.message_id}
            columns={[
              { key: "at", header: "When", cell: (m) => formatDateTime(m.at) },
              { key: "person", header: "Person", wide: true, cell: (m) => personName(m.person) },
              { key: "model", header: "Model", cell: (m) => <span className="font-mono text-caption">{m.provider}/{m.model}</span> },
              {
                key: "tokens",
                header: "Tokens",
                align: "right",
                cell: (m) => {
                  const counts = m.tokens as Record<string, number> | null | undefined;
                  return counts ? <span className="num">{formatTokens(Object.values(counts).reduce((a, b) => a + b, 0))}</span> : "none";
                },
              },
              { key: "why", header: "", cell: (m) => <Badge tone="exception">{m.cost_label === "count_error" ? "count error" : "not priced yet"}</Badge> },
            ]}
          />
        </Section>
      ) : messages.data ? (
        <Section title="Messages without a cost">
          <Empty title="Every message has a cost." />
        </Section>
      ) : null}
      <Section title="Rows in force" flush>
        {table.error ? <ErrorBox error={table.error} /> : null}
        {!table.data ? (
          <Loading />
        ) : (
          <DataTable
            caption="Price table"
            stack
            csvName="price-table.csv"
            rows={groups}
            rowKey={(g) => g.id}
            columns={[
              {
                key: "model",
                header: "Model",
                cell: (g) => (
                  <span className="flex flex-wrap items-center gap-1.5">
                    <span className="font-mono text-[13px]">{g.provider}/{g.model}</span>
                    {g.contract ? <Badge tone="accent">{g.contract}</Badge> : null}
                    {g.source === "managed" ? null : <Badge tone="accent">your row</Badge>}
                  </span>
                ),
                csv: (g) => `${g.provider}/${g.model}`,
              },
              ...[...kinds, ...others].map((kind) => ({
                key: kind,
                header: kindTitle[kind] ?? kind,
                align: "right" as const,
                cell: (g: ModelRow) => <span className="num">{priceOf(g, kind) ?? <span className="text-muted">·</span>}</span>,
                csv: (g: ModelRow) => priceOf(g, kind) ?? null,
              })),
              { key: "from", header: "Valid from", wide: true, cell: (g) => g.valid_from, csv: (g) => g.valid_from },
              { key: "contract", header: "Contract", csvOnly: true, cell: (g) => g.contract, csv: (g) => g.contract },
              {
                key: "act",
                header: "",
                align: "right",
                cell: (g) =>
                  g.source === "organization" ? (
                    <Button size="sm" variant="danger" onClick={() => remove.mutate(g)}>
                      Reverse
                    </Button>
                  ) : null,
              },
            ]}
          />
        )}
        <FormError error={remove.error} />
      </Section>
    </>
  );
}

function AddRowDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const contracts = useQuery({ queryKey: ["contracts"], queryFn: api.contracts });
  const [provider, setProvider] = useState("anthropic");
  const [model, setModel] = useState("");
  const [kind, setKind] = useState("input");
  const [price, setPrice] = useState("");
  const [from, setFrom] = useState(today());
  const [contract, setContract] = useState("");
  const own = (contracts.data ?? []).filter((c) => c.provider === provider.trim());
  const add = useMutation({
    mutationFn: () => api.addPriceRow({ provider, model, kind: kind.trim(), price_per_million: price, valid_from: from, ...(contract && own.some((c) => c.key === contract) ? { contract } : {}) }),
    onSuccess: async () => {
      await refreshAll();
      notify("Row added", `${model} ${kind}`);
      setPrice("");
      onOpenChange(false);
    },
  });
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title="Add a row" description="The same provider, model, kind and day overrides the managed row." wide>
      <form
        className="grid gap-3 sm:grid-cols-2"
        onSubmit={(e) => {
          e.preventDefault();
          add.mutate();
        }}
      >
        <Field label="Provider">
          <Input value={provider} onChange={(e) => setProvider(e.target.value)} />
        </Field>
        <Field label="Model">
          <Input value={model} onChange={(e) => setModel(e.target.value)} />
        </Field>
        <Field label="Token kind">
          <Input list="price-kinds" value={kind} onChange={(e) => setKind(e.target.value)} />
          <datalist id="price-kinds">{kinds.map((k) => <option key={k} value={k} />)}</datalist>
        </Field>
        <Field label="Price per million">
          <Input inputMode="decimal" value={price} onChange={(e) => setPrice(e.target.value)} />
        </Field>
        <Field label="Valid from">
          <Input type="date" value={from} onChange={(e) => setFrom(e.target.value)} />
        </Field>
        <Field label="Contract" hint="Empty: every contract">
          <Select value={contract} onChange={(e) => setContract(e.target.value)}>
            <option value="">Every contract</option>
            {own.map((c) => (
              <option key={c.key} value={c.key}>{c.title ? `${c.title} (${c.key})` : c.key}</option>
            ))}
          </Select>
        </Field>
        <div className="sm:col-span-2">
          <FormError error={add.error} />
        </div>
        <div className="flex justify-end gap-2 sm:col-span-2">
          <Button onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button variant="primary" type="submit" disabled={!model || !price || !kind.trim() || add.isPending}>
            Add the row
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
