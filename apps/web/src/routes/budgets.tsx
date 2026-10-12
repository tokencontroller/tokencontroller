import { useMutation, useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useState, type ReactNode } from "react";
import { ApiError } from "@/api/client";
import type { AcceptInput, Filter, Project, Tags, Ticket, TokenBudgetInput, TokenBudgetView, TokenForecastView } from "@/api/contract";
import { api } from "@/api/endpoints";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog } from "@/components/ui/dialog";
import { Empty, ErrorBox, Loading, MoneyText, Note, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select } from "@/components/ui/form";
import { Menu } from "@/components/ui/menu";
import { Switch } from "@/components/ui/switch";
import { DataTable, type Column } from "@/components/ui/table";
import { Tag, TagValueList } from "@/components/ui/tag";
import { notify } from "@/components/ui/toast";
import { addMonths, describeFilter, filterToText, formatDateTime, formatPeriod } from "@/lib/format";
import { formatMoney, fromDecimal, money, nanosOf } from "@/lib/money";
import { refreshAll, useMe, useProjects, useTagNames, useTickets } from "@/lib/queries";
import { hasRole } from "@/lib/roles";
import { thisPeriod } from "@/lib/today";

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
  const names = useTagNames();
  const [name, setName] = useState("team");
  const [tagValue, setTagValue] = useState("");
  const base = me.person.role === "people_manager" ? me.person.filters : [];
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
          <TagValueList id="filter-values" name={name} />
        </Field>
        <Button onClick={add}>Add tag</Button>
      </div>
    </div>
  );
}

/** When a token budget applies, in words: one period, every month from one on, or all time. */
export function describeApplies(budget: Pick<TokenBudgetView, "period" | "every_month">): string {
  if (!budget.period) return "All time";
  return budget.every_month ? `Every month from ${formatPeriod(budget.period)}` : formatPeriod(budget.period);
}

/** A typed amount as the service reads it: a plain decimal number, without thousands separators. */
const plainAmount = (text: string) => text.replace(/[,\s]/g, "");

/** The service's answer, with a refusal outside the assigned filters said the way the docs say it. */
function withinFilters(error: unknown): unknown {
  if (error instanceof ApiError && error.status === 403) {
    return new Error("People managers set token budgets within the filters they are assigned, and this one is outside them.");
  }
  return error;
}

type Applies = "once" | "every_month" | "all_time";

interface Draft {
  name: string;
  filter: Filter;
  applies: Applies;
  period: string;
  amount: string;
}

function draftOf(budget: TokenBudgetView | null, within: Filter): Draft {
  if (!budget) return { name: "", filter: { ...within }, applies: "every_month", period: thisPeriod(), amount: "" };
  return {
    name: budget.name ?? "",
    filter: budget.filter,
    applies: !budget.period ? "all_time" : budget.every_month ? "every_month" : "once",
    period: budget.period ?? thisPeriod(),
    amount: budget.amount,
  };
}

/** A new token budget, or a change to one. */
function BudgetDialog({ budget, onClose }: { budget: TokenBudgetView | null; onClose: () => void }) {
  const me = useMe();
  const within = me.person.role === "people_manager" ? (me.person.filters[0] ?? {}) : {};
  const [draft, setDraft] = useState(() => draftOf(budget, within));
  const currency = budget?.currency ?? me.organization.reporting_currency;
  const months = Array.from({ length: 19 }, (_, i) => addMonths(thisPeriod(), i - 6));
  if (!months.includes(draft.period)) months.unshift(draft.period);
  const save = useMutation({
    mutationFn: () => {
      const body: TokenBudgetInput = {
        name: draft.name.trim() || null,
        filter: draft.filter,
        amount: plainAmount(draft.amount),
        ...(draft.applies === "all_time" ? {} : { period: draft.period, every_month: draft.applies === "every_month" }),
      };
      return budget ? api.changeTokenBudget(budget.id, body) : api.addTokenBudget(body);
    },
    onSuccess: async () => {
      await refreshAll();
      notify(budget ? "Token budget changed" : "Token budget saved");
      onClose();
    },
  });
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()} title={budget ? "Change token budget" : "New token budget"} description="Money planned for a tag filter. It never stops spending and sends no alert.">
      <form className="space-y-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
        <Field label="Name" hint="Optional, such as Team web, monthly.">
          <Input value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} />
        </Field>
        <FilterBuilder value={draft.filter} onChange={(filter) => setDraft({ ...draft, filter })} />
        <div className="grid gap-3 sm:grid-cols-2">
          <Field label="Applies">
            <Select aria-label="Applies" value={draft.applies} onChange={(e) => setDraft({ ...draft, applies: e.target.value as Applies })}>
              <option value="every_month">Every month from</option>
              <option value="once">One period</option>
              <option value="all_time">All time</option>
            </Select>
          </Field>
          {draft.applies === "all_time" ? null : (
            <Field label="Period">
              <Select aria-label="Period" value={draft.period} onChange={(e) => setDraft({ ...draft, period: e.target.value })}>
                {months.map((month) => (
                  <option key={month} value={month}>
                    {formatPeriod(month)}
                  </option>
                ))}
              </Select>
            </Field>
          )}
        </div>
        <Field label={`Amount in ${currency}`}>
          <Input inputMode="decimal" required value={draft.amount} onChange={(e) => setDraft({ ...draft, amount: e.target.value })} placeholder="1,500" />
        </Field>
        <FormError error={withinFilters(save.error)} />
        <div className="flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={save.isPending}>
            Save token budget
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

