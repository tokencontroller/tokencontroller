import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { BillApi, Contract, ContractKind, Reconciliation } from "@/api/contract";
import { api } from "@/api/endpoints";
import { Amount } from "@/components/controls";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { Empty, ErrorBox, Figure, Loading, Note, PageHeader, Section } from "@/components/ui/display";
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

// Docs: Bills and reconciliation. Each bill is checked under its contract on
// its own range. Metered: bill = cost of its messages + what no message
// explains, zero or more. Flat: bill = fee + extra usage, and the plan
// covered the cost of its messages less the extra usage. Beside each check
// stands what it could hide: the provider's messages with no contract, and
// the part whose contract came only from the provider having one.
export function BillsPage(): ReactNode {
  const reconciliation = useQuery({ queryKey: ["reconciliation"], queryFn: api.reconciliation });
  const contracts = useQuery({ queryKey: ["contracts"], queryFn: api.contracts });
  const rows = reconciliation.data?.toSorted((a, b) => b.to.localeCompare(a.to) || a.contract.localeCompare(b.contract)) ?? [];
  const wrong = rows.filter((r) => r.a_message_is_wrong).length;
  const [uploading, setUploading] = useState(false);
  const [adding, setAdding] = useState(false);
  const titleOf = (key: string) => contracts.data?.find((c) => c.key === key)?.title ?? key;
  return (
    <>
      <PageHeader
        title="Bills"
        actions={
          <>
            <Button onClick={() => setUploading(true)}>Upload CSV</Button>
            <Button variant="primary" onClick={() => setAdding(true)}>Add a bill</Button>
          </>
        }
      />
      <UploadCsvDialog open={uploading} onOpenChange={setUploading} />
      <AddBillDialog open={adding} onOpenChange={setAdding} contracts={contracts.data ?? []} />
      {wrong ? <Note tone="exception">{wrong === 1 ? "1 bill: a message is wrong." : `${String(wrong)} bills: a message is wrong.`}</Note> : null}
      {reconciliation.error ? <ErrorBox error={reconciliation.error} /> : null}
      {!reconciliation.data ? (
        <Loading />
      ) : rows.length ? (
        rows.map((r) => <Check key={r.bill_id} row={r} title={titleOf(r.contract)} />)
      ) : (
        <Section title="Checks">
          <Empty title="No bills yet." action={<Button size="sm" onClick={() => setAdding(true)}>Add a bill</Button>} />
        </Section>
      )}
      <Contracts contracts={contracts.data} error={contracts.error} />
      <Connections />
    </>
  );
}

/** Every contract of the organization: from its bills, its values of `contract` and its rules. */
function Contracts({ contracts, error }: { contracts: Contract[] | undefined; error: Error | null }) {
  return (
    <Section title="Contracts" flush hint="Each bill and each message is under one contract. A new contract comes with a bill, a value of contract under Tags, or a rule.">
      {error ? <div className="px-5 pb-5"><ErrorBox error={error} /></div> : null}
      {!contracts ? (
        error ? null : <div className="px-5 pb-5"><Loading /></div>
      ) : (
        <DataTable
          caption="Contracts"
          stack
          rows={contracts}
          rowKey={(c) => c.key}
          empty="No contracts yet. The first bill brings one."
          columns={[
            { key: "key", header: "Contract", cell: (c) => <span className="inline-flex flex-col items-end sm:items-start"><span className="font-medium">{c.title ?? c.key}</span>{c.title ? <span className="font-mono text-caption text-muted">{c.key}</span> : null}</span> },
            { key: "provider", header: "Provider", cell: (c) => c.provider },
            { key: "kind", header: "Kind", cell: (c) => <Badge tone={c.kind === "flat" ? "accent" : "neutral"}>{c.kind}</Badge> },
            { key: "fee", header: "Fee per month", align: "right", cell: (c) => (c.kind === "flat" ? (c.fee ? <Amount value={c.fee} /> : <span className="text-muted">not known</span>) : null) },
          ]}
        />
      )}
    </Section>
  );
}

/** Where a check stands, in one badge. */
function status(r: Reconciliation): ReactNode {
  if (r.a_message_is_wrong) return <Badge tone="exception">a message is wrong</Badge>;
  if (r.contract_kind === "flat") return r.covered === null || r.covered === undefined ? <Badge>fee not known</Badge> : <Badge tone="posted">covered</Badge>;
  return Number(r.unexplained) === 0 ? <Badge tone="posted">balanced</Badge> : <Badge>gap</Badge>;
}

