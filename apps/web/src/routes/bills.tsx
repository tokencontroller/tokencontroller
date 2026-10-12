import { useMutation, useQuery } from "@tanstack/react-query";
import { getRouteApi } from "@tanstack/react-router";
import { useState, type ReactNode } from "react";
import type { BillApi, BillCheck, BillCloud, BillConnectionView, BillCostSource, BillCsvQuery, BillView, Reconciliation } from "@/api/contract";
import { api } from "@/api/endpoints";
import { PeriodSelect } from "@/components/period-select";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Kpi, KpiRow, Loading, MoneyText, Note, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select, Textarea } from "@/components/ui/form";
import { DataTable, type Column } from "@/components/ui/table";
import { notify } from "@/components/ui/toast";
import { billApiName, billApis, billClouds, billCostSources, billSourceNames, costSourceName, costSourceNames, providerName, providerNames } from "@/lib/bills";
import { describeCostSource } from "@/lib/cost-source";
import { addMonths, formatDate, formatDateTime, formatPeriod } from "@/lib/format";
import { formatMoney, formatShare, fromDecimal, shareOf, sum, toDecimal } from "@/lib/money";
import { refreshAll, useMe } from "@/lib/queries";
import { thisPeriod } from "@/lib/today";

const billsRoute = getRouteApi("/app/bills");

/** An amount as typed, such as `1,240.50`, as the service reads it: no thousands separators. */
const plain = (text: string) => text.replace(/[,\s]/g, "");

const currencies = ["USD", "EUR", "GBP", "CHF"];

