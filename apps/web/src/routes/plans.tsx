import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { Estimate } from "@/api/contract";
import { api } from "@/api/endpoints";
import { Amount } from "@/components/controls";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Hint, Kpi, KpiRow, Loading, MoneyText, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select, Textarea } from "@/components/ui/form";
import { DataTable } from "@/components/ui/table";
import { TagRow } from "@/components/ui/tag";
import { notify } from "@/components/ui/toast";
import { deviationLabelText, displayName, filterFromText, formatFraction, formatRange } from "@/lib/format";
import { refreshAll, usePeople, usePersonName, useTags } from "@/lib/queries";
import { fromDecimal, nanosOf, sum } from "@/lib/money";
import { today } from "@/lib/today";

// Docs: Plans and estimates. An estimate is a distribution with a median,
// markup inside; accepting one makes it the plan for its tag set. A share is
// a person's fraction of a project's plan between two dates.
//
// The service has no list of estimates (apps/server/src/plans/mod.rs:36), so
// the page reads the estimate events from the log and each estimate by id.
function useEstimates() {
  return useQuery({
    queryKey: ["estimates"],
    queryFn: async () => {
      const events = await api.events("estimate");
      const all = await Promise.all(events.map((e) => api.estimate(e.id)));
      return all.toSorted((a, b) => b.recorded_at.localeCompare(a.recorded_at));
    },
  });
}

export function PlansPage(): ReactNode {
  const estimates = useEstimates();
  const personName = usePersonName();
  const [asking, setAsking] = useState(false);
  const accept = useMutation({
    mutationFn: (id: string) => api.accept(id),
    onSuccess: async () => {
      await refreshAll();
      notify("Accepted", "It is the plan for its tag set now.");
    },
  });
  return (
    <>
      <PageHeader title="Plans and estimates" actions={<Button variant="primary" onClick={() => setAsking(true)}>Ask for an estimate</Button>} />
      {estimates.data ? <PlanFigures estimates={estimates.data} /> : null}
      <Section title="Estimates" flush hint="Accepting an estimate makes it the plan for its tag set. A plan is compared with cost and never stops spending.">
        {estimates.error ? <div className="px-5 pb-5"><ErrorBox error={estimates.error} /></div> : null}
        {!estimates.data ? (
          <div className="px-5 pb-5"><Loading /></div>
        ) : (
          <DataTable
            caption="Estimates"
            stack
            rows={estimates.data}
            rowKey={(e) => e.id}
            empty="No estimates yet."
            columns={[
              { key: "work", header: "Work", cell: (e) => <span className="inline-flex max-w-xs flex-col items-end text-right sm:items-start sm:text-left"><span className="font-medium">{e.work}</span><span className="text-caption text-muted">{e.estimator === "classifier" ? "classifier" : personName(e.estimator)} · {formatRange(e.from, e.to)}</span></span> },
              { key: "tags", header: "Tag set", cell: (e) => <TagRow tags={e.tags} /> },
              { key: "amount", header: "Amount", align: "right", cell: (e) => <span className="inline-flex flex-col items-end"><Amount value={e.amount} currency={e.currency} />{e.markup_percent ? <span className="text-caption text-muted">{e.markup_percent} % markup inside</span> : null}</span> },
              { key: "bands", header: "Distribution", wide: true, cell: (e) => <Bands estimate={e} /> },
              { key: "deviation", header: "Cost · deviation", align: "right", cell: (e) => (e.accepted && e.deviation ? <span className="inline-flex flex-col items-end"><Amount value={e.cost} currency={e.currency} /><span className={Number(e.deviation) < 0 ? "text-exception" : "text-posted"}><Amount value={e.deviation} currency={e.currency} signed /></span>{e.deviation_label ? <span className="text-caption text-muted">{deviationLabelText[e.deviation_label] ?? e.deviation_label}</span> : null}</span> : null) },
              { key: "plan", header: "", align: "right", cell: (e) => (e.accepted ? <Badge tone="posted">plan</Badge> : <Button size="sm" onClick={() => accept.mutate(e.id)} aria-label={`Accept ${e.work}`}>Accept</Button>) },
            ]}
          />
        )}
        {accept.error ? <div className="px-5 py-3"><FormError error={accept.error} /></div> : null}
      </Section>
      <Shares />
      <AskForEstimate open={asking} onClose={() => setAsking(false)} />
    </>
  );
}

/**
 * The plans in force against their cost: what the eye should land on first.
 * Plans on different tag names overlap (a feature's cost is inside its
 * project's), so they are added up only within one set of names, one figure
 * per set.
 */