/** A ticket's tags with what it inherits, as the service counts them: its own tags, its project and the project's tags. */
function ticketTags(ticket: Ticket, projects: Project[]): Tags {
  const project = projects.find((p) => p.id === ticket.project_id);
  return { ...project?.tags, ...(project ? { project: project.name } : {}), ...ticket.tags, ticket: ticket.key };
}

/** The median of some amounts; of two middle ones, the halves split the way the service splits money. */
function medianOf(amounts: bigint[]): bigint | null {
  const sorted = [...amounts].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
  const upper = Math.floor(sorted.length / 2);
  const high = sorted[upper];
  if (high === undefined) return null;
  if (sorted.length % 2) return high;
  const both = (sorted[upper - 1] ?? high) + high;
  return (both + (both % 2n)) / 2n;
}

function Forecast() {
  const me = useMe();
  const tickets = useTickets();
  const projects = useProjects();
  const [ticket, setTicket] = useState("");
  const [made, setMade] = useState<TokenForecastView | null>(null);
  // The latest token forecast already made for the ticket, as it was made.
  const latest = useQuery({ queryKey: ["token-forecast", ticket], queryFn: () => api.tokenForecast(ticket), enabled: ticket !== "" });
  const forecast = made ?? (ticket ? (latest.data ?? null) : null);
  const [dropped, setDropped] = useState<ReadonlySet<string>>(new Set());
  const [typed, setTyped] = useState("");
  const make = useMutation({
    mutationFn: api.makeTokenForecast,
    onSuccess: (fresh) => {
      setMade(fresh);
      setDropped(new Set());
      setTyped("");
    },
  });
  const accept = useMutation({
    mutationFn: (shown: TokenForecastView) => {
      const amount = plainAmount(typed);
      const body: AcceptInput = amount ? { amount } : dropped.size ? { tickets: shown.matches.filter((m) => !dropped.has(m.ticket)).map((m) => m.ticket) } : {};
      return api.acceptTokenForecast(shown.id, body);
    },
    onSuccess: async (budget) => {
      await refreshAll();
      notify("Token budget saved", `${describeFilter(budget.filter)}, all time`);
      setMade(null);
      setTicket("");
    },
  });
  // A people manager makes token forecasts for the tickets within their filters.
  const covered = (tickets.data ?? []).filter((t) => me.person.role === "controller" || me.person.filters.some((filter) => {
    const tags = ticketTags(t, projects.data ?? []);
    return Object.entries(filter).every(([name, value]) => tags[name] === value);
  }));
  const picked = forecast?.matches.filter((m) => !dropped.has(m.ticket)) ?? [];
  const proposed = forecast
    ? dropped.size
      ? medianOf(picked.filter((m) => m.unknown_model_entries === 0).map((m) => nanosOf(fromDecimal(m.actual, forecast.currency))))
      : forecast.proposed
        ? nanosOf(fromDecimal(forecast.proposed, forecast.currency))
        : null
    : null;
  const toggle = (key: string, on: boolean) => {
    const next = new Set(dropped);
    if (on) next.delete(key);
    else next.add(key);
    setDropped(next);
  };
  return (
    <div className="rounded-lg border border-line p-4">
      <form className="flex flex-col gap-3 sm:flex-row sm:items-end" onSubmit={(e) => { e.preventDefault(); if (ticket) make.mutate(ticket); }}>
        <Field label="Ticket" hint="Compares its title and description with the summaries of past token entries." className="min-w-0 flex-1">
          <Select aria-label="Ticket for the token forecast" value={ticket} onChange={(e) => { setTicket(e.target.value); setMade(null); setDropped(new Set()); setTyped(""); make.reset(); accept.reset(); }}>
            <option value="">Pick a ticket</option>
            {covered.map((t) => (
              <option key={t.key} value={t.key}>
                {t.key}
                {t.title ? ` · ${t.title}` : ""}
              </option>
            ))}
          </Select>
        </Field>
        <Button type="submit" disabled={!ticket || make.isPending}>
          {forecast ? "Make a new token forecast" : "Make a token forecast"}
        </Button>
      </form>
      <div className="mt-3">
        <FormError error={make.error} />
        <FormError error={latest.error} />
      </div>
      {forecast ? (
        <div className="mt-2">
          <p className="mb-2 text-[13px] text-muted">
            Made {formatDateTime(forecast.created_at)}
            {forecast.token_budget_id ? " · accepted as a token budget" : ""}
          </p>
          {forecast.matches.length ? (
            <>
              <p className="mb-2 text-[13px] text-muted">
                Similar past tickets for {forecast.ticket}, with what each cost.{forecast.token_budget_id ? "" : " Untick the ones that do not fit."}
              </p>
              <ul className="divide-y divide-line" aria-label="Similar past tickets">
                {forecast.matches.map((m) => (
                  <li key={m.ticket} className="flex flex-wrap items-center gap-x-3 gap-y-1 py-2 text-sm">
                    {forecast.token_budget_id ? null : <Checkbox label={`Use ${m.ticket}`} checked={!dropped.has(m.ticket)} onCheckedChange={(on) => toggle(m.ticket, on)} />}
                    <span className="font-mono text-[13px]">{m.ticket}</span>
                    <span className="min-w-0 flex-1 basis-40 truncate text-ink-soft">{m.title}</span>
                    <Badge tone={m.confidence >= 70 ? "posted" : "neutral"}>{m.confidence}% confidence</Badge>
                    {m.unknown_model_entries ? <MoneyText value={null} /> : <MoneyText value={fromDecimal(m.actual, forecast.currency)} />}
                  </li>
                ))}
              </ul>
            </>
          ) : (
            <Empty title={`No similar past work for ${forecast.ticket} yet`}>The more periods you close, the better token forecasts get. You can still type a token budget.</Empty>
          )}
          {forecast.token_budget_id ? (
            <p className="mt-4 border-t border-ink pt-3 text-sm">
              It became the token budget on ticket: {forecast.ticket} in the list above, for all time.
            </p>
          ) : (
            <div className="mt-4 space-y-3 border-t border-ink pt-3">
              <p className="text-sm">
                {proposed === null ? (
                  "No ticked match has a known cost, so there is no proposal. Type an amount instead."
                ) : (
                  <>
                    Proposal, the median: <b className="num">{formatMoney(money(proposed, forecast.currency))}</b>
                  </>
                )}
              </p>
              <div className="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
                <Field label={`Or type an amount in ${forecast.currency}`} className="sm:w-56">
                  <Input inputMode="decimal" value={typed} onChange={(e) => setTyped(e.target.value)} placeholder={proposed === null ? "1,500" : ""} />
                </Field>
                <Button variant="primary" disabled={(proposed === null && !plainAmount(typed)) || accept.isPending} onClick={() => accept.mutate(forecast)}>
                  Accept as token budget
                </Button>
              </div>
              <p className="text-xs text-muted">It becomes a token budget on ticket: {forecast.ticket} for all time.</p>
              <FormError error={withinFilters(accept.error)} />
            </div>
          )}
        </div>
      ) : null}
    </div>
  );
}

