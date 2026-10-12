import { useMutation, useQuery } from "@tanstack/react-query";
import { getRouteApi } from "@tanstack/react-router";
import { useState, type ReactNode } from "react";
import type { Bill, BillConnection, BillProvider, CostSource, Reconciliation } from "@/api/contract";
import { api } from "@/api/endpoints";
import { PeriodSelect } from "@/components/period-select";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Kpi, KpiRow, Loading, MoneyText, Note, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select, Textarea } from "@/components/ui/form";
import { DataTable, type Column } from "@/components/ui/table";
import { notify } from "@/components/ui/toast";
import { formatDate, formatDateTime, formatPeriod } from "@/lib/format";
import { formatMoney, formatShare, parseAmount, toDecimal } from "@/lib/money";
import { refreshAll, useMe, usePeriods } from "@/lib/queries";
import { addMonths } from "@/lib/format";
import { thisPeriod } from "@/lib/today";

const billsRoute = getRouteApi("/app/bills");

/** What the close reconciles against, per cost source (docs: Bills). */
export const costSourceText: Record<CostSource, { name: string; against: string; list: string }> = {
  enterprise_api_rates: { name: "Enterprise plan, usage at API rates", against: "the Enterprise Analytics cost report at list price; the discount is its own line", list: "money, before discount" },
  console_api_key: { name: "Console or API key", against: "the Usage and Cost report, per workspace and day", list: "money" },
  seat_plan: { name: "Seat-based plan (Claude Team, Max, Pro)", against: "nothing: the seat is a flat fee and there is no bill to reconcile", list: "tokens at API rates, labelled as an estimate on every figure" },
  cloud_marketplace: { name: "Bedrock, Vertex, Foundry", against: "the cloud's export, uploaded as CSV", list: "approximate" },
  none: { name: "None yet", against: "nothing yet; pick a cost source under Settings", list: "a list-price estimate" },
};

const providerName: Record<BillConnection["provider"], string> = {
  anthropic_enterprise: "Anthropic Enterprise Analytics API",
  anthropic_console: "Anthropic Console, Usage and Cost Admin API",
  openai: "OpenAI Costs API",
};

const billProviders: { value: BillProvider; label: string }[] = [
  { value: "anthropic_enterprise", label: "Anthropic Enterprise" },
  { value: "anthropic_console", label: "Anthropic Console" },
  { value: "openai", label: "OpenAI" },
  { value: "bedrock", label: "Bedrock" },
  { value: "vertex", label: "Vertex" },
  { value: "foundry", label: "Foundry" },
  { value: "other", label: "Other" },
];

