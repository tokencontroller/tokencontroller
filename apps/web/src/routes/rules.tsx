import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { NewRule, Rule } from "@/api/contract";
import { api } from "@/api/endpoints";
import { Button } from "@/components/ui/button";
import { ErrorBox, Loading, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select } from "@/components/ui/form";
import { DataTable } from "@/components/ui/table";
import { notify } from "@/components/ui/toast";
import { refreshAll, useMe, useTags } from "@/lib/queries";
import { hasRole } from "@/lib/roles";

const signals = ["branch", "repository", "pull_request_title", "prompt_keys", "working_directory", "subagent_type", "agent_name"];

const asNew = (r: Rule): NewRule => ({ name: r.name, signal: r.signal, pattern: r.pattern, tag_name: r.tag_name, key: r.key ?? null });

// Docs: Rules. A pattern over signals gives a tag value. Organization rules
// run before personal ones, each in order; a new rule also runs back over
// unassigned messages since the cutoff.
export function RulesPage(): ReactNode {
  const me = useMe();
  const rules = useQuery({ queryKey: ["rules"], queryFn: api.rules });
  const org = rules.data?.filter((r) => r.scope === "organization") ?? [];
  const mine = rules.data?.filter((r) => r.scope === me.person.email) ?? [];
  const removeOrg = useMutation({ mutationFn: (r: Rule) => api.reverse(r.id), onSuccess: async () => { await refreshAll(); notify("Rule reversed"); } });
  const removeMine = useMutation({ mutationFn: (r: Rule) => api.setMyRules(mine.filter((x) => x.id !== r.id).map(asNew)), onSuccess: async () => { await refreshAll(); notify("Rule removed"); } });
  const columns = (remove: (r: Rule) => void, may: boolean) => [
    { key: "order", header: "#", cell: (r: Rule) => <span className="num">{r.order}</span> },
    { key: "name", header: "Rule", cell: (r: Rule) => r.name },
    { key: "pattern", header: "Pattern", cell: (r: Rule) => <span className="font-mono text-xs">{r.signal} matches {r.pattern}</span> },
    { key: "sets", header: "Sets", cell: (r: Rule) => `${r.tag_name}: ${r.key ?? "that key"}` },
    { key: "act", header: "", cell: (r: Rule) => (may ? <Button size="sm" variant="ghost" onClick={() => remove(r)}>Remove</Button> : null) },
  ];
  return (
    <>
      <PageHeader title="Rules" lead="Set rules once, and people only fix the exceptions. A tag set by hand wins over every rule." />
      {rules.error ? <ErrorBox error={rules.error} /> : null}
      <Section title="Organization rules" lead="Run first, in order.">
        {!rules.data ? <Loading /> : <DataTable caption="Organization rules" stack rows={org} rowKey={(r) => r.id} empty="No organization rules." columns={columns((r) => removeOrg.mutate(r), hasRole(me.person, "people_manager"))} />}
        <FormError error={removeOrg.error} />
      </Section>
      <Section title="Your rules" lead="Run after the organization's, on your messages only. Remember on a split adds one here.">
        {!rules.data ? <Loading /> : <DataTable caption="Your rules" stack rows={mine} rowKey={(r) => r.id} empty="No rules of your own yet." columns={columns((r) => removeMine.mutate(r), true)} />}
        <FormError error={removeMine.error} />
      </Section>
      <AddRule mine={mine} canOrg={hasRole(me.person, "people_manager")} />
    </>
  );
}

function AddRule({ mine, canOrg }: { mine: Rule[]; canOrg: boolean }) {
  const tags = useTags();
  const [scope, setScope] = useState(canOrg ? "organization" : "mine");
  const [name, setName] = useState("");
  const [signal, setSignal] = useState("branch");
  const [pattern, setPattern] = useState("");
  const [tagName, setTagName] = useState("feature");
  const [key, setKey] = useState("");
  const add = useMutation({
    mutationFn: async () => {
      const rule: NewRule = { name, signal, pattern, tag_name: tagName, key: key.trim() || null };
      if (scope === "organization") await api.addRule(rule);
      else await api.setMyRules([...mine.map(asNew), rule]);
    },
    onSuccess: async () => {
      await refreshAll();
      notify("Rule added", "It ran back over unassigned messages since the cutoff.");
      setName("");
      setPattern("");
    },
  });
  return (
    <Section title="Add a rule" lead="A regular expression; a group named key picks the tag value, such as (?<key>KD-\d+). Leave the value empty for that key.">
      <form
        className="grid max-w-4xl gap-3 sm:grid-cols-3"
        onSubmit={(e) => {
          e.preventDefault();
          add.mutate();
        }}
      >
        {canOrg ? (
          <Field label="Scope">
            <Select value={scope} onChange={(e) => setScope(e.target.value)}>
              <option value="organization">Organization</option>
              <option value="mine">Only me</option>
            </Select>
          </Field>
        ) : null}
        <Field label="Rule name"><Input value={name} onChange={(e) => setName(e.target.value)} /></Field>
        <Field label="Signal">
          <Select value={signal} onChange={(e) => setSignal(e.target.value)}>
            {signals.map((s) => <option key={s}>{s}</option>)}
          </Select>
        </Field>
        <Field label="Pattern"><Input value={pattern} onChange={(e) => setPattern(e.target.value)} /></Field>
        <Field label="Sets tag name">
          <Select value={tagName} onChange={(e) => setTagName(e.target.value)}>
            {(tags.data ?? []).map((n) => <option key={n.name}>{n.name}</option>)}
          </Select>
        </Field>
        <Field label="Tag value" hint="Empty: the key the pattern finds."><Input value={key} onChange={(e) => setKey(e.target.value)} /></Field>
        <div className="sm:col-span-3"><Button type="submit" disabled={!name || !pattern || add.isPending}>Add the rule</Button></div>
        <div className="sm:col-span-3"><FormError error={add.error} /></div>
      </form>
    </Section>
  );
}
