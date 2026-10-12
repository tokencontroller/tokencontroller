import { useMutation, useQuery } from "@tanstack/react-query";
import { getRouteApi, Link, useNavigate } from "@tanstack/react-router";
import { useState, type ReactNode } from "react";
import type { PeriodSummary } from "@/api/contract";
import { api } from "@/api/endpoints";
import { ReconciliationNotes, ReconciliationTable } from "@/routes/bills";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Loading, MoneyText, PageHeader, Section } from "@/components/ui/display";
import { FormError } from "@/components/ui/form";
import { DataTable, type Column } from "@/components/ui/table";
import { notify } from "@/components/ui/toast";
import { providerName } from "@/lib/bills";
import { formatDate, formatDateTime, formatDay, formatPeriod } from "@/lib/format";
import { formatMoney, fromDecimal } from "@/lib/money";
import { refreshAll, usePeriods } from "@/lib/queries";
import { thisPeriod } from "@/lib/today";

const closeRoute = getRouteApi("/app/close");

const entries = (n: number) => `${n} token ${n === 1 ? "entry" : "entries"}`;

function Step({ n, title, done, children }: { n: number; title: string; done: boolean; children: ReactNode }) {
  return (
    <li className="flex gap-4 border-b border-line py-4 last:border-0">
      <span aria-hidden="true" className={`flex size-7 shrink-0 items-center justify-center rounded-full text-[13px] font-semibold ${done ? "bg-posted text-on-ink" : "bg-sand text-ink-soft"}`}>
        {done ? "✓" : n}
      </span>
      <div className="min-w-0 flex-1">
        <h3 className="text-sm font-semibold">
          {title} <span className="sr-only">{done ? "(done)" : "(open)"}</span>
        </h3>
        <div className="mt-1 text-sm text-ink-soft">{children}</div>
      </div>
    </li>
  );
}