/** One bill's check under its contract: three or four figures, then what stands beside it. */
function Check({ row: r, title }: { row: Reconciliation; title: string }) {
  const flat = r.contract_kind === "flat";
  const money = (value: string | null | undefined) => <Amount value={value ?? null} currency={r.currency} />;
  return (
    <div role="group" aria-label={`${title} ${formatRange(r.from, r.to)}`}>
      <Section
        title={title}
        actions={
          <span className="flex flex-wrap items-center gap-2 text-small text-muted">
            {formatRange(r.from, r.to)}
            <Badge tone={flat ? "accent" : "neutral"}>{flat ? "flat" : "metered"}</Badge>
            {r.final ? null : <Badge>not final</Badge>}
            {status(r)}
          </span>
        }
      >
        <div className={`grid grid-cols-2 gap-4 ${flat ? "sm:grid-cols-4" : "sm:grid-cols-3"}`}>
          <Figure size="lg" label="Bill" value={money(r.bill)} />
          {flat ? (
            <>
              <Figure label="Fee" value={money(r.fee)} />
              <Figure label="Extra usage" value={money(r.extra_usage)} />
              <Figure label="Covered" tone={r.a_message_is_wrong ? "exception" : undefined} value={money(r.covered)} />
            </>
          ) : (
            <>
              <Figure label="Cost of messages" value={money(r.messages)} />
              <Figure label="No message explains" tone={r.a_message_is_wrong ? "exception" : undefined} value={money(r.unexplained)} />
            </>
          )}
        </div>
        {/* The two lines beside every check, 0.00 included, so a check cannot balance by leaving messages out. */}
        <ul className="mt-4 flex flex-col gap-1 border-t border-line pt-3 text-small">
          <li className={`flex justify-between gap-3 ${Number(r.same_provider_no_contract) === 0 ? "text-muted" : "text-exception"}`}>
            <span>Same provider, no contract</span>
            {money(r.same_provider_no_contract)}
          </li>
          <li className="flex justify-between gap-3 text-muted">
            <span>Contract only from the provider</span>
            {money(r.contract_from_fallback)}
          </li>
          {r.count_errors ? (
            <li className="flex justify-between gap-3 text-exception">
              <span>Count errors</span>
              <span className="num">{r.count_errors} messages</span>
            </li>
          ) : null}
          {/* The check names the models only; their messages and tokens are on the price table. */}
          {r.not_priced_models.length ? (
            <li className="flex justify-between gap-3 text-exception">
              <span>Not priced yet</span>
              <span className="font-mono text-caption">{r.not_priced_models.join(", ")}</span>
            </li>
          ) : null}
        </ul>
      </Section>
    </div>
  );
}

function Connections() {
  const connections = useQuery({ queryKey: ["bill-connections"], queryFn: api.billConnections });
  const [connecting, setConnecting] = useState(false);
  const pull = useMutation({ mutationFn: api.pullBills, onSuccess: async () => { await refreshAll(); notify("Pull started"); } });
  const disconnect = useMutation({ mutationFn: api.disconnectBill, onSuccess: refreshAll });
  const list = connections.data ?? [];
  return (
    <Section
      title="Connected providers"
      flush={list.length > 0}
      hint="Pulled once a day for the last 62 days."
      actions={<Button size="sm" onClick={() => setConnecting(true)}>Connect</Button>}
    >
      <ConnectDialog open={connecting} onOpenChange={setConnecting} />
      {list.length ? (
        <DataTable
          caption="Connected providers"
          stack
          rows={list}
          rowKey={(c) => c.id}
          columns={[
            { key: "provider", header: "Provider", cell: (c) => <span className="font-medium">{providerName[c.provider]}</span> },
            { key: "key", header: "Key", wide: true, cell: (c) => <span className="font-mono text-caption">{c.key_hint}</span> },
            { key: "status", header: "Status", cell: (c) => <span className="flex flex-col"><Badge tone={c.status === "ok" ? "posted" : "exception"}>{c.status}</Badge>{c.message ? <span className="text-caption text-exception">{c.message}</span> : null}</span> },
            { key: "last", header: "Last pull", cell: (c) => (c.last_pull_at ? formatDateTime(c.last_pull_at) : "not yet") },
            {
              key: "act",
              header: "",
              align: "right",
              cell: (c) => (
                <span className="flex justify-end gap-2">
                  <Button size="sm" onClick={() => pull.mutate(c.id)}>Pull now</Button>
                  <Button size="sm" variant="danger" onClick={() => disconnect.mutate(c.id)}>Disconnect</Button>
                </span>
              ),
            },
          ]}
        />
      ) : (
        <Empty title="No provider connected." />
      )}
      <FormError error={pull.error ?? disconnect.error} />
    </Section>
  );
}

function ConnectDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const [provider, setProvider] = useState<BillApi>("anthropic_console");
  const [key, setKey] = useState("");
  const connect = useMutation({
    mutationFn: () => api.connectBill({ provider, key }),
    onSuccess: async () => {
      await refreshAll();
      setKey("");
      onOpenChange(false);
      notify("Connected", "Pulled once a day for the last 62 days.");
    },
  });
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title="Connect a provider">
      <form
        className="flex flex-col gap-3"
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
        <FormError error={connect.error} />
        <div className="flex justify-end gap-2">
          <Button onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button variant="primary" type="submit" disabled={!key || connect.isPending}>Connect</Button>
        </div>
      </form>
    </Dialog>
  );
}

/** A new contract named in the dialog, and its kind. */
const NEW = "__new";

function AddBillDialog({ open, onOpenChange, contracts }: { open: boolean; onOpenChange: (open: boolean) => void; contracts: Contract[] }) {
  const [contract, setContract] = useState("");
  const [newKey, setNewKey] = useState("");
  const [newKind, setNewKind] = useState<ContractKind>("metered");
  const [provider, setProvider] = useState("");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [amount, setAmount] = useState("");
  const [currency, setCurrency] = useState("USD");
  const [fee, setFee] = useState("");
  const [extra, setExtra] = useState("");
  const chosen = contracts.find((c) => c.key === contract);
  const kind: ContractKind = chosen ? (chosen.kind as ContractKind) : contract === NEW ? newKind : "metered";
  const billProvider = chosen?.provider ?? provider;
  const add = useMutation({
    mutationFn: () =>
      api.addBill({
        provider: billProvider,
        from,
        to,
        amount,
        currency,
        typed_in: true,
        ...(chosen ? { contract: chosen.key } : contract === NEW ? { contract: newKey, contract_kind: newKind } : {}),
        ...(kind === "flat" && fee ? { fee } : {}),
        ...(kind === "flat" && extra ? { extra_usage: extra } : {}),
      }),
    onSuccess: async () => {
      await refreshAll();
      notify("Bill added");
      setAmount("");
      setFee("");
      setExtra("");
      onOpenChange(false);
    },
  });
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title="Add a bill" wide>
      <form
        className="grid gap-3 sm:grid-cols-2"
        onSubmit={(e) => {
          e.preventDefault();
          add.mutate();
        }}
      >
        <Field label="Contract" className="sm:col-span-2">
          <Select value={contract} onChange={(e) => setContract(e.target.value)}>
            <option value="">The provider's only contract</option>
            {contracts.map((c) => (
              <option key={c.key} value={c.key}>{c.title ? `${c.title} (${c.key})` : c.key}</option>
            ))}
            <option value={NEW}>A new contract</option>
          </Select>
        </Field>
        {chosen ? null : <Field label="Bill provider"><Input value={provider} placeholder="anthropic" onChange={(e) => setProvider(e.target.value)} /></Field>}
        {contract === NEW ? (
          <>
            <Field label="Contract key"><Input value={newKey} placeholder="anthropic-api" onChange={(e) => setNewKey(e.target.value)} /></Field>
            <Field label="Kind">
              <Select value={newKind} onChange={(e) => setNewKind(e.target.value as ContractKind)}>
                <option value="metered">metered</option>
                <option value="flat">flat</option>
              </Select>
            </Field>
          </>
        ) : null}
        <Field label="First day"><Input type="date" value={from} onChange={(e) => setFrom(e.target.value)} /></Field>
        <Field label="Last day"><Input type="date" value={to} onChange={(e) => setTo(e.target.value)} /></Field>
        <Field label="Amount"><Input inputMode="decimal" value={amount} onChange={(e) => setAmount(e.target.value)} /></Field>
        <Field label="Currency"><Input value={currency} onChange={(e) => setCurrency(e.target.value)} /></Field>
        {kind === "flat" ? (
          <>
            <Field label="Fee" hint="Either is enough"><Input inputMode="decimal" value={fee} onChange={(e) => setFee(e.target.value)} /></Field>
            <Field label="Extra usage"><Input inputMode="decimal" value={extra} onChange={(e) => setExtra(e.target.value)} /></Field>
          </>
        ) : null}
        <div className="sm:col-span-2"><FormError error={add.error} /></div>
        <div className="flex justify-end gap-2 sm:col-span-2">
          <Button onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button variant="primary" type="submit" disabled={!billProvider || !from || !to || !amount || (contract === NEW && !newKey) || add.isPending}>Add the bill</Button>
        </div>
      </form>
    </Dialog>
  );
}
