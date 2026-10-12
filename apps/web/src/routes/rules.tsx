import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { Rule, RuleSignal, SeveralKeys, Tags } from "@/api/contract";
import { api } from "@/api/endpoints";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Loading, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select } from "@/components/ui/form";
import { Switch } from "@/components/ui/switch";
import { Tag } from "@/components/ui/tag";
import { notify } from "@/components/ui/toast";
import { refreshAll, useMe } from "@/lib/queries";
import { hasRole } from "@/lib/roles";

const signalText: Record<RuleSignal, string> = {
  branch: "Branch",
  pull_request_title: "Pull request title",
  prompts: "Prompts or session title",
  repository: "Repository",
  working_directory: "Working directory",
  subagent_type: "Subagent type",
};

const matchText: Record<Rule["match"], string> = {
  ticket_key: "has a ticket key",
  starts_with: "starts with",
  contains: "contains",
  equals: "is",
};

function describe(rule: Rule): string {
  if (rule.match === "ticket_key") return rule.signal === "repository" ? "Repository is mapped to a project: sets that project" : `${signalText[rule.signal]} has a ticket key: sets that ticket`;
  return `${signalText[rule.signal]} ${matchText[rule.match]} “${rule.pattern}”`;
}

function NewRule({ scope, open, onClose }: { scope: Rule["scope"]; open: boolean; onClose: () => void }) {
  const names = useQuery({ queryKey: ["tag-names"], queryFn: api.tagNames });
  const [form, setForm] = useState({ name: "", signal: "branch" as RuleSignal, match: "starts_with" as Rule["match"], pattern: "", tag: "purpose", value: "" });
  const set = <K extends keyof typeof form>(k: K, v: (typeof form)[K]) => setForm((f) => ({ ...f, [k]: v }));
  const add = useMutation({
    mutationFn: () => {
      const sets: Tags = { [form.tag]: form.value.trim() };
      return api.addRule(scope, { name: form.name || `${signalText[form.signal]} ${matchText[form.match]} ${form.pattern}`, signal: form.signal, match: form.match, pattern: form.pattern, sets });
    },
    onSuccess: async () => {
      await refreshAll();
      notify("Rule added", "It runs again over every open period, so it reaches past work too.");
      onClose();
    },
  });
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()} title={scope === "organization" ? "New organization rule" : "New personal rule"} description="A rule turns a signal into a tag. Tags set by hand win over every rule.">
      <form className="space-y-4" onSubmit={(e) => { e.preventDefault(); add.mutate(); }}>
        <Field label="Name">
          <Input value={form.name} onChange={(e) => set("name", e.target.value)} placeholder="Review branches are code review" />
        </Field>
        <div className="grid gap-3 sm:grid-cols-3">
          <Field label="When">
            <Select value={form.signal} onChange={(e) => set("signal", e.target.value as RuleSignal)}>
              {(Object.keys(signalText) as RuleSignal[]).map((s) => (
                <option key={s} value={s}>
                  {signalText[s]}
                </option>
              ))}
            </Select>
          </Field>
          <Field label="Match">
            <Select value={form.match} onChange={(e) => set("match", e.target.value as Rule["match"])}>
              {(["starts_with", "contains", "equals"] as const).map((m) => (
                <option key={m} value={m}>
                  {matchText[m]}
                </option>
              ))}
            </Select>
          </Field>
          <Field label="Text">
            <Input required className="font-mono" value={form.pattern} onChange={(e) => set("pattern", e.target.value)} placeholder="review/" />
          </Field>
        </div>
        <div className="grid gap-3 sm:grid-cols-2">
          <Field label="Set tag">
            <Select value={form.tag} onChange={(e) => set("tag", e.target.value)}>
              {(names.data ?? []).filter((n) => n.name !== "person").map((n) => (
                <option key={n.name}>{n.name}</option>
              ))}
            </Select>
          </Field>
          <Field label="To">
            <Input required value={form.value} onChange={(e) => set("value", e.target.value)} placeholder="code-review" />
          </Field>
        </div>
        <FormError error={add.error} />
        <div className="flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={add.isPending}>
            Add rule
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

