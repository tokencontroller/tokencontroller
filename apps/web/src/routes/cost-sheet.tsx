import { useMutation, useQuery } from "@tanstack/react-query";
import { getRouteApi, Link } from "@tanstack/react-router";
import { useState, type ReactNode } from "react";
import type { CostSheet, CostSheetLine } from "@/api/contract";
import { api } from "@/api/endpoints";
import { ReconciliationTable } from "@/routes/bills";
import { Badge, costKindLabel } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Kpi, KpiRow, Loading, MoneyText, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select, Textarea } from "@/components/ui/form";
import { Checkbox } from "@/components/ui/checkbox";
import { notify } from "@/components/ui/toast";
import { downloadFile } from "@/lib/csv";
import { formatDateTime, formatPeriod } from "@/lib/format";
import { formatMoney, parseAmount, shareOf, formatShare } from "@/lib/money";
import { refreshAll, useMe, usePersonName } from "@/lib/queries";
const sharedCostSheetRoute = getRouteApi("/shared/cost-sheet/$token");
const costSheetRoute = getRouteApi("/app/close/$period");

function Lines({ lines, depth = 0 }: { lines: CostSheetLine[]; depth?: number }) {
  return (
    <>
      {lines.map((line) => (
        <LineRows key={`${line.rung}:${line.value}:${line.label ?? ""}`} line={line} depth={depth} />
      ))}
    </>
  );
}

function LineRows({ line, depth }: { line: CostSheetLine; depth: number }) {
  const overhead = line.label?.endsWith("overhead") ?? false;
  return (
    <>
      <tr className={`border-b border-line ${depth === 0 ? "font-medium" : ""}`}>
        <td className="py-2 pl-4 pr-2 sm:pl-2" style={{ paddingLeft: `${(depth + 1) * 16}px` }}>
          {overhead ? (
            <span className="text-ink-soft">{line.label}</span>
          ) : (
            <>
              <span className={line.rung === "ticket" ? "font-mono text-[13px]" : ""}>{line.value}</span>
              {line.label ? <span className="ml-2 font-normal text-muted">{line.label}</span> : null}
            </>
          )}
        </td>
        <td className="hidden px-2 py-2 text-xs text-muted sm:table-cell">{overhead ? "" : line.rung}</td>
        <td className="num px-2 py-2 pr-4 sm:pr-2">
          <MoneyText value={line.amount} />
        </td>
      </tr>
      {line.children.length ? <Lines lines={line.children} depth={depth + 1} /> : null}
    </>
  );
}

