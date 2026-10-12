import { useMutation, useQuery } from "@tanstack/react-query";
import { getRouteApi, Link, useNavigate } from "@tanstack/react-router";
import { useState, type ReactNode } from "react";
import type { Period } from "@/api/contract";
import { api } from "@/api/endpoints";
import { ReconciliationTable } from "@/routes/bills";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Loading, MoneyText, PageHeader, Section } from "@/components/ui/display";
import { FormError } from "@/components/ui/form";
import { DataTable, type Column } from "@/components/ui/table";
import { notify } from "@/components/ui/toast";
import { formatDate, formatDateTime, formatDay, formatPeriod } from "@/lib/format";
import { formatMoney } from "@/lib/money";
import { refreshAll, usePeriods, usePersonName } from "@/lib/queries";
import { thisPeriod } from "@/lib/today";

const closeRoute = getRouteApi("/app/close");

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
  const check = useQuery({ queryKey: ["close-check", period], queryFn: () => api.closeCheck(period) });
  const name = usePersonName();
  const navigate = useNavigate();
  const [confirm, setConfirm] = useState(false);
  const remind = useMutation({ mutationFn: () => api.askToSubmit(period), onSuccess: (r) => notify(`Reminder sent to ${r.reminded} ${r.reminded === 1 ? "person" : "people"}`) });
  const close = useMutation({
    mutationFn: () => api.close(period),
    onSuccess: async () => {
      await refreshAll();
      notify(`${formatPeriod(period)} is closed`, "The cost sheet is ready for finance.");
      setConfirm(false);
      await navigate({ to: "/close/$period", params: { period } });
    },
  });
  if (check.isPending) return <Loading />;
  if (check.isError) return <ErrorBox error={check.error} />;
  const c = check.data;
  const billsIn = c.bills.every((b) => b.received);
  const nothingOpen = c.late_sheets.length === 0 && (!c.approval || c.unapproved === 0);
  return (
    <>
      <ol className="mb-6 rounded-lg border border-line px-4">
        <Step n={1} title="Check the bill" done={billsIn}>
          <ul className="flex flex-wrap gap-2">
            {c.bills.map((b) => (
              <li key={b.label}>
                <Badge tone={b.received ? (b.final ? "posted" : "accent") : "exception"}>
                  {b.label}: {b.received ? (b.final ? "in, final" : "in, not final yet") : "missing"}
                </Badge>
              </li>
            ))}
          </ul>
          {!billsIn ? (
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
              Organization overhead: <MoneyText value={c.organization_overhead.amount} /> in {c.organization_overhead.entries} token entries.{" "}
              <Link to="/token-entries" search={{ period }} className="text-accent hover:underline">
                Check for cost that belongs on a project
              </Link>
            </li>
            <li>
              {c.late_sheets.length ? (
                <>
                  Late token sheets:{" "}
                  {c.late_sheets.map((s) => `${name(s.person)} (${formatDay(s.starts)} to ${formatDay(s.ends)})`).join(", ")}.{" "}
                  <button type="button" onClick={() => remind.mutate()} className="text-accent hover:underline">
                    Ask everyone to submit
                  </button>
                </>
              ) : (
                "No late token sheets."
              )}
            </li>
            {c.approval ? (
              <li>
                {c.unapproved ? (
                  <>
                    {c.unapproved} token {c.unapproved === 1 ? "entry is" : "entries are"} not approved.{" "}
                    <Link to="/approval" search={{ period }} className="text-accent hover:underline">
                      Approve them
                    </Link>
                  </>
                ) : (
                  "Every token entry is approved."
                )}
              </li>
            ) : null}
          </ul>
        </Step>
        <Step n={3} title="Send back what is wrong" done={nothingOpen}>
          Open a token entry and reject it to its agent manager with a reason. A sheet submitted after the close becomes an adjustment, marked late.
        </Step>
        <Step n={4} title="Close" done={false}>
          {c.can_close ? (
            <p>Token entries and residual add up to the bills. Closing locks the period, its tags, the tag map and the price table.</p>
          ) : (
            <ul className="list-disc pl-4 text-exception">
              {c.blocked_by.map((b) => (
                <li key={b}>{b}</li>
              ))}
            </ul>
          )}
          <Button variant="primary" className="mt-3" disabled={!c.can_close} onClick={() => setConfirm(true)}>
            Close {formatPeriod(period)}
          </Button>
        </Step>
      </ol>
      <Section title="Reconciliation">
        <ReconciliationTable data={c.reconciliation} period={period} />
      </Section>
      <Dialog open={confirm} onOpenChange={setConfirm} title={`Close ${formatPeriod(period)}?`} description="A closed period does not change. Corrections after the close are adjustments.">
        <p className="text-sm">
          Bills <b>{formatMoney(c.reconciliation.total.bill_net)}</b> = token entries <b>{formatMoney(c.reconciliation.total.token_entries)}</b> + residual <b>{formatMoney(c.reconciliation.total.residual)}</b>.
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
  const name = usePersonName();
  const navigate = closeRoute.useNavigate();
  const search = closeRoute.useSearch();
  const list = periods.data ?? [];
  const firstOpenOver = [...list].reverse().find((p) => p.status === "open" && p.id < thisPeriod());
  const period = search.period ?? firstOpenOver?.id ?? thisPeriod();
  const selected = list.find((p) => p.id === period);
  const columns: Column<Period>[] = [
    { key: "period", header: "Period", cell: (p) => <span className="font-medium">{formatPeriod(p.id)}</span>, csv: (p) => p.id },
    { key: "status", header: "Status", cell: (p) => (p.status === "closed" ? <Badge tone="posted">closed</Badge> : <Badge>open</Badge>), csv: (p) => p.status },
    { key: "closed", header: "Closed", wide: true, cell: (p) => (p.closed_at ? `${formatDate(p.closed_at)} by ${name(p.closed_by)}` : ""), csv: (p) => p.closed_at ?? "" },
    { key: "total", header: "Token entries", align: "right", cell: (p) => <MoneyText value={p.total} />, csv: (p) => formatMoney(p.total) },
    {
      key: "open",
      header: "",
      cell: (p) =>
        p.status === "closed" ? (
          <Link to="/close/$period" params={{ period: p.id }} className="text-sm text-accent hover:underline" onClick={(e) => e.stopPropagation()}>
            Cost sheet
          </Link>
        ) : null,
    },
  ];
  return (
    <>
      <PageHeader title="Monthly close" lead="Close a period once its bills are final. The close locks its token entries and tags and produces the cost sheet." />
      {selected?.status === "open" ? (
        <Section title={`Close ${formatPeriod(period)}`}>
          <CloseChecklist period={period} />
        </Section>
      ) : selected?.status === "closed" ? (
        <Section title={formatPeriod(period)}>
          <p className="text-sm">
            Closed {selected.closed_at ? formatDateTime(selected.closed_at) : ""} by {name(selected.closed_by)}.{" "}
            <Link to="/close/$period" params={{ period }} className="text-accent hover:underline">
              Open the cost sheet
            </Link>
          </p>
        </Section>
      ) : null}
      <Section title="Periods">
        {periods.isPending ? <Loading /> : (
          <DataTable stack columns={columns} rows={list} rowKey={(p) => p.id} onRowClick={(p) => void navigate({ search: { period: p.id } })} csvName="periods.csv" caption="Periods" rowClassName={(p) => (p.id === period ? "bg-accent-tint" : "")} />
        )}
      </Section>
    </>
  );
}
