import { useMutation, useQuery } from "@tanstack/react-query";
import { getRouteApi } from "@tanstack/react-router";
import { useState, type ReactNode } from "react";
import type { Filter, Tags, TokenBudget } from "@/api/contract";
import { api } from "@/api/endpoints";
import { PeriodSelect } from "@/components/period-select";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog } from "@/components/ui/dialog";
import { Empty, ErrorBox, Loading, MoneyText, Note, PageHeader, RowBar, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select } from "@/components/ui/form";
import { Menu } from "@/components/ui/menu";
import { DataTable, type Column } from "@/components/ui/table";
import { Tag } from "@/components/ui/tag";
import { notify } from "@/components/ui/toast";
import { describeFilter, filterToText, formatPeriod } from "@/lib/format";
import { formatMoney, formatShare, money, nanosOf, parseAmount, shareOf, toDecimal } from "@/lib/money";
import { refreshAll, useMe, usePersonName } from "@/lib/queries";
import { hasRole } from "@/lib/roles";
import { thisPeriod } from "@/lib/today";

const budgetsRoute = getRouteApi("/app/budgets");

export function FilterTags({ filter }: { filter: Filter }) {
  const entries = Object.entries(filter);
  if (!entries.length) return <span className="text-muted">Whole organization</span>;
  return (
    <span className="flex flex-wrap gap-1">
      {entries.map(([k, v]) => (
        <Tag key={k} name={k} value={v} />
      ))}
    </span>
  );
}

/** Builds a tag filter. A people manager starts from one of their assigned filters. */
export function FilterBuilder({ value, onChange }: { value: Filter; onChange: (filter: Filter) => void }) {
  const me = useMe();
  const names = useQuery({ queryKey: ["tag-names"], queryFn: api.tagNames });
  const [name, setName] = useState("team");
  const [tagValue, setTagValue] = useState("");
  const base = me.person.role === "people_manager" ? me.person.filters : [];
  const valuesOf = names.data?.find((n) => n.name === name)?.values ?? [];
  const add = () => {
    if (!tagValue.trim()) return;
    onChange({ ...value, [name]: tagValue.trim() });
    setTagValue("");
  };
  return (
    <div className="space-y-3">
      {base.length ? (
        <Field label="Within">
          <Select aria-label="Within filter" value={filterToText(base.find((f) => Object.entries(f).every(([k, v]) => value[k] === v)) ?? {})} onChange={(e) => {
            const picked = base.find((f) => filterToText(f) === e.target.value) ?? {};
            onChange({ ...picked });
          }}>
            {base.map((f) => (
              <option key={filterToText(f)} value={filterToText(f)}>
                {describeFilter(f)}
              </option>
            ))}
          </Select>
        </Field>
      ) : null}
      <div className="flex min-h-7 flex-wrap gap-1">
        {Object.entries(value).map(([k, v]) => (
          <Tag key={k} name={k} value={v} onRemove={() => onChange(Object.fromEntries(Object.entries(value).filter(([key]) => key !== k)))} />
        ))}
        {!Object.keys(value).length ? <span className="text-sm text-muted">Whole organization</span> : null}
      </div>
      <div className="flex flex-wrap items-end gap-2">
        <Field label="Tag" className="w-36">
          <Select aria-label="Tag name" value={name} onChange={(e) => setName(e.target.value)}>
            {(names.data ?? []).map((n) => (
              <option key={n.name}>{n.name}</option>
            ))}
          </Select>
        </Field>
        <Field label="Value" className="min-w-36 flex-1">
          <Input aria-label="Tag value" list="filter-values" value={tagValue} onChange={(e) => setTagValue(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); add(); } }} />
          <datalist id="filter-values">
            {valuesOf.map((v) => (
              <option key={v.value} value={v.value} />
            ))}
          </datalist>
        </Field>
        <Button onClick={add}>Add tag</Button>
      </div>
    </div>
  );
}