export function CostSheetView({ sheet, organization, name }: { sheet: CostSheet; organization: string; name: (id: string) => string }) {
  const total = sheet.reconciliation.total.token_entries;
  return (
    <>
      <div className="mb-6 flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
        <div>
          <p className="text-sm text-muted">{organization}</p>
          <h1 className="text-[22px] font-semibold tracking-[-0.01em]">Cost sheet, {formatPeriod(sheet.period)}</h1>
          <p className="mt-1 text-sm text-muted">
            Ladder: {[...sheet.tag_map.ladder, "organization"].join(", ")} · cost center: {sheet.tag_map.cost_center}
            {sheet.approval ? " · approved cost only" : ""}
          </p>
        </div>
        {/* A functional CLOSED block: when, who, and the residual. No rubber stamp. */}
        <div className="shrink-0 rounded-md border border-posted/40 bg-posted-tint px-4 py-3 text-sm" data-testid="closed-block">
          <div className="font-mono text-xs font-medium tracking-wider text-posted">CLOSED</div>
          <div className="mt-1">{formatDateTime(sheet.closed_at)}</div>
          <div className="text-ink-soft">by {name(sheet.closed_by)}</div>
          <div className="text-ink-soft">
            residual <MoneyText value={sheet.reconciliation.total.residual} />
          </div>
        </div>
      </div>
      <KpiRow>
        {(Object.keys(sheet.by_kind) as (keyof CostSheet["by_kind"])[]).map((kind) => (
          <Kpi key={kind} label={costKindLabel[kind]} value={formatMoney(sheet.by_kind[kind])} note={`${formatShare(shareOf(sheet.by_kind[kind], total))} of token entries`} tone={kind === "direct" ? "posted" : undefined} />
        ))}
        <Kpi label="Bills" value={formatMoney(sheet.reconciliation.total.bill_net)} note={`attribution ${formatShare(sheet.reconciliation.attribution)}`} />
      </KpiRow>
      <Section title="Cost per project and ticket">
        <div className="-mx-4 overflow-x-auto sm:mx-0">
          <table className="w-full border-collapse text-sm">
            <thead>
              <tr className="border-b border-ink">
                <th scope="col" className="px-2 py-2 pl-4 text-left font-mono text-[11px] font-medium uppercase tracking-wide text-muted sm:pl-2">Cost object</th>
                <th scope="col" className="hidden px-2 py-2 text-left font-mono text-[11px] font-medium uppercase tracking-wide text-muted sm:table-cell">Rung</th>
                <th scope="col" className="px-2 py-2 pr-4 text-right font-mono text-[11px] font-medium uppercase tracking-wide text-muted sm:pr-2">Cost</th>
              </tr>
            </thead>
            <tbody>
              <Lines lines={sheet.lines} />
              {sheet.adjustments.map((a) => (
                <tr key={a.id} className="border-b border-line">
                  <td className="py-2 pl-4 pr-2 sm:pl-2">
                    <Badge tone={a.late_sheet ? "exception" : "accent"}>{a.late_sheet ? "adjustment, late" : "adjustment"}</Badge>{" "}
                    <span className="ml-1">{a.value}</span> <span className="text-muted">· {a.reason}</span>
                  </td>
                  <td className="hidden px-2 py-2 text-xs text-muted sm:table-cell">{a.rung}</td>
                  <td className="num px-2 py-2 pr-4 sm:pr-2"><MoneyText value={a.amount} signed /></td>
                </tr>
              ))}
              {sheet.reconciliation.bills.map((b) => (
                <tr key={b.bill} className="border-b border-line text-ink-soft">
                  <td className="py-2 pl-4 pr-2 sm:pl-2">Residual, {b.label}</td>
                  <td className="hidden px-2 py-2 text-xs text-muted sm:table-cell">bill</td>
                  <td className="num px-2 py-2 pr-4 sm:pr-2"><MoneyText value={b.residual} /></td>
                </tr>
              ))}
            </tbody>
            <tfoot className="border-t-[3px] border-double border-ink font-medium">
              <tr>
                <td className="py-2.5 pl-4 pr-2 sm:pl-2">Bills</td>
                <td className="hidden sm:table-cell" />
                <td className="num px-2 py-2.5 pr-4 sm:pr-2"><MoneyText value={sheet.reconciliation.total.bill_net} /></td>
              </tr>
            </tfoot>
          </table>
        </div>
        {sheet.adjustments.length ? <p className="mt-2 text-xs text-muted">Adjustments are posted after the close and are not part of the bill check.</p> : null}
      </Section>
      <Section title="Reconciliation">
        <ReconciliationTable data={sheet.reconciliation} period={sheet.period} />
      </Section>
    </>
  );
}

function PostAdjustment({ open, onClose, sheet }: { open: boolean; onClose: () => void; sheet: CostSheet }) {
  const me = useMe();
  const objects = sheet.lines.flatMap(function walk(line): { rung: string; value: string }[] {
    return [{ rung: line.rung, value: line.value }, ...line.children.filter((c) => !c.label?.endsWith("overhead")).flatMap(walk)];
  });
  const [target, setTarget] = useState(objects[0] ? `${objects[0].rung}:${objects[0].value}` : "organization:organization");
  const [amount, setAmount] = useState("");
  const [reason, setReason] = useState("");
  const post = useMutation({
    mutationFn: () => {
      const money = parseAmount(amount, me.organization.reporting_currency);
      if (!money) throw new Error("The amount is not a number. Use a minus sign for a reduction.");
      const [rung = "organization", value = "organization"] = target.split(":");
      return api.postAdjustment(sheet.period, { rung, value, amount: money, reason });
    },
    onSuccess: async () => {
      await refreshAll();
      notify("Adjustment posted");
      setAmount("");
      setReason("");
      onClose();
    },
  });
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()} title="Post an adjustment" description={`A signed correction to ${formatPeriod(sheet.period)}. It carries your name and the time; the token entries stay as they were.`}>
      <form className="space-y-4" onSubmit={(e) => { e.preventDefault(); post.mutate(); }}>
        <Field label="Cost object">
          <Select value={target} onChange={(e) => setTarget(e.target.value)}>
            {objects.map((o) => (
              <option key={`${o.rung}:${o.value}`} value={`${o.rung}:${o.value}`}>
                {o.rung}: {o.value}
              </option>
            ))}
            <option value="organization:organization">organization</option>
          </Select>
        </Field>
        <Field label={`Amount in ${me.organization.reporting_currency}`} hint="Plus or minus, such as -18.40.">
          <Input inputMode="decimal" required value={amount} onChange={(e) => setAmount(e.target.value)} />
        </Field>
        <Field label="Reason">
          <Textarea required value={reason} onChange={(e) => setReason(e.target.value)} />
        </Field>
        <FormError error={post.error} />
        <div className="flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={post.isPending}>
            Post adjustment
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

