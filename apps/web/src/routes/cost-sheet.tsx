import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { getRouteApi, Link } from "@tanstack/react-router";
import { useState, type ReactNode } from "react";
import type { AdjustmentRow, BillBlock, CostKind, CostSheet, CostSheetLink, Money, Tags } from "@/api/contract";
import { ApiError } from "@/api/client";
import { api } from "@/api/endpoints";
import { ReconciliationNotes, ReconciliationTable } from "@/routes/bills";
import { Badge, costKindLabel } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Kpi, KpiRow, Loading, MoneyText, Note, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select, Textarea } from "@/components/ui/form";
import { Checkbox } from "@/components/ui/checkbox";
import { notify } from "@/components/ui/toast";
import { providerName } from "@/lib/bills";
import { downloadFile } from "@/lib/csv";
import { formatDate, formatDateTime, formatPeriod } from "@/lib/format";
import { formatShare, fromDecimal, money, nanosOf, parseAmount, sum, toDecimal } from "@/lib/money";
import { refreshAll, useMe, usePeriods, useTagNames } from "@/lib/queries";

const sharedCostSheetRoute = getRouteApi("/shared/cost-sheet/$token");
const costSheetRoute = getRouteApi("/app/close/$period");

const KINDS: CostKind[] = ["direct", "project_overhead", "organization_overhead"];
const kindOf = (kind: string): CostKind => KINDS.find((k) => k === kind) ?? "organization_overhead";

/** One cost object on the sheet: a rung's value, with the cost under it. */
interface CostNode {
  rung: string;
  value: string;
  /** Everything under it, in nano-units of the reporting currency. */
  total: bigint;
  /** What sits on it and on no narrower rung, per cost kind. */
  own: Map<CostKind, bigint>;
  /** The cost kinds whose cost here is partly on a model missing from the price table. */
  unknown: Set<CostKind>;
  children: Map<string, CostNode>;
}

const node = (rung: string, value: string): CostNode => ({ rung, value, total: 0n, own: new Map(), unknown: new Set(), children: new Map() });

/** Every bill's lines as one tree: the broadest rung first, such as project, then ticket. */
function costTree(sheet: CostSheet): CostNode {
  const root = node("", "");
  for (const block of sheet.blocks) {
    for (const line of block.lines) {
      const nanos = nanosOf(fromDecimal(line.amount, sheet.currency));
      let at = root;
      at.total += nanos;
      for (const rung of sheet.rungs) {
        const value = line.rungs[rung];
        if (!value) continue;
        const key = `${rung}\u0000${value}`;
        const next = at.children.get(key) ?? node(rung, value);
        at.children.set(key, next);
        at = next;
        at.total += nanos;
      }
      const kind = kindOf(line.cost_kind);
      at.own.set(kind, (at.own.get(kind) ?? 0n) + nanos);
      if (line.figure === "unknown_model") at.unknown.add(kind);
    }
  }
  return root;
}

const byValue = (a: CostNode, b: CostNode) => a.value.localeCompare(b.value);
const ownRows = (n: CostNode) => KINDS.flatMap((kind): [CostKind, bigint][] => (n.own.has(kind) ? [[kind, n.own.get(kind) ?? 0n]] : []));

const cell = "py-2 pr-2";
const indent = (depth: number) => ({ paddingLeft: `${16 + depth * 16}px` });

function UnknownModel() {
  return (
    <span className="ml-2 text-xs text-exception" title="Part of it is on a model missing from the price table. That part sits in residual.">
      unknown model
    </span>
  );
}