function NewBudget({ open, onClose, period, initial }: { open: boolean; onClose: () => void; period: string; initial?: { filter: Tags; amount: string; from_forecast: boolean } }) {
  const me = useMe();
  const within = me.person.role === "people_manager" ? (me.person.filters[0] ?? {}) : {};
  const [filter, setFilter] = useState<Filter>({ ...within, ...(initial?.filter ?? {}) });
  const [amount, setAmount] = useState(initial?.amount ?? "");
  const [forPeriod, setForPeriod] = useState(period);
  const add = useMutation({
    mutationFn: () => {
      const value = parseAmount(amount, me.organization.reporting_currency);
      if (!value) throw new Error("The amount is not a number.");
      return api.addBudget({ filter, period: forPeriod, amount: value, from_forecast: initial?.from_forecast ?? false });
    },
    onSuccess: async () => {
      await refreshAll();
      notify("Token budget saved");
      onClose();
    },
  });
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()} title="New token budget" description="Money planned for a tag filter and a period. It never stops spending and sends no alert.">
      <form className="space-y-4" onSubmit={(e) => { e.preventDefault(); add.mutate(); }}>
        <FilterBuilder value={filter} onChange={setFilter} />
        <div className="grid grid-cols-2 gap-3">
          <Field label="Period">
            <PeriodSelect value={forPeriod} onChange={setForPeriod} />
          </Field>
          <Field label={`Amount in ${me.organization.reporting_currency}`}>
            <Input inputMode="decimal" required value={amount} onChange={(e) => setAmount(e.target.value)} placeholder="1,500" />
          </Field>
        </div>
        <FormError error={add.error} />
        <div className="flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={add.isPending}>
            Save token budget
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

function Forecast({ period, onAccept }: { period: string; onAccept: (ticket: string, amount: string) => void }) {
  const me = useMe();
  const tickets = useQuery({ queryKey: ["tickets"], queryFn: () => api.tickets() });
  const [ticket, setTicket] = useState("");
  const forecast = useQuery({ queryKey: ["forecast", ticket], queryFn: () => api.forecast(ticket), enabled: Boolean(ticket), retry: false });
  const [dropped, setDropped] = useState<Set<string>>(new Set());
  if (!me.organization.forecasts) return <p className="text-sm text-muted">Token forecasts are switched off for this organization.</p>;
  const picked = forecast.data?.matches.filter((m) => !dropped.has(m.ticket)) ?? [];
  const costs = picked.map((m) => nanosOf(m.cost)).sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
  const mid = Math.floor(costs.length / 2);
  const median = costs.length ? (costs.length % 2 ? (costs[mid] ?? 0n) : ((costs[mid - 1] ?? 0n) + (costs[mid] ?? 0n)) / 2n) : 0n;
  const proposal = money(median, me.organization.reporting_currency);
  return (
    <div className="rounded-lg border border-line p-4">
      <Field label="Ticket" hint="Compares its title and description with the summaries of past token entries.">
        <Select aria-label="Forecast ticket" value={ticket} onChange={(e) => { setTicket(e.target.value); setDropped(new Set()); }}>
          <option value="">Pick a ticket</option>
          {(tickets.data ?? []).map((t) => (
            <option key={t.key} value={t.key}>
              {t.key} {t.title ? `· ${t.title}` : ""}
            </option>
          ))}
        </Select>
      </Field>
      {forecast.isError ? <p className="mt-3 text-sm text-muted">{forecast.error.message}</p> : null}
      {forecast.data ? (
        <div className="mt-4">
          <p className="mb-2 text-[13px] text-muted">Similar past tickets. Untick the ones that do not fit.</p>
          <ul className="divide-y divide-line">
            {forecast.data.matches.map((m) => (
              <li key={m.ticket} className="flex items-center gap-3 py-2 text-sm">
                <Checkbox label={`Use ${m.ticket}`} checked={!dropped.has(m.ticket)} onCheckedChange={(on) => { const next = new Set(dropped); if (on) next.delete(m.ticket); else next.add(m.ticket); setDropped(next); }} />
                <span className="font-mono text-[13px]">{m.ticket}</span>
                <span className="min-w-0 flex-1 truncate text-ink-soft">{m.title}</span>
                <Badge tone={m.confidence >= 70 ? "posted" : "neutral"}>{m.confidence}% match</Badge>
                <MoneyText value={m.cost} />
              </li>
            ))}
          </ul>
          <div className="mt-4 flex flex-wrap items-center justify-between gap-3 border-t border-ink pt-3">
            <span className="text-sm">
              Proposal, the median: <b className="num">{formatMoney(proposal)}</b>
            </span>
            <Button variant="primary" disabled={!picked.length} onClick={() => onAccept(ticket, toDecimal(proposal))}>
              Accept as token budget for {formatPeriod(period)}
            </Button>
          </div>
        </div>
      ) : null}
    </div>
  );
}

