import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { NewRule, Rule } from "@/api/contract";
import { api } from "@/api/endpoints";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { Empty, ErrorBox, Loading, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select } from "@/components/ui/form";
import { DataTable } from "@/components/ui/table";
import { notify } from "@/components/ui/toast";
import { refreshAll, useMe, useTags } from "@/lib/queries";
import { hasRole } from "@/lib/roles";

/** Signals every agent may send; any other name a collector sends works too. */
const signals = ["branch", "repository", "pull_request_title", "prompt_keys", "working_directory", "subagent_type", "agent_name", "agent_harness", "organization.id"];

const asNew = (r: Rule): NewRule => ({ name: r.name, signal: r.signal, pattern: r.pattern, tag_name: r.tag_name, key: r.key ?? null });

// Docs: Rules. A pattern over any signal gives a tag value or a contract.
// Organization rules run before personal ones, each in order; a new rule
// also runs back over messages since the cutoff with no hand assignment, and a contract
// rule before it too.
export function RulesPage(): ReactNode {
  const me = useMe();
  const rules = useQuery({ queryKey: ["rules"], queryFn: api.rules });
  const [adding, setAdding] = useState(false);
  const canOrg = hasRole(me.person, "people_manager");
  const org = rules.data?.filter((r) => r.scope === "organization") ?? [];
  const mine = rules.data?.filter((r) => r.scope === me.person.email) ?? [];
  const removeOrg = useMutation({ mutationFn: (r: Rule) => api.reverse(r.id), onSuccess: async () => { await refreshAll(); notify("Rule reversed"); } });
  const removeMine = useMutation({ mutationFn: (r: Rule) => api.setMyRules(mine.filter((x) => x.id !== r.id).map(asNew)), onSuccess: async () => { await refreshAll(); notify("Rule removed"); } });
  const columns = (remove: (r: Rule) => void, may: boolean) => [
    { key: "order", header: "#", cell: (r: Rule) => <span className="num text-muted">{r.order}</span> },
    {
      key: "name",
      header: "Rule",
      cell: (r: Rule) => (
        <span className="flex flex-col gap-0.5">
          <span className="font-medium">{r.name}</span>
          <span className="font-mono text-caption text-muted">{r.signal} ~ {r.pattern}</span>
        </span>
      ),
    },
    { key: "sets", header: "Sets", cell: (r: Rule) => <Badge tone={r.tag_name === "contract" ? "accent" : "neutral"}>{r.tag_name}: {r.key ?? "the key found"}</Badge> },
    { key: "act", header: "", align: "right" as const, cell: (r: Rule) => (may ? <Button size="sm" variant="danger" onClick={() => remove(r)}>Remove</Button> : null) },
  ];
  return (
    <>
      <PageHeader title="Rules" actions={<Button variant="primary" onClick={() => setAdding(true)}>Add a rule</Button>} />
      <AddRuleDialog open={adding} onOpenChange={setAdding} mine={mine} canOrg={canOrg} />
      {rules.error ? <ErrorBox error={rules.error} /> : null}
      <Section title="Organization rules" flush hint="After the sender's own tags, in order. Tags sent or set by hand win over every rule.">
        {!rules.data ? <Loading /> : <DataTable caption="Organization rules" stack rows={org} rowKey={(r) => r.id} empty="No organization rules." columns={columns((r) => removeOrg.mutate(r), canOrg)} />}
        <FormError error={removeOrg.error} />
      </Section>
      <Section title="Your rules" flush={mine.length > 0} hint="They run after the organization's, on your messages only. Remember on a split adds one here.">
        {!rules.data ? (
          <Loading />
        ) : mine.length ? (
          <DataTable caption="Your rules" stack rows={mine} rowKey={(r) => r.id} columns={columns((r) => removeMine.mutate(r), true)} />
        ) : (
          <Empty title="No rules of your own." action={<Button size="sm" onClick={() => setAdding(true)}>Add a rule</Button>} />
        )}
        <FormError error={removeMine.error} />
      </Section>
    </>
  );
}

function AddRuleDialog({ open, onOpenChange, mine, canOrg }: { open: boolean; onOpenChange: (open: boolean) => void; mine: Rule[]; canOrg: boolean }) {
  const tags = useTags();
  const contracts = useQuery({ queryKey: ["contracts"], queryFn: api.contracts });
  const [scope, setScope] = useState(canOrg ? "organization" : "mine");
  const [name, setName] = useState("");
  const [signal, setSignal] = useState("branch");
  const [pattern, setPattern] = useState("");
  const [tagName, setTagName] = useState("feature");
  const [key, setKey] = useState("");
  const names = [...(tags.data ?? []).map((n) => n.name).filter((n) => n !== "contract"), "contract"];
  const add = useMutation({
    mutationFn: async () => {
      const rule: NewRule = { name, signal: signal.trim(), pattern, tag_name: tagName, key: key.trim() || null };
      if (scope === "organization") await api.addRule(rule);
      else await api.setMyRules([...mine.map(asNew), rule]);
    },
    onSuccess: async () => {
      await refreshAll();
      notify("Rule added", `It ran back since the cutoff, except where ${tagName} was set by hand.`);
      setName("");
      setPattern("");
      setKey("");
      onOpenChange(false);
    },
  });
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title="Add a rule" wide>
      <form
        className="grid gap-3 sm:grid-cols-2"
        onSubmit={(e) => {
          e.preventDefault();
          add.mutate();
        }}
      >
        <Field label="Rule name"><Input value={name} onChange={(e) => setName(e.target.value)} /></Field>
        {canOrg ? (
          <Field label="Scope">
            <Select value={scope} onChange={(e) => setScope(e.target.value)}>
              <option value="organization">Organization</option>
              <option value="mine">Only me</option>
            </Select>
          </Field>
        ) : null}
        <Field label="Signal" hint="Any signal name">
          <Input list="rule-signals" value={signal} onChange={(e) => setSignal(e.target.value)} />
          <datalist id="rule-signals">{signals.map((s) => <option key={s} value={s} />)}</datalist>
        </Field>
        <Field label="Pattern" hint="(?<key>KD-\d+) picks the key"><Input value={pattern} onChange={(e) => setPattern(e.target.value)} /></Field>
        <Field label="Sets tag name">
          <Select value={tagName} onChange={(e) => setTagName(e.target.value)}>
            {names.map((n) => <option key={n}>{n}</option>)}
          </Select>
        </Field>
        <Field label="Tag value" hint="Empty: the key found">
          <Input list="rule-values" value={key} onChange={(e) => setKey(e.target.value)} />
          <datalist id="rule-values">{tagName === "contract" ? (contracts.data ?? []).map((c) => <option key={c.key} value={c.key} />) : null}</datalist>
        </Field>
        <div className="sm:col-span-2"><FormError error={add.error} /></div>
        <div className="flex justify-end gap-2 sm:col-span-2">
          <Button onClick={() => onOpenChange(false)}>Cancel</Button>
          <Button variant="primary" type="submit" disabled={!name || !pattern || !signal.trim() || add.isPending}>Add the rule</Button>
        </div>
      </form>
    </Dialog>
  );
}
