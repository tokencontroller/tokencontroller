import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { Rule, RuleAction, RuleCondition, RuleKeySource, RuleSignal, RuleTest, SeveralKeys } from "@/api/contract";
import { api } from "@/api/endpoints";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Loading, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select } from "@/components/ui/form";
import { Switch } from "@/components/ui/switch";
import { Tag } from "@/components/ui/tag";
import { notify } from "@/components/ui/toast";
import { refreshAll, useMe, useTagNames } from "@/lib/queries";
import { hasRole } from "@/lib/roles";

// Docs: Rules. A rule is a name, conditions on signals that must all hold,
// and one action. Each list is saved as a whole, in order.

const signalText: Record<RuleSignal, string> = {
  branch: "Branch",
  working_directory: "Working directory",
  repository: "Repository",
  pull_request: "Pull request number",
  subagent_type: "Subagent type",
  ticket: "Ticket",
};

const keySourceText: Record<RuleKeySource, string> = {
  branch: "the branch",
  pull_request_title: "the pull request title",
  prompts_or_session_title: "the prompts or the session title",
};

type TestKind = "is" | "starts_with" | "contains" | "matches" | "is_set";

const testText: Record<TestKind, string> = { is: "is", starts_with: "starts with", contains: "contains", matches: "matches the pattern", is_set: "is set" };

const severalKeysText: Record<SeveralKeys, string> = {
  most_often: "The ticket named most often; on a tie the latest",
  equal_split: "An equal split between the tickets",
  first_named: "The first one named",
};

function readTest(test: RuleTest): { kind: TestKind; text: string } {
  if (test === "is_set") return { kind: "is_set", text: "" };
  if ("is" in test) return { kind: "is", text: test.is };
  if ("starts_with" in test) return { kind: "starts_with", text: test.starts_with };
  if ("contains" in test) return { kind: "contains", text: test.contains };
  return { kind: "matches", text: test.matches };
}

function makeTest(kind: TestKind, text: string): RuleTest {
  switch (kind) {
    case "is_set":
      return "is_set";
    case "is":
      return { is: text };
    case "starts_with":
      return { starts_with: text };
    case "contains":
      return { contains: text };
    case "matches":
      return { matches: text };
  }
}

function describeCondition({ signal, test }: RuleCondition): string {
  const { kind, text } = readTest(test);
  return `${signalText[signal]} ${testText[kind]}${kind === "is_set" ? "" : ` “${text}”`}`;
}

function DescribeAction({ action }: { action: RuleAction }): ReactNode {
  if (action === "repository_project") return <>places it on the project its repository is mapped to</>;
  if ("ticket_key_in" in action) return <>finds a ticket key in {keySourceText[action.ticket_key_in]}</>;
  return (
    <>
      sets{" "}
      {Object.entries(action.set_tags).map(([k, v]) => (
        <Tag key={k} name={k} value={v} />
      ))}
    </>
  );
}

/** The rules every organization starts with (docs: The default rules). */
function isDefault(rule: Rule): boolean {
  return !rule.when?.length && (rule.then === "repository_project" || (typeof rule.then === "object" && "ticket_key_in" in rule.then));
}

function RuleList({ rules, editable, saving, onChange }: { rules: Rule[]; editable: boolean; saving: boolean; onChange: (rules: Rule[]) => void }) {
  const move = (from: number, to: number) => {
    const next = [...rules];
    const [rule] = next.splice(from, 1);
    if (!rule) return;
    next.splice(to, 0, rule);
    onChange(next);
  };
  if (!rules.length) return <p className="text-sm text-muted">No rules.</p>;
  return (
    <ol className="divide-y divide-line rounded-lg border border-line">
      {rules.map((rule, i) => (
        <li key={`${String(i)}-${rule.name}`} className="flex flex-col gap-2 px-4 py-3 sm:flex-row sm:items-center">
          <span className="num hidden w-6 text-muted sm:block">{i + 1}</span>
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2 text-sm font-medium">
              {rule.name} {isDefault(rule) ? <Badge>default</Badge> : null}
            </div>
            <div className="mt-0.5 flex flex-wrap items-center gap-1.5 text-[13px] text-muted">
              {rule.when?.length ? `When ${rule.when.map(describeCondition).join(" and ")}, it` : "It"} <DescribeAction action={rule.then} />
            </div>
          </div>
          {editable ? (
            <div className="flex gap-1">
              <Button size="sm" variant="ghost" aria-label={`Move ${rule.name} up`} disabled={saving || i === 0} onClick={() => move(i, i - 1)}>
                ↑
              </Button>
              <Button size="sm" variant="ghost" aria-label={`Move ${rule.name} down`} disabled={saving || i === rules.length - 1} onClick={() => move(i, i + 1)}>
                ↓
              </Button>
              <Button size="sm" variant="ghost" aria-label={`Remove ${rule.name}`} disabled={saving} onClick={() => onChange(rules.filter((_, j) => j !== i))}>
                Remove
              </Button>
            </div>
          ) : null}
        </li>
      ))}
    </ol>
  );
}