export function BudgetsPage(): ReactNode {
  const me = useMe();
  const name = usePersonName();
  const navigate = budgetsRoute.useNavigate();
  const search = budgetsRoute.useSearch();
  const period = search.period ?? thisPeriod();
  const budgets = useQuery({ queryKey: ["budgets", period], queryFn: () => api.budgets(period) });
  const [dialog, setDialog] = useState<{ filter: Tags; amount: string; from_forecast: boolean } | "new" | null>(null);
  const remove = useMutation({ mutationFn: api.removeBudget, onSuccess: async () => { await refreshAll(); notify("Token budget removed"); } });
  const columns: Column<TokenBudget>[] = [
    { key: "filter", header: "Filter", cell: (b) => <FilterTags filter={b.filter} />, csv: (b) => describeFilter(b.filter) },
    { key: "budget", header: "Token budget", align: "right", cell: (b) => <MoneyText value={b.amount} />, csv: (b) => toDecimal(b.amount) },
    { key: "actual", header: "Actual", align: "right", cell: (b) => <MoneyText value={b.actual} />, csv: (b) => toDecimal(b.actual) },
    {
      key: "used",
      header: "Used",
      wide: true,
      className: "w-40",
      cell: (b) => {
        const used = shareOf(b.actual, b.amount);
        return (
          <span className="flex items-center gap-2">
            <RowBar share={Math.min(used, 10000)} color={used > 10000 ? "var(--color-exception)" : "var(--color-posted)"} />
            <span className="num w-12 text-xs">{formatShare(used)}</span>
          </span>
        );
      },
      csv: (b) => formatShare(shareOf(b.actual, b.amount)),
    },
    { key: "source", header: "From", wide: true, cell: (b) => (b.from_forecast ? <Badge tone="accent">forecast</Badge> : <span className="text-[13px] text-muted">typed by {name(b.created_by)}</span>), csv: (b) => (b.from_forecast ? "forecast" : "typed") },
    { key: "menu", header: "", cell: (b) => <span onClick={(e) => e.stopPropagation()}><Menu label="Token budget actions" items={[{ label: "Remove", danger: true, onClick: () => remove.mutate(b.id) }]} /></span> },
  ];
  return (
    <>
      <PageHeader
        title="Token budgets"
        lead={hasRole(me.person, "controller") ? "Money planned for any tag filter and a period, compared with actual cost." : "Money planned within the filters you are assigned, compared with actual cost."}
        actions={
          <Button variant="primary" onClick={() => setDialog("new")}>
            New token budget
          </Button>
        }
      />
      <Note>Token budgets can overlap: one for team: web and one for client: acme can count the same money. Never add them up. A token budget never stops an agent.</Note>
      <div className="mb-4">
        <PeriodSelect value={period} onChange={(p) => void navigate({ search: { period: p } })} />
      </div>
      <Section title={`Token budgets for ${formatPeriod(period)}`}>
        {budgets.isPending ? <Loading /> : budgets.isError ? <ErrorBox error={budgets.error} /> : budgets.data.length ? (
          <DataTable stack columns={columns} rows={budgets.data} rowKey={(b) => b.id} csvName={`token-budgets-${period}.csv`} caption="Token budgets" />
        ) : (
          <Empty title="No token budgets for this period">Type one, or accept a token forecast for a ticket.</Empty>
        )}
      </Section>
      <Section title="Token forecast" lead="What similar past work cost. It becomes a token budget only when you accept it.">
        <Forecast period={period} onAccept={(ticket, amount) => setDialog({ filter: { ticket }, amount, from_forecast: true })} />
      </Section>
      {dialog ? (
        <NewBudget key={JSON.stringify(dialog)} open onClose={() => setDialog(null)} period={period} {...(dialog === "new" ? {} : { initial: dialog })} />
      ) : null}
    </>
  );
}