function PlanFigures({ estimates }: { estimates: Estimate[] }) {
  const plans = estimates.filter((e) => e.accepted);
  const currency = plans[0]?.currency;
  if (!currency) return null;
  const byNames = new Map<string, Estimate[]>();
  for (const e of plans) {
    const names = Object.keys(e.tags).sort().join(" and ") || "no tag";
    byNames.set(names, [...(byNames.get(names) ?? []), e]);
  }
  const total = (of: Estimate[], pick: (e: Estimate) => string | null | undefined) => sum(of.map((e) => fromDecimal(pick(e) ?? "0", currency)), currency);
  const count = (of: Estimate[]) => `${String(of.length)} ${of.length === 1 ? "plan" : "plans"}`;
  const groups = [...byNames.entries()].sort(([a], [b]) => a.localeCompare(b));
  const only = groups.length === 1 ? groups[0] : undefined;
  if (only) {
    const [, of] = only;
    const deviation = total(of, (e) => e.deviation);
    return (
      <KpiRow>
        <Kpi hero label="Deviation" value={<MoneyText value={deviation} signed />} tone={nanosOf(deviation) < 0n ? "exception" : "posted"} note="plan minus cost" />
        <Kpi label="Planned" value={<MoneyText value={total(of, (e) => e.amount)} />} note={count(of)} />
        <Kpi label="Cost" value={<MoneyText value={total(of, (e) => e.cost)} />} />
      </KpiRow>
    );
  }
  return (
    <KpiRow>
      {groups.map(([names, of]) => {
        const deviation = total(of, (e) => e.deviation);
        return (
          <Kpi
            key={names}
            label={`Deviation, plans on ${names}`}
            value={<MoneyText value={deviation} signed />}
            tone={nanosOf(deviation) < 0n ? "exception" : "posted"}
            note={<>{count(of)}: planned <MoneyText value={total(of, (e) => e.amount)} />, cost <MoneyText value={total(of, (e) => e.cost)} /></>}
          />
        );
      })}
    </KpiRow>
  );
}

/** The bands as one bar, darker where more likely, with the full list behind the hint. */
function Bands({ estimate }: { estimate: Estimate }) {
  if (!estimate.distribution.length) return <span className="text-caption text-muted">median only</span>;
  const bands = estimate.distribution.map((b, i, all) => {
    const lower = i === 0 ? "0" : (all[i - 1]?.up_to ?? "0");
    return { label: b.up_to ? `${lower}–${b.up_to}` : `over ${lower}`, p: Number(b.p) };
  });
  const most = Math.max(...bands.map((b) => b.p));
  return (
    <span className="flex items-center gap-2">
      <span className="flex h-2 w-28 overflow-hidden rounded-full bg-sand" role="img" aria-label={`Distribution: ${bands.map((b) => `${b.label} ${formatFraction(String(b.p))}`).join(", ")}`}>
        {bands.map((b) => (
          <span key={b.label} className="h-full border-r border-surface last:border-r-0" style={{ width: `${String(100 / bands.length)}%`, background: "var(--color-spectrum-6)", opacity: most ? 0.15 + (0.85 * b.p) / most : 0 }} />
        ))}
      </span>
      <Hint label={`Distribution of ${estimate.work}`}>
        <ul className="flex flex-col gap-0.5">
          {bands.map((b) => (
            <li key={b.label} className="flex justify-between gap-4"><span className="num !text-left">{b.label}</span><span className="num">{formatFraction(String(b.p))}</span></li>
          ))}
        </ul>
      </Hint>
    </span>
  );
}