type ActionKind = "set_tags" | "ticket_key_in" | "repository_project";
type ConditionRow = { signal: RuleSignal; kind: TestKind; text: string };

function NewRule({ title, onAdd, onClose, error, saving }: { title: string; onAdd: (rule: Rule) => void; onClose: () => void; error: unknown; saving: boolean }) {
  const names = useTagNames();
  const [name, setName] = useState("");
  const [conditions, setConditions] = useState<ConditionRow[]>([{ signal: "branch", kind: "starts_with", text: "" }]);
  const [action, setAction] = useState<ActionKind>("set_tags");
  const [source, setSource] = useState<RuleKeySource>("branch");
  const [tags, setTags] = useState<{ name: string; value: string }[]>([{ name: "purpose", value: "" }]);
  // The ticket a token entry was placed on is a signal only for rules that set other tags.
  const signals = (Object.keys(signalText) as RuleSignal[]).filter((s) => s !== "ticket" || action === "set_tags");
  const settable = (names.data ?? []).filter((n) => n.name !== "person");
  const change = (i: number, row: Partial<ConditionRow>) => setConditions(conditions.map((c, j) => (j === i ? { ...c, ...row } : c)));
  const build = (): Rule => {
    const then: RuleAction = action === "repository_project" ? "repository_project" : action === "ticket_key_in" ? { ticket_key_in: source } : { set_tags: Object.fromEntries<string>(tags.map((t) => [t.name, t.value.trim()])) };
    return { name: name.trim(), when: conditions.map((c) => ({ signal: c.signal, test: makeTest(c.kind, c.text.trim()) })), then };
  };
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()} title={title} description="When every condition holds, the rule does its action. Tags set by hand win over every rule.">
      <form className="space-y-4" onSubmit={(e) => { e.preventDefault(); onAdd(build()); }}>
        <Field label="Name">
          <Input required maxLength={100} value={name} onChange={(e) => setName(e.target.value)} placeholder="Review branches are code review" />
        </Field>
        <fieldset className="space-y-2">
          <legend className="mb-1 text-[13px] font-medium text-ink-soft">When</legend>
          {conditions.map((c, i) => (
            <div key={i} className="grid gap-2 sm:grid-cols-[1fr_1fr_1fr_auto]">
              <Select aria-label={`Signal ${String(i + 1)}`} value={c.signal} onChange={(e) => change(i, { signal: e.target.value as RuleSignal })}>
                {signals.map((s) => (
                  <option key={s} value={s}>
                    {signalText[s]}
                  </option>
                ))}
              </Select>
              <Select aria-label={`Test ${String(i + 1)}`} value={c.kind} onChange={(e) => change(i, { kind: e.target.value as TestKind })}>
                {(Object.keys(testText) as TestKind[]).map((k) => (
                  <option key={k} value={k}>
                    {testText[k]}
                  </option>
                ))}
              </Select>
              {c.kind === "is_set" ? <span /> : <Input aria-label={`Text ${String(i + 1)}`} required className="font-mono" value={c.text} onChange={(e) => change(i, { text: e.target.value })} placeholder={c.kind === "matches" ? "^review/" : "review/"} />}
              <Button size="sm" variant="ghost" onClick={() => setConditions(conditions.filter((_, j) => j !== i))}>
                Remove
              </Button>
            </div>
          ))}
          <div className="flex flex-wrap items-center gap-3">
            <Button size="sm" onClick={() => setConditions([...conditions, { signal: "repository", kind: "is", text: "" }])}>
              Add a condition
            </Button>
            {!conditions.length ? <span className="text-[13px] text-muted">No condition: the rule always applies.</span> : null}
          </div>
        </fieldset>
        <Field label="Then">
          <Select
            value={action}
            onChange={(e) => {
              const next = e.target.value as ActionKind;
              setAction(next);
              if (next !== "set_tags") setConditions(conditions.map((c) => (c.signal === "ticket" ? { ...c, signal: "branch" } : c)));
            }}
          >
            <option value="set_tags">Set tags</option>
            <option value="ticket_key_in">Find a ticket key</option>
            <option value="repository_project">Place on the repository's project</option>
          </Select>
        </Field>
        {action === "ticket_key_in" ? (
          <Field label="Look for it in" hint="Keys that fit a project's ticket key pattern.">
            <Select value={source} onChange={(e) => setSource(e.target.value as RuleKeySource)}>
              {(Object.keys(keySourceText) as RuleKeySource[]).map((k) => (
                <option key={k} value={k}>
                  {keySourceText[k]}
                </option>
              ))}
            </Select>
          </Field>
        ) : null}
        {action === "set_tags" ? (
          <fieldset className="space-y-2">
            <legend className="mb-1 text-[13px] font-medium text-ink-soft">Tags it sets</legend>
            {tags.map((t, i) => (
              <div key={i} className="grid gap-2 sm:grid-cols-[1fr_1fr_auto]">
                <Select aria-label={`Tag name ${String(i + 1)}`} value={t.name} onChange={(e) => setTags(tags.map((x, j) => (j === i ? { ...x, name: e.target.value } : x)))}>
                  {settable.map((n) => (
                    <option key={n.name}>{n.name}</option>
                  ))}
                </Select>
                <Input aria-label={`Tag value ${String(i + 1)}`} required value={t.value} onChange={(e) => setTags(tags.map((x, j) => (j === i ? { ...x, value: e.target.value } : x)))} placeholder="code-review" />
                <Button size="sm" variant="ghost" disabled={tags.length === 1} onClick={() => setTags(tags.filter((_, j) => j !== i))}>
                  Remove
                </Button>
              </div>
            ))}
            <Button size="sm" onClick={() => setTags([...tags, { name: settable.find((n) => !tags.some((t) => t.name === n.name))?.name ?? "purpose", value: "" }])}>
              Add a tag
            </Button>
          </fieldset>
        ) : null}
        <FormError error={error} />
        <div className="flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={saving}>
            Add rule
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