function CostRows({ at, depth, currency }: { at: CostNode; depth: number; currency: string }) {
  const children = [...at.children.values()].sort(byValue);
  const own = ownRows(at);
  // A ticket with only direct cost needs no line under it; anything else says its cost kind.
  const plain = children.length === 0 && own.every(([kind]) => kind === "direct");
  return (
    <>
      <tr className={`border-b border-line ${depth === 0 ? "font-medium" : ""}`}>
        <td className={cell} style={indent(depth)}>
          <span className={at.rung === "ticket" ? "font-mono text-[13px]" : ""}>{at.value}</span>
          {plain && at.unknown.size ? <UnknownModel /> : null}
        </td>
        <td className="hidden px-2 py-2 text-xs text-muted sm:table-cell">{at.rung}</td>
        <td className="num px-2 py-2 pr-4 sm:pr-2">
          <MoneyText value={money(at.total, currency)} />
        </td>
      </tr>
      {children.map((child) => (
        <CostRows key={`${child.rung}:${child.value}`} at={child} depth={depth + 1} currency={currency} />
      ))}
      {plain
        ? null
        : own.map(([kind, nanos]) => (
            <tr key={kind} className="border-b border-line text-ink-soft">
              <td className={cell} style={indent(depth + 1)}>
                {costKindLabel[kind]}
                {at.unknown.has(kind) ? <UnknownModel /> : null}
              </td>
              <td className="hidden px-2 py-2 text-xs text-muted sm:table-cell" />
              <td className="num px-2 py-2 pr-4 sm:pr-2">
                <MoneyText value={money(nanos, currency)} />
              </td>
            </tr>
          ))}
    </>
  );
}

/** The cost object an adjustment is on: its narrowest rung, or the organization. */
function costObject(tags: Tags, rungs: string[]): string {
  const rung = [...rungs].reverse().find((r) => tags[r]);
  return rung ? `${rung} ${tags[rung] ?? ""}` : "organization";
}

function SummaryRow({ label, value, strong = false, signed = false, unknown = false }: { label: ReactNode; value: Money; strong?: boolean; signed?: boolean; unknown?: boolean }) {
  return (
    <tr className={`border-b border-line ${strong ? "font-medium" : "text-ink-soft"}`}>
      <td className={cell} style={indent(0)}>
        {label}
        {unknown ? <UnknownModel /> : null}
      </td>
      <td className="hidden sm:table-cell" />
      <td className="num px-2 py-2 pr-4 sm:pr-2">
        <MoneyText value={value} signed={signed} />
      </td>
    </tr>
  );
}

function blockName(block: BillBlock, blocks: BillBlock[]): string {
  const name = providerName(block.provider);
  return blocks.filter((b) => b.provider === block.provider).length > 1 ? `${name} (${block.bill})` : name;
}

function AdjustmentLine({ row, rungs, currency }: { row: AdjustmentRow; rungs: string[]; currency: string }) {
  return (
    <tr className="border-b border-line">
      <td className={cell} style={indent(0)}>
        <Badge tone={row.late ? "exception" : "accent"}>{row.late ? "late adjustment" : "adjustment"}</Badge> <span className="ml-1">{costObject(row.tags, rungs)}</span>
        <span className="block text-xs text-muted">
          {row.reason} · {row.posted_by ? `${row.posted_by}, ` : ""}
          {formatDate(row.posted_at)}
        </span>
      </td>
      <td className="hidden px-2 py-2 text-xs text-muted sm:table-cell">{costKindLabel[kindOf(row.cost_kind)]}</td>
      <td className="num px-2 py-2 pr-4 sm:pr-2">
        <MoneyText value={fromDecimal(row.amount, currency)} signed />
      </td>
    </tr>
  );
}