function AskForEstimate({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [work, setWork] = useState("");
  const [tags, setTags] = useState("");
  const [markup, setMarkup] = useState("");
  const [median, setMedian] = useState("");
  const [from, setFrom] = useState(today());
  const [to, setTo] = useState("");
  const ask = useMutation({
    mutationFn: () =>
      api.addEstimate({
        work,
        tags: filterFromText(tags.replace(/\s+/g, "")),
        ...(markup ? { markup_percent: Number(markup) } : {}),
        ...(median ? { median } : {}),
        from,
        ...(to ? { to } : {}),
      }),
    onSuccess: async (e) => {
      await refreshAll();
      notify("Estimate made", `Median $${e.median}`);
      setWork("");
      onClose();
    },
  });
  return (
    <Dialog open={open} onOpenChange={(o) => { if (!o) onClose(); }} title="Ask for an estimate" description="The classifier answers with a distribution, or type the median yourself.">
      <form className="grid gap-4 sm:grid-cols-2" onSubmit={(e) => { e.preventDefault(); ask.mutate(); }}>
        <Field label="Work" className="sm:col-span-2"><Textarea value={work} onChange={(e) => setWork(e.target.value)} /></Field>
        <Field label="Tag set" hint="Such as feature:KD-2"><Input value={tags} onChange={(e) => setTags(e.target.value)} /></Field>
        <Field label="Markup, percent"><Input inputMode="numeric" value={markup} onChange={(e) => setMarkup(e.target.value)} /></Field>
        <Field label="Median, typed in" hint="Empty: classifier"><Input inputMode="decimal" value={median} onChange={(e) => setMedian(e.target.value)} /></Field>
        <div className="hidden sm:block" />
        <Field label="First day"><Input type="date" value={from} onChange={(e) => setFrom(e.target.value)} /></Field>
        <Field label="Last day" hint="Empty: 30 days"><Input type="date" value={to} onChange={(e) => setTo(e.target.value)} /></Field>
        <div className="sm:col-span-2"><FormError error={ask.error} /></div>
        <div className="flex justify-end gap-2 sm:col-span-2">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={!work.trim() || ask.isPending}>Estimate</Button>
        </div>
      </form>
    </Dialog>
  );
}

function Shares() {
  const shares = useQuery({ queryKey: ["shares"], queryFn: api.shares });
  const personName = usePersonName();
  const [adding, setAdding] = useState(false);
  return (
    <Section
      title="Shares"
      flush
      hint="Which part of a project's plan a person carries. Shares may add up to more than 1; that is shown, never corrected. A handover is a new share."
      actions={<Button size="sm" onClick={() => setAdding(true)}>Add a share</Button>}
    >
      {!shares.data ? (
        <div className="px-5 pb-5"><Loading /></div>
      ) : (
        <DataTable
          caption="Shares"
          stack
          rows={shares.data}
          rowKey={(s) => s.id}
          empty="No shares yet."
          columns={[
            { key: "person", header: "Person", cell: (s) => <span className="font-medium">{personName(s.person)}</span> },
            { key: "project", header: "Project", cell: (s) => s.project },
            { key: "fraction", header: "Fraction", align: "right", cell: (s) => <span className="num">{s.fraction}</span> },
            { key: "range", header: "From – to", cell: (s) => (s.to ? formatRange(s.from, s.to) : `from ${s.from}`) },
            { key: "over", header: "", align: "right", cell: (s) => (s.over ? <Badge tone="exception">shares add up to {s.over}</Badge> : null) },
          ]}
        />
      )}
      <AddShare open={adding} onClose={() => setAdding(false)} />
    </Section>
  );
}

function AddShare({ open, onClose }: { open: boolean; onClose: () => void }) {
  const people = usePeople();
  const tags = useTags();
  const projects = tags.data?.find((n) => n.name === "project")?.values ?? [];
  const [person, setPerson] = useState("");
  const [project, setProject] = useState("");
  const [fraction, setFraction] = useState("");
  const [from, setFrom] = useState(today());
  const [to, setTo] = useState("");
  const add = useMutation({
    mutationFn: () => api.addShare({ person, project, fraction, from, ...(to ? { to } : {}) }),
    onSuccess: async () => {
      await refreshAll();
      notify("Share added");
      onClose();
    },
  });
  return (
    <Dialog open={open} onOpenChange={(o) => { if (!o) onClose(); }} title="Add a share">
      <form className="grid gap-4 sm:grid-cols-2" onSubmit={(e) => { e.preventDefault(); add.mutate(); }}>
        <Field label="Share person">
          <Select value={person} onChange={(e) => setPerson(e.target.value)}>
            <option value="">Pick</option>
            {(people.data ?? []).map((p) => <option key={p.id} value={p.email}>{displayName(p)}</option>)}
          </Select>
        </Field>
        <Field label="Share project">
          <Select value={project} onChange={(e) => setProject(e.target.value)}>
            <option value="">Pick</option>
            {projects.map((p) => <option key={p.key} value={p.key}>{p.title ?? p.key}</option>)}
          </Select>
        </Field>
        <Field label="Fraction" hint="Such as 0.6"><Input inputMode="decimal" value={fraction} onChange={(e) => setFraction(e.target.value)} /></Field>
        <div className="hidden sm:block" />
        <Field label="Share from"><Input type="date" value={from} onChange={(e) => setFrom(e.target.value)} /></Field>
        <Field label="Share to" hint="Empty: open"><Input type="date" value={to} onChange={(e) => setTo(e.target.value)} /></Field>
        <div className="sm:col-span-2"><FormError error={add.error} /></div>
        <div className="flex justify-end gap-2 sm:col-span-2">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={!person || !project || !fraction || add.isPending}>Add the share</Button>
        </div>
      </form>
    </Dialog>
  );
}