function Connect({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [provider, setProvider] = useState<BillApi>("anthropic_enterprise");
  const [key, setKey] = useState("");
  const connect = useMutation({
    mutationFn: () => api.connectBill({ provider, key }),
    onSuccess: async () => {
      await refreshAll();
      notify("Connected", "The first pull loads the current and the previous period.");
      setKey("");
      onClose();
    },
  });
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()} title="Connect a bill" description="A connected provider is pulled once a day, for the current and the previous period.">
      <form className="space-y-4" onSubmit={(e) => { e.preventDefault(); connect.mutate(); }}>
        <Field label="Provider">
          <Select value={provider} onChange={(e) => setProvider(e.target.value as BillApi)}>
            {billApis.map((a) => (
              <option key={a.value} value={a.value}>
                {a.name}
              </option>
            ))}
          </Select>
        </Field>
        <Field label="Admin key" hint={billApis.find((a) => a.value === provider)?.key}>
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
  const reporting = me.organization.reporting_currency;
  const orgSource = me.organization.cost_source;
  const [form, setForm] = useState({
    provider: "other",
    cost_source: (billCostSources as string[]).includes(orgSource ?? "") ? (orgSource as BillCostSource) : "api_key",
    period,
    currency: reporting,
    gross: "",
    discount: "",
    credit: "",
    net: "",
    exchange_rate: "",
  });
  const set = (k: keyof typeof form, v: string) => setForm((f) => ({ ...f, [k]: v }));
  const add = useMutation({
    mutationFn: () =>
      api.addBill({
        period: form.period,
        provider: form.provider,
        cost_source: form.cost_source,
        currency: form.currency,
        net: plain(form.net),
        ...(form.gross.trim() ? { gross: plain(form.gross) } : {}),
        ...(form.discount.trim() ? { discount: plain(form.discount) } : {}),
        ...(form.credit.trim() ? { credit: plain(form.credit) } : {}),
        ...(form.currency !== reporting && form.exchange_rate.trim() ? { exchange_rate: plain(form.exchange_rate) } : {}),
        typed_in: true,
      }),
    onSuccess: async ({ bill, changed }) => {
      await refreshAll();
      if (!changed) notify("Nothing changed", "This bill was already in with the same figures.");
      else if (bill.revision > 1) notify("Bill restated", `Revision ${bill.revision} of the bill from ${providerName(bill.provider)}.`);
      else notify("Bill added");
      onClose();
    },
  });
  const currencyChoices = currencies.includes(reporting) ? currencies : [reporting, ...currencies];
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()} title="Type in a bill" description="For a provider with no API. Metered usage only: seats and other flat fees stay off.">
      <form className="space-y-4" onSubmit={(e) => { e.preventDefault(); add.mutate(); }}>
        <div className="grid gap-3 sm:grid-cols-2">
          <Field label="Provider">
            <Select value={form.provider} onChange={(e) => set("provider", e.target.value)}>
              {Object.entries(providerNames).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </Select>
          </Field>
          <Field label="Cost source">
            <Select value={form.cost_source} onChange={(e) => set("cost_source", e.target.value)}>
              {billCostSources.map((s) => (
                <option key={s} value={s}>
                  {costSourceNames[s]}
                </option>
              ))}
            </Select>
          </Field>
          <Field label="Period">
            <Input type="month" required value={form.period} onChange={(e) => set("period", e.target.value)} />
          </Field>
          <Field label="Currency">
            <Select value={form.currency} onChange={(e) => set("currency", e.target.value)}>
              {currencyChoices.map((c) => (
                <option key={c}>{c}</option>
              ))}
            </Select>
          </Field>
          <Field label="Net">
            <Input inputMode="decimal" required value={form.net} onChange={(e) => set("net", e.target.value)} placeholder="1234.56" />
          </Field>
          <Field label="Gross" hint="Before discount and credit. Left out, it is the other three added up.">
            <Input inputMode="decimal" value={form.gross} onChange={(e) => set("gross", e.target.value)} />
          </Field>
          <Field label="Discount">
            <Input inputMode="decimal" value={form.discount} onChange={(e) => set("discount", e.target.value)} placeholder="0" />
          </Field>
          <Field label="Credit">
            <Input inputMode="decimal" value={form.credit} onChange={(e) => set("credit", e.target.value)} placeholder="0" />
          </Field>
          {form.currency !== reporting ? (
            <Field label={`Exchange rate to ${reporting}`} hint="At the bill's date. Left out, Token Controller fills it in." className="sm:col-span-2">
              <Input inputMode="decimal" value={form.exchange_rate} onChange={(e) => set("exchange_rate", e.target.value)} placeholder="0.9214" />
            </Field>
          ) : null}
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

function UploadCsv({ open, onClose, period }: { open: boolean; onClose: () => void; period: string }) {
  const [text, setText] = useState("");
  const [cloud, setCloud] = useState<BillCloud | "">("");
  const [exportPeriod, setExportPeriod] = useState(period);
  const query: BillCsvQuery = cloud ? { cloud, period: exportPeriod } : {};
  const preview = useMutation({ mutationFn: () => api.previewBillCsv(text, query) });
  const save = useMutation({
    mutationFn: () => api.importBillCsv(text, query),
    onSuccess: async (uploaded) => {
      await refreshAll();
      const count = uploaded.bills.length;
      const same = count - uploaded.changed;
      notify(`${count} ${count === 1 ? "bill" : "bills"} uploaded`, same ? `${same} already in with the same figures.` : undefined);
      setText("");
      preview.reset();
      onClose();
    },
  });
  const changeFile = (next: string) => {
    setText(next);
    preview.reset();
  };
  const read = preview.data;
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()} title="Upload a bill as CSV" description="Give the period, gross, discount, credit, net and currency, or upload the cloud's own export. The upload shows what it read before anything is saved." wide>
      <div className="space-y-4">
        <div className="grid gap-3 sm:grid-cols-2">
          <Field label="File">
            <Select value={cloud} onChange={(e) => { setCloud(e.target.value as BillCloud | ""); preview.reset(); }}>
              <option value="">One bill per row, in the bill schema</option>
              {billClouds.map((c) => (
                <option key={c.value} value={c.value}>
                  {c.name}
                </option>
              ))}
            </Select>
          </Field>
          {cloud ? (
            <Field label="The export is the bill for">
              <Input type="month" required value={exportPeriod} onChange={(e) => { setExportPeriod(e.target.value); preview.reset(); }} />
            </Field>
          ) : null}
        </div>
        <Field label="CSV file">
          <Input type="file" accept=".csv,text/csv" className="pt-1.5" onChange={(e) => void e.target.files?.[0]?.text().then(changeFile)} />
        </Field>
        <Field label="Or paste it">
          <Textarea rows={4} className="font-mono text-xs" value={text} onChange={(e) => changeFile(e.target.value)} placeholder={cloud ? "" : "period,cost_source,gross,discount,credit,net,currency,provider\n2026-09,reseller,612.40,0,0,612.40,USD,cursor"} />
        </Field>
        {read ? (
          <div className="rounded-md border border-line p-3 text-sm">
            {read.problem ? (
              <p className="text-exception">
                {read.problem.row != null ? `Row ${read.problem.row}${read.problem.column ? `, column ${read.problem.column}` : ""}: ` : read.problem.column ? `Column ${read.problem.column}: ` : ""}
                {read.problem.message}
              </p>
            ) : (
              <ul className="space-y-1">
                {read.bills.map((b) => (
                  <li key={`${b.provider}:${b.cost_source}:${b.period}`} className="flex flex-wrap justify-between gap-x-4">
                    <span>
                      {providerName(b.provider)} · {costSourceName(b.cost_source)} · {formatPeriod(b.period)}
                      {b.lines ? <span className="text-muted"> · from {b.lines} lines</span> : null}
                    </span>
                    <span>
                      net <MoneyText value={fromDecimal(b.net, b.currency)} />
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </div>
        ) : null}
        <FormError error={preview.error ?? save.error} />
        <div className="flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          {read && !read.problem && read.bills.length ? (
            <Button variant="primary" onClick={() => save.mutate()} disabled={save.isPending}>
              Upload {read.bills.length} {read.bills.length === 1 ? "bill" : "bills"}
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

/** Money the service sends as a decimal string, in the currency named next to it. */
const money = (amount: string, currency: string) => <MoneyText value={fromDecimal(amount, currency)} />;

/** Token entries plus residual per bill, in the bill's currency, and the organization in the reporting currency. */
export function ReconciliationTable({ data }: { data: Reconciliation }) {
  const c = data.reporting_currency;
  const totals = data.totals;
  const explained = fromDecimal(totals.token_entries, c);
  const gross = sum([explained, fromDecimal(totals.residual, c)], c);
  const columns: Column<BillCheck>[] = [
    {
      key: "bill",
      header: "Bill",
      cell: (r) => (
        <span>
          <span className="font-medium">{providerName(r.bill.provider)}</span> <span className="text-xs text-muted">{costSourceName(r.bill.cost_source)}</span>
        </span>
      ),
      csv: (r) => `${r.bill.provider} ${r.bill.cost_source}`,
    },
    { key: "gross", header: "Gross", align: "right", cell: (r) => money(r.gross, r.currency), csv: (r) => r.gross },
    { key: "entries", header: "Token entries", align: "right", cell: (r) => money(r.token_entries, r.currency), csv: (r) => r.token_entries },
    { key: "residual", header: "Residual", align: "right", cell: (r) => money(r.residual, r.currency), csv: (r) => r.residual },
    { key: "attribution", header: "Attribution", align: "right", wide: true, cell: (r) => (r.attribution_percent == null ? "" : `${r.attribution_percent}%`), csv: (r) => r.attribution_percent ?? "" },
    { key: "net", header: "Net", align: "right", wide: true, cell: (r) => money(r.net, r.currency), csv: (r) => r.net },
  ];
  return (
    <DataTable
      stack
      columns={columns}
      rows={data.bills}
      rowKey={(r) => r.bill.id}
      csvName={`reconciliation-${data.period}.csv`}
      caption="Reconciliation"
      empty="No bills for this period yet."
      totals={{
        label: "Organization",
        values: {
          gross: <MoneyText value={gross} />,
          entries: <MoneyText value={explained} />,
          residual: money(totals.residual, c),
          attribution: Number(gross.nanos) > 0 ? formatShare(shareOf(explained, gross)) : "",
          net: money(totals.net, c),
        },
        csv: { gross: toDecimal(gross), entries: totals.token_entries, residual: totals.residual, net: totals.net },
      }}
    />
  );
}

/** What the reconciliation flags in plain words: providers without a bill, unknown models, bills restated after the close. */
export function ReconciliationNotes({ data }: { data: Reconciliation }) {
  const c = data.reporting_currency;
  const unknown = data.bills.filter((b) => b.unknown_model_count > 0);
  return (
    <>
      {data.without_bill.map((w) => (
        <Note key={w.provider} tone={data.metered ? "exception" : "accent"}>
          {data.metered ? (
            <>
              No bill from <b>{providerName(w.provider)}</b> yet: {w.token_entry_count} token {w.token_entry_count === 1 ? "entry" : "entries"}, {formatMoney(fromDecimal(w.token_entries, c))} at list value, are on no bill.
            </>
          ) : (
            <>
              {providerName(w.provider)}: {w.token_entry_count} token {w.token_entry_count === 1 ? "entry" : "entries"}, {formatMoney(fromDecimal(w.token_entries, c))} at list value. On a seat plan or subscription there is no bill, so this list value is an estimate.
            </>
          )}
          {w.unknown_model_count ? ` ${w.unknown_model_count} of them use a model missing from the price table.` : null}
        </Note>
      ))}
      {unknown.map((b) => (
        <Note key={`unknown-${b.bill.id}`}>
          {b.unknown_model_count} token {b.unknown_model_count === 1 ? "entry" : "entries"} on the bill from {providerName(b.bill.provider)} {b.unknown_model_count === 1 ? "uses" : "use"} a model missing from the price table. Their cost is unknown, so it sits in residual.
        </Note>
      ))}
      {data.restated_after_close.map((b) => (
        <Note key={`restated-${b.id}`} tone="exception">
          {providerName(b.provider)} restated its bill after the close: revision {b.revision}, net {formatMoney(fromDecimal(b.net, b.currency))}. The closed figures stay as they were; post an adjustment for the difference.
        </Note>
      ))}
    </>
  );
}

function Revisions({ bill, onClose }: { bill: BillView | null; onClose: () => void }) {
  const revisions = useQuery({ queryKey: ["bill-revisions", bill?.id], queryFn: () => api.billRevisions(bill?.id ?? ""), enabled: Boolean(bill) });
  return (
    <Dialog open={Boolean(bill)} onOpenChange={(o) => !o && onClose()} title={`Bill from ${bill ? providerName(bill.provider) : ""}, ${bill ? formatPeriod(bill.period) : ""}`} description="A provider can restate a bill. Each revision is kept.">
      {revisions.isPending ? <Loading /> : revisions.isError ? <ErrorBox error={revisions.error} /> : (
        <ul className="divide-y divide-line text-sm">
          {[...revisions.data].reverse().map((r) => (
            <li key={r.revision} className="flex justify-between gap-4 py-2">
              <span>
                Revision {r.revision} · {formatDate(r.recorded_at)} · {billSourceNames[r.source]}
                {r.revision === bill?.revision ? <span className="text-muted"> · latest</span> : null}
              </span>
              {money(r.net, r.currency)}
            </li>
          ))}
        </ul>
      )}
    </Dialog>
  );
}

function Connections() {
  const connections = useQuery({ queryKey: ["bill-connections"], queryFn: api.billConnections });
  const [leaving, setLeaving] = useState<BillConnectionView | null>(null);
  const pull = useMutation({
    mutationFn: (c: BillConnectionView) => api.pullBills(c.id),
    onSuccess: (c) => notify(`Pulling ${billApiName(c.provider)}`, "The bills update when the pull is done."),
  });
  const disconnect = useMutation({
    mutationFn: (c: BillConnectionView) => api.disconnectBill(c.id),
    onSuccess: async () => {
      await refreshAll();
      notify("Disconnected", "Its bills stay. They stop updating.");
      setLeaving(null);
    },
  });
  if (connections.isPending) return <Loading />;
  if (connections.isError) return <ErrorBox error={connections.error} />;
  return (
    <>
      {connections.data.length ? (
        <ul className="divide-y divide-line rounded-lg border border-line">
          {connections.data.map((c) => (
            <li key={c.id} className="flex flex-col gap-2 px-4 py-3 sm:flex-row sm:items-center sm:justify-between">
              <div className="min-w-0">
                <div className="text-sm font-medium">
                  {billApiName(c.provider)} <span className="font-mono text-xs font-normal text-muted">key {c.key_hint}</span>
                </div>
                <div className={`text-xs ${c.status === "failing" ? "text-exception" : "text-muted"}`}>
                  {c.status === "failing" ? `The last pull failed: ${c.message ?? "check the key"}` : c.last_pull_at ? `Pulled ${formatDateTime(c.last_pull_at)}` : "Not pulled yet"}
                </div>
              </div>
              <div className="flex gap-2">
                <Button size="sm" onClick={() => pull.mutate(c)} disabled={pull.isPending}>
                  Pull now
                </Button>
                <Button size="sm" variant="ghost" onClick={() => setLeaving(c)}>
                  Disconnect
                </Button>
              </div>
            </li>
          ))}
        </ul>
      ) : (
        <p className="text-sm text-muted">No provider connected. Bills from a provider without an API go in as CSV or typed in.</p>
      )}
      <FormError error={pull.error} />
      <Dialog open={Boolean(leaving)} onOpenChange={(o) => !o && setLeaving(null)} title={`Disconnect ${leaving ? billApiName(leaving.provider) : ""}?`} description="Its bills stay. They stop updating.">
        <FormError error={disconnect.error} />
        <div className="mt-5 flex justify-end gap-2">
          <Button onClick={() => setLeaving(null)}>Cancel</Button>
          <Button variant="danger" onClick={() => leaving && disconnect.mutate(leaving)} disabled={disconnect.isPending}>
            Disconnect
          </Button>
        </div>
      </Dialog>
    </>
  );
}

export function BillsPage(): ReactNode {
  const me = useMe();
  const navigate = billsRoute.useNavigate();
  const search = billsRoute.useSearch();
  const period = search.period ?? addMonths(thisPeriod(), -1);
  const bills = useQuery({ queryKey: ["bills", period], queryFn: () => api.bills(period) });
  const recon = useQuery({ queryKey: ["reconciliation", period], queryFn: () => api.reconciliation(period) });
  const [dialog, setDialog] = useState<"" | "connect" | "type" | "csv">("");
  const [revisions, setRevisions] = useState<BillView | null>(null);
  const source = describeCostSource(me.organization.cost_source);
  const reporting = me.organization.reporting_currency;
  const columns: Column<BillView>[] = [
    {
      key: "provider",
      header: "Provider",
      cell: (b) => (
        <span>
          <span className="font-medium">{providerName(b.provider)}</span> <span className="text-xs text-muted">{costSourceName(b.cost_source)}</span>
        </span>
      ),
      csv: (b) => b.provider,
    },
    { key: "cost_source", header: "Cost source", csvOnly: true, cell: (b) => b.cost_source, csv: (b) => b.cost_source },
    { key: "source", header: "From", wide: true, cell: (b) => <Badge>{billSourceNames[b.source]}</Badge>, csv: (b) => b.source },
    { key: "recorded", header: "Recorded", wide: true, cell: (b) => formatDate(b.recorded_at), csv: (b) => b.recorded_at },
    {
      key: "revision",
      header: "Revision",
      cell: (b) => (
        <span className="inline-flex flex-wrap items-center gap-2">
          {b.revision > 1 ? (
            <button type="button" className="text-accent hover:underline" onClick={() => setRevisions(b)}>
              {b.revision}, restated
            </button>
          ) : (
            b.revision
          )}
          {b.final ? null : <Badge tone="exception">not final</Badge>}
        </span>
      ),
      csv: (b) => b.revision,
    },
    { key: "final", header: "Final", csvOnly: true, cell: (b) => (b.final ? "yes" : "no"), csv: (b) => (b.final ? "yes" : "no") },
    { key: "gross", header: "Gross", align: "right", wide: true, cell: (b) => money(b.gross, b.currency), csv: (b) => b.gross },
    { key: "discount", header: "Discount", align: "right", wide: true, cell: (b) => money(b.discount, b.currency), csv: (b) => b.discount },
    { key: "credit", header: "Credit", align: "right", wide: true, cell: (b) => money(b.credit, b.currency), csv: (b) => b.credit },
    { key: "net", header: "Net", align: "right", cell: (b) => money(b.net, b.currency), csv: (b) => b.net },
    { key: "currency", header: "Currency", csvOnly: true, cell: (b) => b.currency, csv: (b) => b.currency },
    {
      key: "rate",
      header: `Rate to ${reporting}`,
      align: "right",
      wide: "xl",
      cell: (b) => (b.currency === reporting ? "" : `${b.exchange_rate ?? "not known yet"}${b.exchange_rate && !b.exchange_rate_given ? " (reference)" : ""}`),
      csv: (b) => b.exchange_rate ?? "",
    },
  ];
  const r = recon.data;
  const totals = r ? { explained: fromDecimal(r.totals.token_entries, r.reporting_currency), residual: fromDecimal(r.totals.residual, r.reporting_currency), net: fromDecimal(r.totals.net, r.reporting_currency) } : null;
  const gross = totals && r ? sum([totals.explained, totals.residual], r.reporting_currency) : null;
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
      {r && totals && gross ? (
        <KpiRow>
          <Kpi label="Bills, net" value={formatMoney(totals.net)} note={`${r.bills.length} ${r.bills.length === 1 ? "bill" : "bills"}`} />
          <Kpi label="Token entries" value={formatMoney(totals.explained)} note="on the bills, at list value" />
          <Kpi label="Residual" value={formatMoney(totals.residual)} note="no token entry explains it" tone={Number(totals.residual.nanos) ? "exception" : undefined} />
          {r.metered ? (
            <Kpi label="Attribution" value={Number(gross.nanos) > 0 ? formatShare(shareOf(totals.explained, gross)) : "none yet"} note="share of the bills that token entries explain" tone="posted" />
          ) : (
            <Kpi label="Estimates" value={formatMoney(fromDecimal(r.totals.estimates, r.reporting_currency))} note="list value of token entries on no bill" />
          )}
        </KpiRow>
      ) : null}
      <Section title={`Bills for ${formatPeriod(period)}`} lead={r?.closed ? "This period is closed. It keeps the bill figures it closed with." : undefined}>
        {bills.isPending ? <Loading /> : bills.isError ? <ErrorBox error={bills.error} /> : (
          <DataTable stack columns={columns} rows={bills.data} rowKey={(b) => b.id} csvName={`bills-${period}.csv`} caption="Bills" empty="No bills for this period yet. Connect a provider, upload the bill as CSV or type it in." />
        )}
      </Section>
      <Section title="Reconciliation" lead="Per bill: token entries plus residual equal the bill. Tags play no part in this check.">
        {r ? (
          <>
            <ReconciliationTable data={r} />
            <div className="mt-3">
              <ReconciliationNotes data={r} />
            </div>
          </>
        ) : recon.isError ? <ErrorBox error={recon.error} /> : <Loading />}
      </Section>
      <Section title="Connected providers" lead="Pulled once a day, for the current and the previous period. Pull now pulls at once.">
        <Connections />
      </Section>
      <Connect open={dialog === "connect"} onClose={() => setDialog("")} />
      {dialog === "type" ? <TypeIn open onClose={() => setDialog("")} period={period} /> : null}
      {dialog === "csv" ? <UploadCsv open onClose={() => setDialog("")} period={period} /> : null}
      <Revisions bill={revisions} onClose={() => setRevisions(null)} />
    </>
  );
}
