import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { BillApi } from "@/api/contract";
import { api } from "@/api/endpoints";
import { Amount } from "@/components/controls";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ErrorBox, Loading, Note, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select } from "@/components/ui/form";
import { DataTable } from "@/components/ui/table";
import { notify } from "@/components/ui/toast";
import { formatDateTime, formatRange } from "@/lib/format";
import { refreshAll } from "@/lib/queries";
import { UploadCsvDialog } from "./bills-upload";

const providerName: Record<BillApi, string> = {
  anthropic_enterprise: "Anthropic Enterprise",
  anthropic_console: "Anthropic Console",
  openai: "OpenAI",
};

// Docs: Bills and reconciliation. On each bill's own range: bill = cost of
// its provider's messages + what the bill has that no message explains.
// That line is zero or positive; negative means a message is wrong.
export function BillsPage(): ReactNode {
  const reconciliation = useQuery({ queryKey: ["reconciliation"], queryFn: api.reconciliation });
  const rows = reconciliation.data?.toSorted((a, b) => b.to.localeCompare(a.to)) ?? [];
  const wrong = rows.filter((r) => r.a_message_is_wrong);
  const [uploading, setUploading] = useState(false);
  return (
    <>
      <PageHeader
        title="Bills and reconciliation"
        lead="Each bill is checked on its own range, per provider. Tags play no part in the check."
        actions={<Button variant="primary" onClick={() => setUploading(true)}>Upload CSV</Button>}
      />
      <UploadCsvDialog open={uploading} onOpenChange={setUploading} />
      {wrong.length ? <Note tone="exception">{wrong.length} bill{wrong.length > 1 ? "s have" : " has"} less than the cost of its messages: a message is wrong.</Note> : null}
      <Section title="Reconciliation">
        {reconciliation.error ? <ErrorBox error={reconciliation.error} /> : null}
        {!reconciliation.data ? (
          <Loading />
        ) : (
          <DataTable
            caption="Reconciliation"
            stack
            csvName="reconciliation.csv"
            rows={rows}
            rowKey={(r) => r.bill_id}
            empty="No bills yet. Upload a CSV, connect a provider or add a bill below."
            rowClassName={(r) => (r.a_message_is_wrong ? "bg-exception-tint" : "")}
            columns={[
              { key: "provider", header: "Provider", cell: (r) => <span className="flex items-center gap-1.5">{r.provider}{r.final ? null : <Badge>not final</Badge>}</span>, csv: (r) => r.provider },
              { key: "range", header: "Range", cell: (r) => formatRange(r.from, r.to), csv: (r) => `${r.from} to ${r.to}` },
              { key: "bill", header: "Bill", align: "right", cell: (r) => <Amount value={r.bill} />, csv: (r) => r.bill },
              { key: "messages", header: "Cost of messages", align: "right", cell: (r) => <Amount value={r.messages} />, csv: (r) => r.messages },
              {
                key: "unexplained",
                header: "No message explains",
                align: "right",
                cell: (r) => (
                  <span className={`flex flex-col items-end ${r.a_message_is_wrong ? "text-exception" : ""}`}>
                    <Amount value={r.unexplained} />
                    {r.a_message_is_wrong ? <Badge tone="exception">a message is wrong</Badge> : null}
                  </span>
                ),
                csv: (r) => r.unexplained,
              },
              {
                key: "missing",
                header: "Without a cost",
                wide: true,
                cell: (r) => (
                  <span className="flex flex-wrap gap-1">
                    {r.count_errors ? <Badge tone="exception">{r.count_errors} count errors</Badge> : null}
                    {r.unknown_models.map((m) => (
                      <Badge key={m} tone="exception">{m}</Badge>
                    ))}
                  </span>
                ),
              },
            ]}
          />
        )}
      </Section>
      <Connections />
      <AddBill />
    </>
  );
}