function CloseChecklist({ period }: { period: string }) {
  const navigate = useNavigate();
  const view = useQuery({ queryKey: ["period", period], queryFn: () => api.period(period) });
  const recon = useQuery({ queryKey: ["reconciliation", period], queryFn: () => api.reconciliation(period) });
  const late = useQuery({ queryKey: ["late-sheets", period], queryFn: () => api.lateSheets(period) });
  const [confirm, setConfirm] = useState(false);
  const remind = useMutation({
    mutationFn: () => api.askToSubmit(period),
    onSuccess: (r) => notify(`Asked ${r.reminded} ${r.reminded === 1 ? "person" : "people"} to submit`, "The request shows until the period is closed."),
  });
  const close = useMutation({
    mutationFn: () => api.close(period),
    onSuccess: async () => {
      await refreshAll();
      notify(`${formatPeriod(period)} is closed`, "The cost sheet is ready for finance.");
      setConfirm(false);
      await navigate({ to: "/close/$period", params: { period } });
    },
  });
  if (view.isPending || recon.isPending) return <Loading />;
  if (view.isError) return <ErrorBox error={view.error} />;
  if (recon.isError) return <ErrorBox error={recon.error} />;
  const v = view.data;
  const r = recon.data;
  const open = v.open_items;
  const lateSheets = late.data ?? [];
  const billsIn = !r.metered || (r.bills.length > 0 && open.providers_without_bill.length === 0 && open.bills_not_final.length === 0);
  const nothingOpen = lateSheets.length === 0 && open.unapproved === 0 && open.without_agent_manager === 0;
  const totals = r.totals;
  const m = (amount: string) => formatMoney(fromDecimal(amount, r.reporting_currency));
  return (
    <>
      <ol className="mb-6 rounded-lg border border-line px-4">
        <Step n={1} title="Check the bill" done={billsIn}>
          {r.metered ? (
            <ul className="flex flex-wrap gap-2">
              {r.bills.map((b) => (
                <li key={b.bill.id}>
                  {b.bill.final ? (
                    <Badge tone="posted">{providerName(b.bill.provider)}: in, revision {b.bill.revision}</Badge>
                  ) : (
                    <Badge tone="exception">{providerName(b.bill.provider)}: in, not final yet</Badge>
                  )}
                </li>
              ))}
              {open.providers_without_bill.map((p) => (
                <li key={p}>
                  <Badge tone="exception">{providerName(p)}: no bill yet</Badge>
                </li>
              ))}
              {!r.bills.length && !open.providers_without_bill.length ? <li>No bills for this period yet.</li> : null}
            </ul>
          ) : (
            <p>On a seat plan or subscription there is no bill to check. Every list value is an estimate.</p>
          )}
          {r.metered && !billsIn ? (
            <p className="mt-2">
              <Link to="/bills" search={{ period }} className="text-accent hover:underline">
                Add the missing bills
              </Link>
            </p>
          ) : null}
        </Step>
        <Step n={2} title="Look at what is open" done={nothingOpen}>
          <ul className="space-y-1.5">
            <li>
              Organization overhead: <MoneyText value={fromDecimal(open.organization_overhead, open.currency)} /> in {entries(open.organization_overhead_entries)}.{" "}
              <Link to="/token-entries" search={{ period }} className="text-accent hover:underline">
                Check for cost that belongs on a project
              </Link>
            </li>
            <li>
              {lateSheets.length
                ? `Late token sheets: ${lateSheets.map((s) => `${s.person} (${formatDay(s.period_start)} to ${formatDay(s.period_end)})`).join(", ")}.`
                : late.isPending
                  ? "Late token sheets: …"
                  : "No late token sheets."}{" "}
              <button type="button" onClick={() => remind.mutate()} disabled={remind.isPending} className="text-accent hover:underline">
                Ask everyone to submit
              </button>
              <FormError error={remind.error} />
            </li>
            {v.approval ? (
              <li>
                {open.unapproved ? (
                  <>
                    {entries(open.unapproved)} {open.unapproved === 1 ? "is" : "are"} not approved.{" "}
                    <Link to="/approval" search={{ period }} className="text-accent hover:underline">
                      Approve them
                    </Link>
                  </>
                ) : (
                  "Every token entry is approved."
                )}
              </li>
            ) : null}
            {open.rejected ? (
              <li>
                {entries(open.rejected)} {open.rejected === 1 ? "was" : "were"} sent back and {open.rejected === 1 ? "waits" : "wait"} for {open.rejected === 1 ? "its" : "their"} agent manager.
              </li>
            ) : null}
            {open.without_agent_manager ? (
              <li>
                {entries(open.without_agent_manager)} {open.without_agent_manager === 1 ? "has" : "have"} no agent manager.{" "}
                <Link to="/token-entries" search={{ period }} className="text-accent hover:underline">
                  Take {open.without_agent_manager === 1 ? "it" : "them"} over
                </Link>
              </li>
            ) : null}
            {open.unknown_model ? (
              <li>
                {entries(open.unknown_model)} {open.unknown_model === 1 ? "uses" : "use"} a model missing from the price table. Their cost is unknown, so it sits in residual.
              </li>
            ) : null}
          </ul>
        </Step>
        <Step n={3} title="Send back what is wrong" done={nothingOpen}>
          Open a token entry and reject it to its agent manager with a reason. A token sheet submitted after the close becomes an adjustment to that month, marked late.
        </Step>
        <Step n={4} title="Close" done={false}>
          {v.can_close ? (
            <p>Token entries and residual add up to the bills. Closing locks the period: its token entries, their tags and the tag map it closed with.</p>
          ) : (
            <ul className="list-disc space-y-1 pl-4 text-exception">
              {v.blockers.map((b) => (
                <li key={b}>{b}</li>
              ))}
            </ul>
          )}
          <Button variant="primary" className="mt-3" disabled={!v.can_close} onClick={() => setConfirm(true)}>
            Close {formatPeriod(period)}
          </Button>
        </Step>
      </ol>
      <Section title="Reconciliation" lead="Per bill: token entries plus residual equal the bill. Residual is always shown.">
        <ReconciliationTable data={r} />
        <div className="mt-3">
          <ReconciliationNotes data={r} />
        </div>
      </Section>
      <Dialog open={confirm} onOpenChange={setConfirm} title={`Close ${formatPeriod(period)}?`} description="A closed period does not change. Corrections after the close are adjustments.">
        <p className="text-sm">
          Bills: token entries <b>{m(totals.token_entries)}</b> plus residual <b>{m(totals.residual)}</b>, minus discount <b>{m(totals.discount)}</b> and credit <b>{m(totals.credit)}</b>, come to <b>{m(totals.net)}</b>.
        </p>
        <FormError error={close.error} />
        <div className="mt-5 flex justify-end gap-2">
          <Button onClick={() => setConfirm(false)}>Cancel</Button>
          <Button variant="primary" onClick={() => close.mutate()} disabled={close.isPending}>
            Close the period
          </Button>
        </div>
      </Dialog>
    </>
  );
}