export function CostSheetView({ sheet, organization, closedBy }: { sheet: CostSheet; organization?: string; closedBy?: string | null }) {
  const c = sheet.currency;
  const recon = sheet.reconciliation;
  const tree = costTree(sheet);
  const total = (pick: (b: BillBlock) => string) => sum(sheet.blocks.map((b) => fromDecimal(pick(b), c)), c);
  const discount = total((b) => b.discount);
  const credit = total((b) => b.credit);
  const negate = (m: Money) => money(-nanosOf(m), c);
  const share = (percent: number | null | undefined) => (percent == null ? "" : `${formatShare(percent * 100)} of token entries`);
  const head = "px-2 py-2 font-mono text-[11px] font-medium uppercase tracking-wide text-muted";
  return (
    <>
      <div className="mb-6 flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
        <div className="min-w-0">
          {organization ? <p className="text-sm text-muted">{organization}</p> : null}
          <h1 className="text-[22px] font-semibold tracking-[-0.01em]">Cost sheet, {formatPeriod(sheet.period)}</h1>
          <p className="mt-1 text-sm text-muted">
            Ladder: {[...sheet.rungs].reverse().concat("organization").join(", ")} · tag map version {sheet.tag_map_version}
            {sheet.approval ? " · approved cost only" : ""}
          </p>
        </div>
        {sheet.closed ? (
          // A plain CLOSED block: when, who, and the residual.
          <div className="shrink-0 rounded-md border border-posted/40 bg-posted-tint px-4 py-3 text-sm" data-testid="closed-block">
            <div className="font-mono text-xs font-medium tracking-wider text-posted">CLOSED</div>
            {sheet.closed_at ? <div className="mt-1">{formatDateTime(sheet.closed_at)}</div> : null}
            {closedBy ? <div className="text-ink-soft">by {closedBy}</div> : null}
            <div className="text-ink-soft">
              residual <MoneyText value={fromDecimal(recon.totals.residual, recon.reporting_currency)} />
            </div>
          </div>
        ) : null}
      </div>
      {sheet.closed ? null : (
        <Note>
          {formatPeriod(sheet.period)} is open, so this is a preview: its figures can still change. The cost sheet is final once the period is closed.
        </Note>
      )}
      <KpiRow>
        {KINDS.map((kind) => (
          <Kpi key={kind} label={costKindLabel[kind]} value={<MoneyText value={fromDecimal(sheet.kinds[kind], c)} />} note={share(sheet.kinds[`${kind}_percent`])} tone={kind === "direct" ? "posted" : undefined} />
        ))}
        <Kpi label="Total" value={<MoneyText value={fromDecimal(sheet.total, c)} />} note={sheet.adjustments.length ? "Bills net plus adjustments" : "Bills net"} />
      </KpiRow>
      <Section title="Cost per project and ticket" lead="By cost kind, with residual per bill as its own line. Adjustments posted after the close are their own lines.">
        <div className="-mx-4 overflow-x-auto sm:mx-0">
          <table className="w-full border-collapse text-sm">
            <caption className="sr-only">Cost per project and ticket</caption>
            <thead>
              <tr className="border-b border-ink">
                <th scope="col" className={`${head} pl-4 text-left`}>
                  Cost object
                </th>
                <th scope="col" className={`${head} hidden text-left sm:table-cell`}>
                  Rung
                </th>
                <th scope="col" className={`${head} pr-4 text-right sm:pr-2`}>
                  Cost
                </th>
              </tr>
            </thead>
            <tbody>
              {[...tree.children.values()].sort(byValue).map((child) => (
                <CostRows key={`${child.rung}:${child.value}`} at={child} depth={0} currency={c} />
              ))}
              {ownRows(tree).map(([kind, nanos]) => (
                <SummaryRow key={kind} label={costKindLabel[kind]} value={money(nanos, c)} strong unknown={tree.unknown.has(kind)} />
              ))}
              {sheet.blocks.map((b) => (
                <SummaryRow key={b.bill} label={`Residual, ${blockName(b, sheet.blocks)}`} value={fromDecimal(b.residual, c)} />
              ))}
              <SummaryRow label="Bills gross" value={total((b) => b.gross)} strong />
              {nanosOf(discount) ? <SummaryRow label="Discount" value={negate(discount)} /> : null}
              {nanosOf(credit) ? <SummaryRow label="Credit" value={negate(credit)} /> : null}
              <SummaryRow label="Bills net" value={total((b) => b.net)} strong />
              {sheet.adjustments.map((a) => (
                <AdjustmentLine key={a.id} row={a} rungs={sheet.rungs} currency={c} />
              ))}
            </tbody>
            <tfoot className="border-t-[3px] border-double border-ink font-medium">
              <tr>
                <td className="py-2.5 pr-2" style={indent(0)}>
                  Total
                </td>
                <td className="hidden sm:table-cell" />
                <td className="num px-2 py-2.5 pr-4 sm:pr-2">
                  <MoneyText value={fromDecimal(sheet.total, c)} />
                </td>
              </tr>
            </tfoot>
          </table>
        </div>
        {sheet.blocks.length ? null : <p className="mt-2 text-sm text-muted">No bills for this period.</p>}
      </Section>
      <Section title="Reconciliation" lead="Per bill: token entries plus residual equal the bill. Residual is always shown.">
        <ReconciliationTable data={recon} />
        <div className="mt-3">
          <ReconciliationNotes data={recon} />
        </div>
      </Section>
    </>
  );
}