function Connect({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [provider, setProvider] = useState<BillConnection["provider"]>("anthropic_console");
  const [key, setKey] = useState("");
  const connect = useMutation({
    mutationFn: () => api.connectBill(provider, key),
    onSuccess: async () => {
      await refreshAll();
      notify("Connected", "The first pull loads the current and the previous period.");
      setKey("");
      onClose();
    },
  });
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()} title="Connect a bill" description="Bills are pulled every day. The key is stored encrypted and only reads usage and cost.">
      <form className="space-y-4" onSubmit={(e) => { e.preventDefault(); connect.mutate(); }}>
        <Field label="Provider">
          <Select value={provider} onChange={(e) => setProvider(e.target.value as BillConnection["provider"])}>
            {Object.entries(providerName).map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </Select>
        </Field>
        <Field label="Admin key" hint="An Enterprise Analytics key, a Console admin key or an OpenAI admin key.">
          <Input type="password" required value={key} onChange={(e) => setKey(e.target.value)} autoComplete="off" />
        </Field>
        <FormError error={connect.error} />
        <div className="flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={connect.isPending}>
            Connect
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

function TypeIn({ open, onClose, period }: { open: boolean; onClose: () => void; period: string }) {
  const me = useMe();
  const [form, setForm] = useState({ provider: "other" as BillProvider, label: "", period, currency: me.organization.reporting_currency, gross: "", discount: "0", credit: "0", net: "" });
  const set = (k: keyof typeof form, v: string) => setForm((f) => ({ ...f, [k]: v }));
  const add = useMutation({
    mutationFn: () => {
      const money = (text: string) => {
        const value = parseAmount(text || "0", form.currency);
        if (!value) throw new Error(`"${text}" is not an amount.`);
        return value;
      };
      return api.addBill({ provider: form.provider, label: form.label || billProviders.find((p) => p.value === form.provider)?.label || "Other", period: form.period, gross: money(form.gross || form.net), discount: money(form.discount), credit: money(form.credit), net: money(form.net) });
    },
    onSuccess: async () => {
      await refreshAll();
      notify("Bill added");
      onClose();
    },
  });
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()} title="Type in a bill" description="For a provider with no API. Metered usage only: seats and other flat fees stay off.">
      <form className="space-y-4" onSubmit={(e) => { e.preventDefault(); add.mutate(); }}>
        <div className="grid gap-3 sm:grid-cols-2">
          <Field label="Provider">
            <Select value={form.provider} onChange={(e) => set("provider", e.target.value)}>
              {billProviders.map((p) => (
                <option key={p.value} value={p.value}>
                  {p.label}
                </option>
              ))}
            </Select>
          </Field>
          <Field label="Name on reports">
            <Input value={form.label} onChange={(e) => set("label", e.target.value)} placeholder="Cursor" />
          </Field>
          <Field label="Period">
            <Input type="month" required value={form.period} onChange={(e) => set("period", e.target.value)} />
          </Field>
          <Field label="Currency">
            <Select value={form.currency} onChange={(e) => set("currency", e.target.value)}>
              {["USD", "EUR", "GBP", "CHF"].map((c) => (
                <option key={c}>{c}</option>
              ))}
            </Select>
          </Field>
          <Field label="Gross">
            <Input inputMode="decimal" value={form.gross} onChange={(e) => set("gross", e.target.value)} />
          </Field>
          <Field label="Discount">
            <Input inputMode="decimal" value={form.discount} onChange={(e) => set("discount", e.target.value)} />
          </Field>
          <Field label="Credit">
            <Input inputMode="decimal" value={form.credit} onChange={(e) => set("credit", e.target.value)} />
          </Field>
          <Field label="Net">
            <Input inputMode="decimal" required value={form.net} onChange={(e) => set("net", e.target.value)} />
          </Field>
        </div>
        <FormError error={add.error} />
        <div className="flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={add.isPending}>
            Add bill
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

function UploadCsv({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [text, setText] = useState("");
  const preview = useMutation({ mutationFn: () => api.previewBillCsv(text) });
  const save = useMutation({
    mutationFn: () => api.importBillCsv(text),
    onSuccess: async (bills) => {
      await refreshAll();
      notify(`${bills.length} ${bills.length === 1 ? "bill" : "bills"} added`);
      setText("");
      preview.reset();
      onClose();
    },
  });
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()} title="Upload a bill as CSV" description="Columns: provider, period, gross, discount, credit, net, currency. Or upload the cloud's own export." wide>
      <div className="space-y-4">
        <Field label="CSV file">
          <Input type="file" accept=".csv,text/csv" className="pt-1.5" onChange={(e) => { void e.target.files?.[0]?.text().then(setText); preview.reset(); }} />
        </Field>
        <Field label="Or paste it">
          <Textarea aria-label="Bill CSV" rows={4} className="font-mono text-xs" value={text} onChange={(e) => { setText(e.target.value); preview.reset(); }} placeholder={"provider,period,gross,discount,credit,net,currency\nCursor,2026-09,612.40,0,0,612.40,USD"} />
        </Field>
        {preview.data ? (
          <div className="rounded-md border border-line p-3 text-sm">
            {preview.data.rows.map((b, i) => (
              <p key={i}>
                {b.label} · {formatPeriod(b.period)} · net {formatMoney(b.net)}
              </p>
            ))}
            {preview.data.problems.map((p) => (
              <p key={`${p.row}${p.message}`} className="text-exception">
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
              Save {preview.data.rows.length} {preview.data.rows.length === 1 ? "bill" : "bills"}
            </Button>
          ) : (
            <Button variant="primary" onClick={() => preview.mutate()} disabled={!text.trim()}>
              Check the file
            </Button>
          )}
        </div>
      </div>
    </Dialog>
  );
}

export function ReconciliationTable({ data, period }: { data: Reconciliation; period: string }) {
  type Row = Reconciliation["bills"][number];
  const columns: Column<Row>[] = [
    { key: "bill", header: "Provider", cell: (r) => r.label, csv: (r) => r.label },
    { key: "net", header: "Bill", align: "right", cell: (r) => <MoneyText value={r.bill_net} />, csv: (r) => toDecimal(r.bill_net) },
    { key: "entries", header: "Token entries", align: "right", cell: (r) => <MoneyText value={r.token_entries} />, csv: (r) => toDecimal(r.token_entries) },
    { key: "residual", header: "Residual", align: "right", cell: (r) => <MoneyText value={r.residual} />, csv: (r) => toDecimal(r.residual) },
  ];
  return (
    <DataTable
      columns={columns}
      rows={data.bills}
      rowKey={(r) => r.bill}
      csvName={`reconciliation-${period}.csv`}
      caption="Reconciliation"
      empty="No bills for this period yet."
      totals={{
        label: "Organization",
        values: { net: <MoneyText value={data.total.bill_net} />, entries: <MoneyText value={data.total.token_entries} />, residual: <MoneyText value={data.total.residual} /> },
        csv: { net: toDecimal(data.total.bill_net), entries: toDecimal(data.total.token_entries), residual: toDecimal(data.total.residual) },
      }}
    />
  );
}

export function BillsPage(): ReactNode {
  const me = useMe();
  const navigate = billsRoute.useNavigate();
  const search = billsRoute.useSearch();
  const periods = usePeriods();
  const lastWithBills = addMonths(thisPeriod(), -1);
  const period = search.period ?? lastWithBills;
  const bills = useQuery({ queryKey: ["bills", period], queryFn: () => api.bills(period) });
  const recon = useQuery({ queryKey: ["reconciliation", period], queryFn: () => api.reconciliation(period) });
  const connections = useQuery({ queryKey: ["bill-connections"], queryFn: api.billConnections });
  const pull = useMutation({ mutationFn: api.pullBills, onSuccess: async () => { await refreshAll(); notify("Pulled", "Bills are up to date."); } });
  const [dialog, setDialog] = useState<"" | "connect" | "type" | "csv">("");
  const [revisions, setRevisions] = useState<Bill | null>(null);
  const source = costSourceText[me.organization.cost_source];
  const closed = periods.data?.find((p) => p.id === period)?.status === "closed";
  const columns: Column<Bill>[] = [
    { key: "provider", header: "Provider", cell: (b) => <span className="font-medium">{b.label}</span>, csv: (b) => b.label },
    { key: "source", header: "From", wide: true, cell: (b) => <Badge>{b.source === "api" ? "API" : b.source === "csv" ? "CSV" : "typed in"}</Badge>, csv: (b) => b.source },
    { key: "received", header: "Received", wide: true, cell: (b) => formatDate(b.received_at), csv: (b) => b.received_at },
    {
      key: "revision",
      header: "Revision",
      wide: true,
      cell: (b) => (b.revisions.length ? <button type="button" className="text-accent hover:underline" onClick={() => setRevisions(b)}>{b.revision}, restated</button> : b.revision),
      csv: (b) => b.revision,
    },
    { key: "gross", header: "Gross", align: "right", wide: true, cell: (b) => <MoneyText value={b.gross} />, csv: (b) => toDecimal(b.gross) },
    { key: "discount", header: "Discount", align: "right", wide: true, cell: (b) => <MoneyText value={b.discount} />, csv: (b) => toDecimal(b.discount) },
    { key: "credit", header: "Credit", align: "right", wide: true, cell: (b) => <MoneyText value={b.credit} />, csv: (b) => toDecimal(b.credit) },
    { key: "net", header: "Net", align: "right", cell: (b) => <MoneyText value={b.net} />, csv: (b) => toDecimal(b.net) },
  ];
  return (
    <>
      <PageHeader
        title="Bills"
        lead="Each provider's figure for a period's metered usage. Token entries plus residual must add up to each bill."
        actions={
          <>
            <Button onClick={() => setDialog("csv")}>Upload CSV</Button>
            <Button onClick={() => setDialog("type")}>Type in</Button>
            <Button variant="primary" onClick={() => setDialog("connect")}>
              Connect a bill
            </Button>
          </>
        }
      />
      <Note>
        <b>{source.name}.</b> The close reconciles against {source.against}. List value is {source.list}.
      </Note>
      <div className="mb-6">
        <PeriodSelect value={period} onChange={(p) => void navigate({ search: { period: p } })} />
      </div>
      {recon.data ? (
        <KpiRow>
          <Kpi label="Bills" value={formatMoney(recon.data.total.bill_net)} note={`${recon.data.bills.length} providers`} />
          <Kpi label="Token entries" value={formatMoney(recon.data.total.token_entries)} />
          <Kpi label="Residual" value={formatMoney(recon.data.total.residual)} note="no token entry explains it" tone={Number(recon.data.total.residual.nanos) ? "exception" : undefined} />
          <Kpi label="Attribution" value={formatShare(recon.data.attribution)} note="share of the bills that token entries explain" tone="posted" />
        </KpiRow>
      ) : null}
      <Section title={`Bills for ${formatPeriod(period)}`} lead={closed ? "This period is closed." : undefined}>
        {bills.isPending ? <Loading /> : bills.isError ? <ErrorBox error={bills.error} /> : (
          <DataTable stack columns={columns} rows={bills.data} rowKey={(b) => b.id} csvName={`bills-${period}.csv`} caption="Bills" empty="No bills for this period yet. Connect a provider or upload the bill as CSV." />
        )}
      </Section>
      <Section title="Reconciliation" lead="Per bill: token entries plus residual equal the bill. Tags play no part in this check.">
        {recon.data ? <ReconciliationTable data={recon.data} period={period} /> : recon.isError ? <ErrorBox error={recon.error} /> : <Loading />}
      </Section>
      <Section title="Connected providers">
        {connections.data?.length ? (
          <ul className="divide-y divide-line rounded-lg border border-line">
            {connections.data.map((c) => (
              <li key={c.id} className="flex flex-col gap-2 px-4 py-3 sm:flex-row sm:items-center sm:justify-between">
                <div>
                  <div className="text-sm font-medium">{providerName[c.provider]}</div>
                  <div className="text-xs text-muted">
                    {c.status === "ok" ? "Working" : `Failing: ${c.message ?? "check the key"}`} · last pull {c.last_pull_at ? formatDateTime(c.last_pull_at) : "never"}
                  </div>
                </div>
                <Button size="sm" onClick={() => pull.mutate(c.id)} disabled={pull.isPending}>
                  Pull now
                </Button>
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-sm text-muted">No provider connected. Bills from providers without an API go in as CSV or typed in.</p>
        )}
      </Section>
      <Connect open={dialog === "connect"} onClose={() => setDialog("")} />
      <TypeIn open={dialog === "type"} onClose={() => setDialog("")} period={period} />
      <UploadCsv open={dialog === "csv"} onClose={() => setDialog("")} />
      <Dialog open={Boolean(revisions)} onOpenChange={() => setRevisions(null)} title={`${revisions?.label ?? ""} revisions`} description="A provider can restate a bill. Each revision is kept.">
        <ul className="space-y-2 text-sm">
          {revisions ? (
            <li className="flex justify-between">
              <span>Revision {revisions.revision} · {formatDate(revisions.received_at)} · current</span>
              <MoneyText value={revisions.net} />
            </li>
          ) : null}
          {revisions?.revisions.map((r) => (
            <li key={r.revision} className="flex justify-between text-muted">
              <span>Revision {r.revision} · {formatDate(r.received_at)}</span>
              <MoneyText value={r.net} />
            </li>
          ))}
        </ul>
      </Dialog>
    </>
  );
}