export function ClosePage(): ReactNode {
  const periods = usePeriods();
  const navigate = closeRoute.useNavigate();
  const search = closeRoute.useSearch();
  const list = periods.data ?? [];
  // The next close: the oldest period that is over and still open.
  const next = [...list].reverse().find((p) => p.ended && !p.closed) ?? list.find((p) => !p.closed);
  const period = search.period ?? next?.period ?? thisPeriod();
  const selected = list.find((p) => p.period === period);
  const columns: Column<PeriodSummary>[] = [
    { key: "period", header: "Period", cell: (p) => <span className="font-medium">{formatPeriod(p.period)}</span>, csv: (p) => p.period },
    {
      key: "status",
      header: "Status",
      cell: (p) => (p.closed ? <Badge tone="posted">closed</Badge> : p.ended ? <Badge>open</Badge> : <Badge>running</Badge>),
      csv: (p) => (p.closed ? "closed" : "open"),
    },
    { key: "closed", header: "Closed", wide: true, cell: (p) => (p.closed_at ? `${formatDate(p.closed_at)}${p.closed_by ? ` by ${p.closed_by}` : ""}` : ""), csv: (p) => p.closed_at ?? "" },
    {
      key: "open",
      header: "",
      cell: (p) =>
        p.closed ? (
          <Link to="/close/$period" params={{ period: p.period }} className="text-sm text-accent hover:underline" onClick={(e) => e.stopPropagation()}>
            Cost sheet
          </Link>
        ) : null,
    },
  ];
  return (
    <>
      <PageHeader title="Monthly close" lead="Close a period once its bills are final. The close locks its token entries and tags and produces the cost sheet." />
      {periods.isError ? <ErrorBox error={periods.error} /> : null}
      {selected && !selected.closed ? (
        <Section title={`Close ${formatPeriod(period)}`}>
          <CloseChecklist period={period} />
        </Section>
      ) : selected?.closed ? (
        <Section title={formatPeriod(period)}>
          <p className="text-sm">
            Closed {selected.closed_at ? formatDateTime(selected.closed_at) : ""}
            {selected.closed_by ? ` by ${selected.closed_by}` : ""}.{" "}
            <Link to="/close/$period" params={{ period }} className="text-accent hover:underline">
              Open the cost sheet
            </Link>
          </p>
        </Section>
      ) : null}
      <Section title="Periods">
        {periods.isPending ? <Loading /> : (
          <DataTable stack columns={columns} rows={list} rowKey={(p) => p.period} onRowClick={(p) => void navigate({ search: { period: p.period } })} csvName="periods.csv" caption="Periods" rowClassName={(p) => (p.period === period ? "bg-accent-tint" : "")} />
        )}
      </Section>
    </>
  );
}