function ExportCsv({ open, onClose, period }: { open: boolean; onClose: () => void; period: string }) {
  const names = useQuery({ queryKey: ["tag-names"], queryFn: api.tagNames });
  const [columns, setColumns] = useState<string[]>([]);
  const download = useMutation({
    mutationFn: () => api.costSheetCsv(period, columns),
    onSuccess: (blob) => {
      downloadFile(`cost-sheet-${period}.csv`, blob);
      onClose();
    },
  });
  const extra = (names.data ?? []).filter((n) => !["ticket", "project"].includes(n.name));
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()} title="Download the cost sheet as CSV" description="Add any tag name as a column, such as team or purpose.">
      <ul className="space-y-2">
        {extra.map((n) => (
          <li key={n.name}>
            <label className="flex items-center gap-2 text-sm">
              <Checkbox label={n.name} checked={columns.includes(n.name)} onCheckedChange={(on) => setColumns(on ? [...columns, n.name] : columns.filter((c) => c !== n.name))} />
              {n.name}
            </label>
          </li>
        ))}
      </ul>
      <FormError error={download.error} />
      <div className="mt-5 flex justify-end gap-2">
        <Button onClick={onClose}>Cancel</Button>
        <Button variant="primary" onClick={() => download.mutate()} disabled={download.isPending}>
          Download CSV
        </Button>
      </div>
    </Dialog>
  );
}

export function CostSheetPage(): ReactNode {
  const { period } = costSheetRoute.useParams();
  const me = useMe();
  const name = usePersonName();
  const sheet = useQuery({ queryKey: ["cost-sheet", period], queryFn: () => api.costSheet(period) });
  const [dialog, setDialog] = useState<"" | "adjust" | "csv">("");
  const share = useMutation({
    mutationFn: () => api.shareCostSheet(period),
    onSuccess: async ({ url }) => {
      try {
        await navigator.clipboard.writeText(url);
        notify("Link copied", "Finance can open it without signing in.");
      } catch {
        notify("Link for finance", url);
      }
    },
  });
  if (sheet.isPending) return <Loading />;
  if (sheet.isError) return <ErrorBox error={sheet.error} />;
  return (
    <>
      <PageHeader
        title=""
        actions={
          <>
            <Link to="/close" className="mr-auto text-sm text-accent hover:underline sm:mr-2">
              All periods
            </Link>
            <Button onClick={() => share.mutate()}>Copy link for finance</Button>
            <Button onClick={() => setDialog("csv")}>Download CSV</Button>
            <Button variant="primary" onClick={() => setDialog("adjust")}>
              Post adjustment
            </Button>
          </>
        }
      />
      <CostSheetView sheet={sheet.data} organization={me.organization.name} name={(id) => name(id)} />
      <PostAdjustment open={dialog === "adjust"} onClose={() => setDialog("")} sheet={sheet.data} />
      <ExportCsv open={dialog === "csv"} onClose={() => setDialog("")} period={period} />
    </>
  );
}

/** The cost sheet as finance sees it from a link: read-only, no sign-in. */
export function SharedCostSheetPage(): ReactNode {
  const { token } = sharedCostSheetRoute.useParams();
  const sheet = useQuery({ queryKey: ["shared-cost-sheet", token], queryFn: () => api.sharedCostSheet(token) });
  return (
    <main className="mx-auto max-w-5xl px-4 py-8 sm:px-6">
      <div className="mb-6 flex items-center gap-2">
        <img src="/logo-light-small.svg" width="24" height="24" alt="" className="logo-light size-6" />
        <img src="/logo-dark-small.svg" width="24" height="24" alt="" className="logo-dark size-6" />
        <span className="text-[15px] font-semibold">Token Controller</span>
      </div>
      {sheet.isPending ? <Loading /> : sheet.isError ? <ErrorBox error={sheet.error} /> : <CostSheetView sheet={sheet.data} organization={sheet.data.organization} name={(id) => id} />}
    </main>
  );
}
