import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { Estimate } from "@/api/contract";
import { api } from "@/api/endpoints";
import { Amount } from "@/components/controls";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ErrorBox, Loading, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select, Textarea } from "@/components/ui/form";
import { DataTable } from "@/components/ui/table";
import { TagRow } from "@/components/ui/tag";
import { notify } from "@/components/ui/toast";
import { deviationLabelText, displayName, filterFromText, formatFraction, formatRange } from "@/lib/format";
import { refreshAll, usePeople, usePersonName, useTags } from "@/lib/queries";
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
  const accept = useMutation({
    mutationFn: (id: string) => api.accept(id),
    onSuccess: async () => {
      await refreshAll();
      notify("Accepted", "It is the plan for its tag set now.");
    },
  });
  return (
    <>
      <PageHeader title="Plans and estimates" lead="A plan is compared with cost and never stops spending." />
      <Section title="Estimates">
        {estimates.error ? <ErrorBox error={estimates.error} /> : null}
        {!estimates.data ? (
          <Loading />
        ) : (
          <DataTable
            caption="Estimates"
            stack
            rows={estimates.data}
            rowKey={(e) => e.id}
            empty="No estimates yet."
            columns={[
              { key: "work", header: "Work", cell: (e) => <span className="block max-w-xs">{e.work}<span className="block text-xs text-muted">{e.estimator === "classifier" ? "classifier" : personName(e.estimator)} · {formatRange(e.from, e.to)}</span></span> },
              { key: "tags", header: "Tag set", cell: (e) => <TagRow tags={e.tags} /> },
              { key: "amount", header: "Amount", align: "right", cell: (e) => <span className="flex flex-col items-end"><Amount value={e.amount} />{e.markup_percent ? <span className="text-xs text-muted">{e.markup_percent} % markup inside</span> : null}</span> },
              { key: "bands", header: "Distribution", wide: true, cell: (e) => <Bands estimate={e} /> },
              { key: "deviation", header: "Cost · deviation", align: "right", cell: (e) => (e.accepted && e.deviation ? <span className="flex flex-col items-end"><Amount value={e.cost} /><span className={Number(e.deviation) < 0 ? "text-exception" : "text-posted"}><Amount value={e.deviation} signed /></span>{e.deviation_label ? <span className="text-xs text-muted">{deviationLabelText[e.deviation_label] ?? e.deviation_label}</span> : null}</span> : null) },
              { key: "plan", header: "", cell: (e) => (e.accepted ? <Badge tone="posted">plan</Badge> : <Button size="sm" onClick={() => accept.mutate(e.id)} aria-label={`Accept ${e.work}`}>Accept</Button>) },
            ]}
          />
        )}
        <FormError error={accept.error} />
      </Section>
      <AskForEstimate />
      <Shares />
    </>
  );
}

/** The bands as a bar: the chance of each, with the median marked. */
function Bands({ estimate }: { estimate: Estimate }) {
  if (!estimate.distribution.length) return <span className="text-xs text-muted">typed in: median only</span>;
  const bands = estimate.distribution;
  return (
    <ul className="flex flex-col gap-0.5 text-xs" aria-label="Distribution">
      {bands.map((b, i) => {
        const lower = i === 0 ? "0" : (bands[i - 1]?.up_to ?? "0");
        const label = b.up_to ? `${lower}–${b.up_to}` : `over ${lower}`;
        return (
          <li key={label} className="grid grid-cols-[7rem_1fr_3rem] items-center gap-2">
            <span className="num !text-left text-muted">{label}</span>
            <span className="h-1.5 rounded-full bg-sand"><span className="block h-full rounded-full bg-[var(--color-spectrum-6)]" style={{ width: `${String(Number(b.p) * 100)}%` }} /></span>
            <span className="num">{formatFraction(b.p)}</span>
          </li>
        );
      })}
    </ul>
  );
}

function AskForEstimate() {
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
    },
  });
  return (
    <Section title="Ask for an estimate" lead="Describe the work. The classifier answers with a distribution in dollars; or type the median yourself.">
      <form className="grid max-w-3xl gap-3 sm:grid-cols-3" onSubmit={(e) => { e.preventDefault(); ask.mutate(); }}>
        <Field label="Work" className="sm:col-span-3"><Textarea value={work} onChange={(e) => setWork(e.target.value)} /></Field>
        <Field label="Tag set" hint="Such as feature:KD-2. May stay empty."><Input value={tags} onChange={(e) => setTags(e.target.value)} /></Field>
        <Field label="Markup, percent"><Input inputMode="numeric" value={markup} onChange={(e) => setMarkup(e.target.value)} /></Field>
        <Field label="Median, typed in" hint="Empty: the classifier estimates."><Input inputMode="decimal" value={median} onChange={(e) => setMedian(e.target.value)} /></Field>
        <Field label="First day"><Input type="date" value={from} onChange={(e) => setFrom(e.target.value)} /></Field>
        <Field label="Last day" hint="Empty: 30 days."><Input type="date" value={to} onChange={(e) => setTo(e.target.value)} /></Field>
        <div className="sm:col-span-3"><Button type="submit" variant="primary" disabled={!work.trim() || ask.isPending}>Estimate</Button></div>
        <div className="sm:col-span-3"><FormError error={ask.error} /></div>
      </form>
    </Section>
  );
}

function Shares() {
  const shares = useQuery({ queryKey: ["shares"], queryFn: api.shares });
  const people = usePeople();
  const tags = useTags();
  const personName = usePersonName();
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
    },
  });
  return (
    <Section title="Shares" lead="Which part of a project's plan a person carries. Shares may add up to more than 1; that is shown, never corrected. A handover is a new share.">
      {!shares.data ? (
        <Loading />
      ) : (
        <DataTable
          caption="Shares"
          stack
          rows={shares.data}
          rowKey={(s) => s.id}
          empty="No shares yet."
          columns={[
            { key: "person", header: "Person", cell: (s) => personName(s.person) },
            { key: "project", header: "Project", cell: (s) => s.project },
            { key: "fraction", header: "Fraction", align: "right", cell: (s) => <span className="num">{s.fraction}</span> },
            { key: "range", header: "From – to", cell: (s) => (s.to ? formatRange(s.from, s.to) : `from ${s.from}`) },
            { key: "over", header: "", cell: (s) => (s.over ? <Badge tone="exception">shares add up to {s.over}</Badge> : null) },
          ]}
        />
      )}
      <form className="mt-4 grid max-w-3xl gap-3 sm:grid-cols-5 sm:items-end" onSubmit={(e) => { e.preventDefault(); add.mutate(); }}>
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
        <Field label="Share from"><Input type="date" value={from} onChange={(e) => setFrom(e.target.value)} /></Field>
        <Field label="Share to"><Input type="date" value={to} onChange={(e) => setTo(e.target.value)} /></Field>
        <div className="sm:col-span-5"><Button type="submit" disabled={!person || !project || !fraction || add.isPending}>Add the share</Button></div>
        <div className="sm:col-span-5"><FormError error={add.error} /></div>
      </form>
    </Section>
  );
}
