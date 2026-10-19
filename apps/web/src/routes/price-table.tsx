import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { PriceRow } from "@/api/contract";
import { api } from "@/api/endpoints";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Empty, ErrorBox, Loading, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select } from "@/components/ui/form";
import { DataTable } from "@/components/ui/table";
import { notify } from "@/components/ui/toast";
import { formatDateTime, formatTokens } from "@/lib/format";
import { refreshAll, usePersonName } from "@/lib/queries";
import { today } from "@/lib/today";

const kinds = ["input", "output", "cache_read", "cache_write"];

// Docs: Price table. The only thing that turns token counts into cost. The
// managed rows are read-only; your own rows and overrides win from their
// valid-from day. Messages without counts and models without a price are
// listed, never zero.
export function PriceTablePage(): ReactNode {
  const table = useQuery({ queryKey: ["price-table"], queryFn: api.priceTable });
  const messages = useQuery({ queryKey: ["messages", "all"], queryFn: () => api.messages() });
  const personName = usePersonName();
  const remove = useMutation({
    mutationFn: (row: PriceRow) => api.reverse(row.id),
    onSuccess: async () => {
      await refreshAll();
      notify("Row reversed");
    },
  });
  const missing = messages.data?.messages.filter((m) => m.cost_label !== "priced") ?? [];
  return (
    <>
      <PageHeader title="Price table" lead="Prices per million tokens, in US dollars. A message's cost uses the row valid on its day." />
      <Section title="Rows in force">
        {table.error ? <ErrorBox error={table.error} /> : null}
        {!table.data ? (
          <Loading />
        ) : (
          <DataTable
            caption="Price table"
            stack
            csvName="price-table.csv"
            rows={table.data.rows}
            rowKey={(r) => r.id}
            columns={[
              { key: "provider", header: "Provider", cell: (r) => r.provider, csv: (r) => r.provider },
              { key: "model", header: "Model", cell: (r) => <span className="font-mono text-[13px]">{r.model}</span>, csv: (r) => r.model },
              { key: "kind", header: "Token kind", cell: (r) => r.kind, csv: (r) => r.kind },
              { key: "price", header: "Per million", align: "right", cell: (r) => <span className="num">{r.price_per_million}</span>, csv: (r) => r.price_per_million },
              { key: "from", header: "Valid from", cell: (r) => r.valid_from, csv: (r) => r.valid_from },
              { key: "source", header: "Source", cell: (r) => (r.source === "managed" ? <Badge>managed</Badge> : <Badge tone="accent">your row</Badge>), csv: (r) => r.source },
              {
                key: "act",
                header: "",
                cell: (r) =>
                  r.source === "organization" ? (
                    <Button size="sm" variant="ghost" onClick={() => remove.mutate(r)}>
                      Reverse
                    </Button>
                  ) : null,
              },
            ]}
          />
        )}
        <FormError error={remove.error} />
      </Section>
      <AddRow />
      <Section title="Count errors and unknown models" lead="These messages have no cost, never zero. Reconciliation shows them in what the bill has that no message explains, until the counts or the row arrive.">
        {!messages.data ? (
          <Loading />
        ) : missing.length ? (
          <DataTable
            caption="Messages without a cost"
            stack
            rows={missing}
            rowKey={(m) => m.message_id}
            columns={[
              { key: "at", header: "When", cell: (m) => formatDateTime(m.at) },
              { key: "person", header: "Person", cell: (m) => personName(m.person) },
              { key: "model", header: "Model", cell: (m) => `${m.provider}/${m.model}` },
              {
                key: "tokens",
                header: "Tokens",
                align: "right",
                cell: (m) => {
                  const counts = m.tokens as Record<string, number> | null | undefined;
                  return counts ? <span className="num">{formatTokens(Object.values(counts).reduce((a, b) => a + b, 0))}</span> : "none";
                },
              },
              { key: "why", header: "Shown as", cell: (m) => <Badge tone="exception">{m.cost_label === "count_error" ? "count error" : "unknown model"}</Badge> },
            ]}
          />
        ) : (
          <Empty title="Every message has a cost." />
        )}
      </Section>
    </>
  );
}

function AddRow() {
  const [provider, setProvider] = useState("anthropic");
  const [model, setModel] = useState("");
  const [kind, setKind] = useState("input");
  const [price, setPrice] = useState("");
  const [from, setFrom] = useState(today());
  const add = useMutation({
    mutationFn: () => api.addPriceRow({ provider, model, kind, price_per_million: price, valid_from: from }),
    onSuccess: async () => {
      await refreshAll();
      notify("Row added", `${model} ${kind}`);
      setPrice("");
    },
  });
  return (
    <Section title="Add a row" lead="For a model the managed table lacks, or a price you negotiated: the same provider, model, kind and valid-from day overrides the managed row.">
      <form
        className="grid max-w-4xl gap-3 sm:grid-cols-5 sm:items-end"
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
          <Select value={kind} onChange={(e) => setKind(e.target.value)}>
            {kinds.map((k) => (
              <option key={k}>{k}</option>
            ))}
          </Select>
        </Field>
        <Field label="Price per million">
          <Input inputMode="decimal" value={price} onChange={(e) => setPrice(e.target.value)} />
        </Field>
        <Field label="Valid from">
          <Input type="date" value={from} onChange={(e) => setFrom(e.target.value)} />
        </Field>
        <div className="sm:col-span-5">
          <Button type="submit" disabled={!model || !price || add.isPending}>
            Add the row
          </Button>
        </div>
        <div className="sm:col-span-5">
          <FormError error={add.error} />
        </div>
      </form>
    </Section>
  );
}