function Connections() {
  const connections = useQuery({ queryKey: ["bill-connections"], queryFn: api.billConnections });
  const [provider, setProvider] = useState<BillApi>("anthropic_console");
  const [key, setKey] = useState("");
  const connect = useMutation({
    mutationFn: () => api.connectBill({ provider, key }),
    onSuccess: async () => {
      await refreshAll();
      setKey("");
      notify("Connected", "Pulled once a day for the last 62 days.");
    },
  });
  const pull = useMutation({ mutationFn: api.pullBills, onSuccess: async () => { await refreshAll(); notify("Pull started"); } });
  const disconnect = useMutation({ mutationFn: api.disconnectBill, onSuccess: refreshAll });
  return (
    <Section title="Connected providers" lead="A connected provider is pulled once a day for the last 62 days.">
      <DataTable
        caption="Connected providers"
        stack
        rows={connections.data ?? []}
        rowKey={(c) => c.id}
        empty="No provider connected."
        columns={[
          { key: "provider", header: "Provider", cell: (c) => providerName[c.provider] },
          { key: "key", header: "Key", cell: (c) => <span className="font-mono text-xs">{c.key_hint}</span> },
          { key: "status", header: "Status", cell: (c) => <span className="flex flex-col"><Badge tone={c.status === "ok" ? "posted" : "exception"}>{c.status}</Badge>{c.message ? <span className="text-xs text-exception">{c.message}</span> : null}</span> },
          { key: "last", header: "Last pull", cell: (c) => (c.last_pull_at ? formatDateTime(c.last_pull_at) : "not yet") },
          {
            key: "act",
            header: "",
            cell: (c) => (
              <span className="flex gap-2">
                <Button size="sm" onClick={() => pull.mutate(c.id)}>Pull now</Button>
                <Button size="sm" variant="ghost" onClick={() => disconnect.mutate(c.id)}>Disconnect</Button>
              </span>
            ),
          },
        ]}
      />
      <form
        className="mt-4 grid max-w-2xl gap-3 sm:grid-cols-3 sm:items-end"
        onSubmit={(e) => {
          e.preventDefault();
          connect.mutate();
        }}
      >
        <Field label="Provider to connect">
          <Select value={provider} onChange={(e) => setProvider(e.target.value as BillApi)}>
            {(Object.keys(providerName) as BillApi[]).map((p) => (
              <option key={p} value={p}>{providerName[p]}</option>
            ))}
          </Select>
        </Field>
        <Field label="Admin key">
          <Input type="password" value={key} onChange={(e) => setKey(e.target.value)} />
        </Field>
        <Button type="submit" disabled={!key || connect.isPending}>Connect</Button>
        <div className="sm:col-span-3">
          <FormError error={connect.error ?? pull.error ?? disconnect.error} />
        </div>
      </form>
    </Section>
  );
}

function AddBill() {
  const [provider, setProvider] = useState("");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [amount, setAmount] = useState("");
  const [currency, setCurrency] = useState("USD");
  const add = useMutation({
    mutationFn: () => api.addBill({ provider, from, to, amount, currency, typed_in: true }),
    onSuccess: async () => {
      await refreshAll();
      notify("Bill added");
      setAmount("");
    },
  });
  return (
    <Section title="Add a bill by hand" lead="Metered usage only: seats and subscriptions are not on a bill. A new bill for the same provider and range restates the old one.">
      <form
        className="grid max-w-4xl gap-3 sm:grid-cols-5 sm:items-end"
        onSubmit={(e) => {
          e.preventDefault();
          add.mutate();
        }}
      >
        <Field label="Bill provider"><Input value={provider} placeholder="bedrock" onChange={(e) => setProvider(e.target.value)} /></Field>
        <Field label="First day"><Input type="date" value={from} onChange={(e) => setFrom(e.target.value)} /></Field>
        <Field label="Last day"><Input type="date" value={to} onChange={(e) => setTo(e.target.value)} /></Field>
        <Field label="Amount"><Input inputMode="decimal" value={amount} onChange={(e) => setAmount(e.target.value)} /></Field>
        <Field label="Currency"><Input value={currency} onChange={(e) => setCurrency(e.target.value)} /></Field>
        <div className="sm:col-span-5"><Button type="submit" disabled={!provider || !from || !to || !amount || add.isPending}>Add the bill</Button></div>
        <div className="sm:col-span-5"><FormError error={add.error} /></div>
      </form>
    </Section>
  );
}