function TokenForecasts() {
  const me = useMe();
  const settings = useQuery({ queryKey: ["token-forecast-settings"], queryFn: api.tokenForecastSettings });
  const change = useMutation({
    mutationFn: api.switchTokenForecasts,
    onSuccess: async (now) => {
      await refreshAll();
      notify(now.enabled ? "Token forecasts switched on" : "Token forecasts switched off");
    },
  });
  const controller = hasRole(me.person, "controller");
  return (
    <Section
      title="Token forecasts"
      lead="What similar past work cost. It becomes a token budget only when you accept it."
      actions={
        controller && settings.data ? (
          <label className="flex items-center gap-2 text-sm">
            <Switch label="Token forecasts" checked={settings.data.enabled} disabled={!settings.data.available || change.isPending} onCheckedChange={(on) => change.mutate(on)} />
            Token forecasts
          </label>
        ) : null
      }
    >
      <FormError error={change.error} />
      {settings.isPending ? (
        <Loading />
      ) : settings.isError ? (
        <ErrorBox error={settings.error} />
      ) : settings.data.enabled && settings.data.available ? (
        <Forecast />
      ) : (
        <Note tone="exception">{settings.data.reason ?? "Token forecasts are off. Token budgets can still be typed."}</Note>
      )}
    </Section>
  );
}