export function RulesPage(): ReactNode {
  const me = useMe();
  const controller = hasRole(me.person, "controller");
  const organization = useQuery({ queryKey: ["rules", "organization"], queryFn: api.organizationRules });
  const mine = useQuery({ queryKey: ["rules", "me"], queryFn: api.myRules });
  const [adding, setAdding] = useState<"organization" | "personal" | null>(null);
  const changeOrganization = useMutation({
    mutationFn: api.changeOrganizationRules,
    onSuccess: async (_, change) => {
      await refreshAll();
      setAdding(null);
      notify("Saved", change.rules ? "Rules run again over every open period, so a fix reaches past work too." : undefined);
    },
  });
  const changeMine = useMutation({
    mutationFn: api.setMyRules,
    onSuccess: async () => {
      await refreshAll();
      setAdding(null);
      notify("Saved", "Your rules run again over your token entries in every open period.");
    },
  });
  if (organization.isPending || mine.isPending) return <Loading />;
  if (organization.isError) return <ErrorBox error={organization.error} />;
  if (mine.isError) return <ErrorBox error={mine.error} />;
  const org = organization.data;
  return (
    <>
      <PageHeader title="Rules" lead="Rules set the tags of token entries: set once, every new token entry lands where it belongs, and people only handle the exceptions." />
      <Section
        title="Organization rules"
        lead="In order. The first rule that finds a ticket wins; then the summary match; then rules that place on a project."
        actions={controller ? <Button variant="primary" onClick={() => { changeOrganization.reset(); setAdding("organization"); }}>New organization rule</Button> : undefined}
      >
        <RuleList rules={org.rules} editable={controller} saving={changeOrganization.isPending} onChange={(rules) => changeOrganization.mutate({ rules })} />
        {adding !== "organization" ? <FormError error={changeOrganization.error} /> : null}
      </Section>
      <Section title="Your personal rules" lead="They run after the organization's, so they only handle what its rules leave open." actions={<Button onClick={() => { changeMine.reset(); setAdding("personal"); }}>New personal rule</Button>}>
        <RuleList rules={mine.data.rules} editable saving={changeMine.isPending} onChange={(rules) => changeMine.mutate(rules)} />
        {adding !== "personal" ? <FormError error={changeMine.error} /> : null}
      </Section>
      <Section title="When no rule finds a ticket">
        <div className="space-y-4 rounded-lg border border-line p-4">
          <label className="flex items-start gap-3 text-sm">
            <Switch label="Summary match" checked={org.summary_match} disabled={!controller || changeOrganization.isPending} onCheckedChange={(summary_match) => changeOrganization.mutate({ summary_match })} />
            <span>
              <b className="font-medium">Summary match.</b> Compare the summary with the titles of open tickets and place the token entry on one that clearly fits, marked matched by summary.
            </span>
          </label>
          <Field label="When prompts name several tickets" className="max-w-md">
            <Select disabled={!controller || changeOrganization.isPending} value={org.several_keys} onChange={(e) => changeOrganization.mutate({ several_keys: e.target.value as SeveralKeys })}>
              {(Object.keys(severalKeysText) as SeveralKeys[]).map((k) => (
                <option key={k} value={k}>
                  {severalKeysText[k]}
                </option>
              ))}
            </Select>
          </Field>
        </div>
      </Section>
      {adding === "organization" ? (
        <NewRule title="New organization rule" saving={changeOrganization.isPending} error={changeOrganization.error} onClose={() => setAdding(null)} onAdd={(rule) => changeOrganization.mutate({ rules: [...org.rules, rule] })} />
      ) : null}
      {adding === "personal" ? (
        <NewRule title="New personal rule" saving={changeMine.isPending} error={changeMine.error} onClose={() => setAdding(null)} onAdd={(rule) => changeMine.mutate([...mine.data.rules, rule])} />
      ) : null}
    </>
  );
}