/** The cost objects the sheet shows, as the tags an adjustment carries. */
function costObjects(sheet: CostSheet): { label: string; tags: Tags }[] {
  const seen = new Map<string, { label: string; tags: Tags }>();
  for (const line of sheet.blocks.flatMap((b) => b.lines)) {
    const tags: Tags = {};
    for (const rung of sheet.rungs) {
      const value = line.rungs[rung];
      if (!value) continue;
      tags[rung] = value;
      const label = sheet.rungs.filter((r) => tags[r]).map((r) => `${r} ${tags[r] ?? ""}`).join(" · ");
      if (!seen.has(label)) seen.set(label, { label, tags: { ...tags } });
    }
  }
  return [...seen.values()].sort((a, b) => a.label.localeCompare(b.label));
}

function PostAdjustment({ open, onClose, sheet }: { open: boolean; onClose: () => void; sheet: CostSheet }) {
  const objects = costObjects(sheet);
  const [target, setTarget] = useState("");
  const [amount, setAmount] = useState("");
  const [reason, setReason] = useState("");
  const post = useMutation({
    mutationFn: () => {
      const value = parseAmount(amount, sheet.currency);
      if (!value) throw new Error("The amount is not a number. Use a minus sign for a reduction.");
      const tags = objects.find((o) => o.label === target)?.tags ?? {};
      return api.postAdjustment(sheet.period, { amount: toDecimal(value), reason, tags });
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
      <form
        className="space-y-4"
        onSubmit={(e) => {
          e.preventDefault();
          post.mutate();
        }}
      >
        <Field label="Cost object">
          <Select value={target} onChange={(e) => setTarget(e.target.value)}>
            <option value="">organization</option>
            {objects.map((o) => (
              <option key={o.label} value={o.label}>
                {o.label}
              </option>
            ))}
          </Select>
        </Field>
        <Field label={`Amount in ${sheet.currency}`} hint="Plus or minus, such as -18.40.">
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

function FinanceLinks({ open, onClose, period }: { open: boolean; onClose: () => void; period: string }) {
  const client = useQueryClient();
  const links = useQuery({ queryKey: ["cost-sheet-links", period], queryFn: () => api.costSheetLinks(period), enabled: open });
  const [made, setMade] = useState<CostSheetLink | null>(null);
  const refresh = () => client.invalidateQueries({ queryKey: ["cost-sheet-links", period] });
  const make = useMutation({
    mutationFn: () => api.makeCostSheetLink(period),
    onSuccess: async (link) => {
      setMade(link);
      await refresh();
    },
  });
  const revoke = useMutation({
    mutationFn: (id: string) => api.revokeCostSheetLink(id),
    onSuccess: async (_, id) => {
      if (made?.id === id) setMade(null);
      await refresh();
      notify("Link revoked", "It no longer opens the cost sheet.");
    },
  });
  const copy = async (url: string) => {
    try {
      await navigator.clipboard.writeText(url);
      notify("Link copied", "Finance can open it without signing in.");
    } catch {
      notify("Copy the link from the box");
    }
  };
  const close = () => {
    setMade(null);
    onClose();
  };
  return (
    <Dialog open={open} onOpenChange={(o) => !o && close()} title="Link for finance" description={`Finance opens the cost sheet for ${formatPeriod(period)} from the link without signing in. Revoke a link to stop it working.`}>
      {made?.url ? (
        <div className="mb-4 space-y-2">
          <Field label="New link" hint="The link is shown only now.">
            <Input readOnly value={made.url} onFocus={(e) => e.target.select()} />
          </Field>
          <Button variant="primary" onClick={() => void copy(made.url ?? "")}>
            Copy link
          </Button>
        </div>
      ) : (
        <Button variant="primary" className="mb-4" onClick={() => make.mutate()} disabled={make.isPending}>
          Make a link
        </Button>
      )}
      <FormError error={make.error ?? revoke.error} />
      <h3 className="mb-2 text-sm font-semibold">Links</h3>
      {links.isPending ? (
        <Loading />
      ) : links.isError ? (
        <ErrorBox error={links.error} />
      ) : links.data.length === 0 ? (
        <p className="text-sm text-muted">No links yet.</p>
      ) : (
        <ul className="divide-y divide-line rounded-md border border-line">
          {links.data.map((link) => (
            <li key={link.id} className="flex items-center justify-between gap-3 px-3 py-2 text-sm">
              <span className="min-w-0">
                Made {formatDateTime(link.created_at)} by {link.created_by}
                {link.revoked_at ? <span className="block text-xs text-muted">Revoked {formatDateTime(link.revoked_at)}</span> : null}
              </span>
              {link.revoked_at ? (
                <Badge>revoked</Badge>
              ) : (
                <Button onClick={() => revoke.mutate(link.id)} disabled={revoke.isPending}>
                  Revoke
                </Button>
              )}
            </li>
          ))}
        </ul>
      )}
      <div className="mt-5 flex justify-end">
        <Button onClick={close}>Done</Button>
      </div>
    </Dialog>
  );
}

function ExportCsv({ open, onClose, sheet }: { open: boolean; onClose: () => void; sheet: CostSheet }) {
  const names = useTagNames();
  const [columns, setColumns] = useState<string[]>([]);
  const download = useMutation({
    mutationFn: () => api.costSheetCsv(sheet.period, columns),
    onSuccess: (blob) => {
      downloadFile(`cost-sheet-${sheet.period}.csv`, blob);
      onClose();
    },
  });
  const extra = (names.data ?? []).filter((n) => !sheet.rungs.includes(n.name));
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
  const periods = usePeriods();
  const sheet = useQuery({ queryKey: ["cost-sheet", period], queryFn: () => api.costSheet(period) });
  const [dialog, setDialog] = useState<"" | "adjust" | "csv" | "link">("");
  if (sheet.isPending) return <Loading />;
  if (sheet.isError) return <ErrorBox error={sheet.error} />;
  const closed = sheet.data.closed;
  return (
    <>
      <PageHeader
        title=""
        actions={
          <>
            <Link to="/close" className="mr-auto text-sm text-accent hover:underline sm:mr-2">
              All periods
            </Link>
            {closed ? <Button onClick={() => setDialog("link")}>Link for finance</Button> : null}
            <Button onClick={() => setDialog("csv")}>Download CSV</Button>
            {closed ? (
              <Button variant="primary" onClick={() => setDialog("adjust")}>
                Post adjustment
              </Button>
            ) : null}
          </>
        }
      />
      <CostSheetView sheet={sheet.data} organization={me.organization.name} closedBy={periods.data?.find((p) => p.period === period)?.closed_by ?? null} />
      {closed ? <PostAdjustment open={dialog === "adjust"} onClose={() => setDialog("")} sheet={sheet.data} /> : null}
      {closed ? <FinanceLinks open={dialog === "link"} onClose={() => setDialog("")} period={period} /> : null}
      <ExportCsv open={dialog === "csv"} onClose={() => setDialog("")} sheet={sheet.data} />
    </>
  );
}

/** The cost sheet as finance sees it from a link: read-only, no sign-in. */
export function SharedCostSheetPage(): ReactNode {
  const { token } = sharedCostSheetRoute.useParams();
  const sheet = useQuery({ queryKey: ["shared-cost-sheet", token], queryFn: () => api.sharedCostSheet(token), retry: false });
  const download = useMutation({
    mutationFn: () => api.sharedCostSheetCsv(token),
    onSuccess: (blob) => {
      downloadFile(`cost-sheet-${sheet.data?.period ?? "shared"}.csv`, blob);
    },
  });
  const gone = sheet.error instanceof ApiError && sheet.error.status === 404;
  return (
    <main className="mx-auto max-w-5xl px-4 py-8 sm:px-6">
      <div className="mb-6 flex items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <img src="/logo-light-small.svg" width="24" height="24" alt="" className="logo-light size-6" />
          <img src="/logo-dark-small.svg" width="24" height="24" alt="" className="logo-dark size-6" />
          <span className="text-[15px] font-semibold">Token Controller</span>
        </div>
        {sheet.data ? (
          <Button onClick={() => download.mutate()} disabled={download.isPending}>
            Download CSV
          </Button>
        ) : null}
      </div>
      <FormError error={download.error} />
      {sheet.isPending ? (
        <Loading />
      ) : gone ? (
        <Note tone="exception">This link does not open a cost sheet. It may have been revoked; ask the controller who sent it for a new one.</Note>
      ) : sheet.isError ? (
        <ErrorBox error={sheet.error} />
      ) : (
        <CostSheetView sheet={sheet.data} organization={sheet.data.organization} />
      )}
    </main>
  );
}