export function BudgetsPage(): ReactNode {
  const me = useMe();
  const budgets = useQuery({ queryKey: ["token-budgets"], queryFn: api.tokenBudgets });
  const [editing, setEditing] = useState<TokenBudgetView | "new" | null>(null);
  const remove = useMutation({
    mutationFn: api.removeTokenBudget,
    onSuccess: async () => {
      await refreshAll();
      notify("Token budget removed");
    },
  });
  const title = (b: TokenBudgetView) => b.name ?? describeFilter(b.filter);
  const columns: Column<TokenBudgetView>[] = [
    {
      key: "budget",
      header: "Token budget",
      cell: (b) => (
        <span className="flex flex-col items-end gap-1 sm:items-start">
          {b.name ? <span className="font-medium">{b.name}</span> : null}
          <FilterTags filter={b.filter} />
        </span>
      ),
      csv: (b) => b.name ?? "",
    },
    { key: "filter", header: "Filter", csvOnly: true, cell: () => null, csv: (b) => filterToText(b.filter) },
    { key: "applies", header: "Applies", cell: (b) => describeApplies(b), csv: (b) => b.applies },
    { key: "amount", header: "Amount", align: "right", cell: (b) => <MoneyText value={fromDecimal(b.amount, b.currency)} />, csv: (b) => b.amount },
    { key: "currency", header: "Currency", csvOnly: true, cell: () => null, csv: (b) => b.currency },
    {
      key: "from",
      header: "From",
      cell: (b) =>
        b.token_forecast_id ? (
          <span className="flex flex-wrap items-center justify-end gap-1.5 text-[13px] text-muted sm:justify-start">
            <Badge tone="accent">token forecast</Badge>
            {b.set_by ? `accepted by ${b.set_by}` : null}
          </span>
        ) : (
          <span className="text-[13px] text-muted">typed{b.set_by ? ` by ${b.set_by}` : ""}</span>
        ),
      csv: (b) => (b.token_forecast_id ? "token forecast" : "typed"),
    },
    { key: "set_by", header: "Set by", csvOnly: true, cell: () => null, csv: (b) => b.set_by ?? "" },
    {
      key: "menu",
      header: "",
      cell: (b) => (
        <Menu
          label={`Actions for ${title(b)}`}
          items={[
            { label: "Change", onClick: () => setEditing(b) },
            { label: "Remove", danger: true, onClick: () => remove.mutate(b.id) },
          ]}
        />
      ),
    },
  ];
  return (
    <>
      <PageHeader
        title="Token budgets"
        lead={hasRole(me.person, "controller") ? "Money planned for any tag filter, compared with actual cost." : "Money planned within the filters you are assigned, compared with actual cost."}
        actions={
          <>
            <Link to="/reports" className="text-sm text-accent hover:underline">
              Token budget vs actual
            </Link>
            <Button variant="primary" onClick={() => setEditing("new")}>
              New token budget
            </Button>
          </>
        }
      />
      <Note>Token budgets can overlap: one for team: web and one for client: acme can count the same money. Never add them up. A token budget never stops an agent and sends no alert.</Note>
      <Section title="Token budgets">
        <FormError error={withinFilters(remove.error)} />
        {budgets.isPending ? (
          <Loading />
        ) : budgets.isError ? (
          <ErrorBox error={budgets.error} />
        ) : budgets.data.length ? (
          <DataTable stack columns={columns} rows={budgets.data} rowKey={(b) => b.id} csvName="token-budgets.csv" caption="Token budgets" />
        ) : (
          <Empty title="No token budgets yet">Type one, or accept a token forecast for a ticket.</Empty>
        )}
      </Section>
      <TokenForecasts />
      {editing ? <BudgetDialog key={editing === "new" ? "new" : editing.id} budget={editing === "new" ? null : editing} onClose={() => setEditing(null)} /> : null}
    </>
  );
}