function RuleList({ rules, editable, all }: { rules: Rule[]; editable: boolean; all: Rule[] }) {
  const remove = useMutation({ mutationFn: api.removeRule, onSuccess: refreshAll });
  const order = useMutation({ mutationFn: api.orderRules, onSuccess: refreshAll });
  const move = (rule: Rule, delta: number) => {
    const ids = all.map((r) => r.id);
    const from = ids.indexOf(rule.id);
    const target = rules[rules.indexOf(rule) + delta];
    if (!target) return;
    const to = ids.indexOf(target.id);
    ids.splice(from, 1);
    ids.splice(to, 0, rule.id);
    order.mutate(ids);
  };
  if (!rules.length) return <p className="text-sm text-muted">No rules yet.</p>;
  return (
    <ol className="divide-y divide-line rounded-lg border border-line">
      {rules.map((rule, i) => (
        <li key={rule.id} className="flex flex-col gap-2 px-4 py-3 sm:flex-row sm:items-center">
          <span className="num hidden w-6 text-muted sm:block">{i + 1}</span>
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2 text-sm font-medium">
              {rule.name} {rule.built_in ? <Badge>default</Badge> : null}
            </div>
            <div className="mt-0.5 flex flex-wrap items-center gap-1.5 text-[13px] text-muted">
              {describe(rule)}
              {Object.entries(rule.sets).map(([k, v]) => (
                <Tag key={k} name={k} value={v} />
              ))}
            </div>
          </div>
          {editable ? (
            <div className="flex gap-1">
              <Button size="sm" variant="ghost" aria-label={`Move ${rule.name} up`} disabled={i === 0} onClick={() => move(rule, -1)}>↑</Button>
              <Button size="sm" variant="ghost" aria-label={`Move ${rule.name} down`} disabled={i === rules.length - 1} onClick={() => move(rule, 1)}>↓</Button>
              {!rule.built_in ? <Button size="sm" variant="ghost" onClick={() => remove.mutate(rule.id)}>Remove</Button> : null}
            </div>
          ) : null}
        </li>
      ))}
    </ol>
  );
}

export function RulesPage(): ReactNode {
  const me = useMe();
  const rules = useQuery({ queryKey: ["rules"], queryFn: api.rules });
  const [adding, setAdding] = useState<Rule["scope"] | null>(null);
  const controller = hasRole(me.person, "controller");
  const update = useMutation({ mutationFn: api.updateOrganization, onSuccess: async () => { await refreshAll(); notify("Saved"); } });
  if (rules.isPending) return <Loading />;
  if (rules.isError) return <ErrorBox error={rules.error} />;
  const org = rules.data.filter((r) => r.scope === "organization");
  const personal = rules.data.filter((r) => r.scope === "personal");
  return (
    <>
      <PageHeader title="Rules" lead="Rules set the tags of token entries: set once, every new token entry lands where it belongs, and people only handle the exceptions. Organization rules run before personal ones." />
      <Section title="Organization rules" lead="The first rule that finds a ticket wins; then the summary match; then rules that place on a project." actions={controller ? <Button variant="primary" onClick={() => setAdding("organization")}>New organization rule</Button> : undefined}>
        <RuleList rules={org} editable={controller} all={rules.data} />
      </Section>
      <Section title="Your personal rules" lead="They only handle what the organization's rules leave open." actions={<Button onClick={() => setAdding("personal")}>New personal rule</Button>}>
        <RuleList rules={personal} editable all={rules.data} />
      </Section>
      <Section title="When no rule finds a ticket">
        <div className="space-y-4 rounded-lg border border-line p-4">
          <label className="flex items-start gap-3 text-sm">
            <Switch label="Summary match" checked={me.organization.summary_match} disabled={!controller} onCheckedChange={(summary_match) => update.mutate({ summary_match })} />
            <span>
              <b className="font-medium">Summary match.</b> Compare the summary with the titles of open tickets and place the token entry on one that clearly fits, marked matched by summary.
            </span>
          </label>
          <Field label="When prompts name several tickets" className="max-w-md">
            <Select disabled={!controller} value={me.organization.several_keys} onChange={(e) => update.mutate({ several_keys: e.target.value as SeveralKeys })}>
              <option value="most_named">The ticket named most often; on a tie the latest</option>
              <option value="equal_split">An equal split between the tickets</option>
              <option value="first_named">The first one named</option>
            </Select>
          </Field>
        </div>
      </Section>
      {adding ? <NewRule scope={adding} open onClose={() => setAdding(null)} /> : null}
    </>
  );
}
