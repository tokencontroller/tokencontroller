import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode, type SubmitEvent } from "react";
import type { Contract, ContractKind, NewRule, Session } from "@/api/contract";
import { api } from "@/api/endpoints";
import { Amount } from "@/components/controls";
import { Badge, LabelBadge, StatusBadge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { CheckField } from "@/components/ui/checkbox";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Hint, Loading, Note, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select } from "@/components/ui/form";
import { DataTable } from "@/components/ui/table";
import { Segmented } from "@/components/ui/tabs";
import { TagRow, TagValueList } from "@/components/ui/tag";
import { notify } from "@/components/ui/toast";
import { formatDay, formatFraction, formatTime } from "@/lib/format";
import { refreshAll, useMe, usePersonName, useTags } from "@/lib/queries";
import { fromDecimal, sum, toDecimal } from "@/lib/money";
import { hasRole } from "@/lib/roles";

// Docs: Check and submit. Every session since the last submit, how it was
// split and why; fix the exceptions with a split, remember it as a rule, submit.
// Sessions with messages under no contract, or from an unknown provider, are
// asked about once, per agent, and the answer can be remembered as a rule.
export function SessionsPage(): ReactNode {
  const me = useMe();
  const [everyone, setEveryone] = useState(false);
  const sessions = useQuery({ queryKey: ["sessions", everyone], queryFn: () => api.sessions(everyone ? {} : { filter: { person: me.person.email } }) });
  const requests = useQuery({ queryKey: ["submit-requests"], queryFn: api.submitRequests });
  const [editing, setEditing] = useState<Session | null>(null);
  const personName = usePersonName();
  const submit = useMutation({
    mutationFn: api.submit,
    onSuccess: async (s) => {
      await refreshAll();
      notify("Submitted", `${String(s.messages)} messages · $${s.cost}`);
    },
  });
  const rows = sessions.data?.sessions ?? [];
  const open = rows.filter((s) => s.person === me.person.email && (s.submitted === "open" || s.submitted === "rejected"));
  const request = requests.data?.find((r) => !r.done);
  // Open and rejected sessions are the ones to check; the rest are muted in their own view.
  const [view, setView] = useState<"check" | "submitted">("check");
  const toCheck = rows.filter((s) => s.submitted === "open" || s.submitted === "rejected");
  const shown = view === "check" ? toCheck : rows.filter((s) => s.submitted !== "open" && s.submitted !== "rejected");
  return (
    <>
      <PageHeader
        title="Check and submit"
        actions={
          <Button variant="primary" onClick={() => submit.mutate()} disabled={submit.isPending || !open.length}>
            Submit {open.length ? `${String(open.length)} sessions` : ""}
          </Button>
        }
      />
      {request ? <Note>Asked to submit by {formatDay(request.until)}{request.message ? `: ${request.message}` : "."}</Note> : null}
      <FormError error={submit.error} />
      <ContractQuestion sessions={open} />
      <Section
        title="Sessions"
        flush
        actions={
          <>
            {hasRole(me.person, "people_manager") ? <CheckField checked={everyone} onCheckedChange={setEveryone} label="Everyone I may view" /> : null}
            <Segmented
              label="Which sessions"
              value={view}
              onValueChange={setView}
              options={[
                { value: "check", label: "To check", count: toCheck.length },
                { value: "submitted", label: "Submitted", count: rows.length - toCheck.length },
              ]}
            />
          </>
        }
      >
        {sessions.error ? <div className="px-5 pb-5"><ErrorBox error={sessions.error} /></div> : null}
        {!sessions.data ? (
          <div className="px-5 pb-5"><Loading /></div>
        ) : (
          <DataTable
            caption="Sessions"
            stack
            rows={shown}
            rowKey={(s) => s.session_id}
            rowClassName={() => (view === "check" ? "" : "text-ink-soft")}
            empty={view === "check" ? "Nothing to check." : "Nothing submitted yet."}
            columns={[
              { key: "when", header: "When", cell: (s) => `${formatDay(s.started_at)} ${formatTime(s.started_at)}`, csv: (s) => s.started_at },
              ...(everyone ? [{ key: "person", header: "Person", cell: (s: Session) => personName(s.person), csv: (s: Session) => s.person }] : []),
              { key: "summary", header: "Summary", cell: (s) => <span className="inline-flex max-w-xs flex-col items-end text-right sm:items-start sm:text-left"><span className="font-medium text-ink">{s.summary}</span>{s.agent_name ? <span className="text-caption text-muted">{s.agent_name}</span> : null}</span>, csv: (s) => s.summary ?? "" },
              {
                key: "cost",
                header: "Cost",
                align: "right",
                cell: (s) => (
                  <span className="flex flex-col items-end gap-0.5">
                    <Amount value={s.cost} />
                    {s.count_errors ? <Badge tone="exception">{s.count_errors} count error{s.count_errors > 1 ? "s" : ""}</Badge> : null}
                    {s.unknown_models.length ? <Badge tone="exception">not priced yet</Badge> : null}
                  </span>
                ),
                csv: (s) => s.cost,
              },
              {
                key: "split",
                header: "Split",
                cell: (s) => (
                  <span className="flex flex-col gap-1">
                    {s.parts.map((p, i) => (
                      <span key={i} className="flex flex-wrap items-center gap-1">
                        {s.parts.length > 1 ? <span className="num text-caption text-muted">{formatFraction(p.fraction)}</span> : null}
                        {Object.keys(p.assigned).length ? <TagRow tags={p.assigned} /> : <span className="text-caption text-muted">no project</span>}
                        <LabelBadge label={p.label} />
                      </span>
                    ))}
                  </span>
                ),
              },
              {
                key: "contract",
                header: "Contract",
                cell: (s) => (
                  <span className="flex flex-wrap gap-1">
                    {s.contracts.map((c) => <Badge key={c}>{c}</Badge>)}
                    {s.no_contract ? <Badge tone="exception">no contract</Badge> : null}
                    {s.provider_unknown ? <Badge tone="exception">provider unknown</Badge> : null}
                  </span>
                ),
                csv: (s) => s.contracts.join(" "),
              },
              { key: "by", header: "By", wide: true, cell: (s) => (s.assigned_by === "default" ? "no rule matched" : s.assigned_by) },
              { key: "status", header: "Status", cell: (s) => <StatusBadge status={s.submitted} /> },
              { key: "fix", header: "", align: "right", cell: (s) => <Button size="sm" onClick={() => setEditing(s)} aria-label={`Split ${s.summary ?? s.session_id}`}>Split</Button> },
            ]}
          />
        )}
      </Section>
      {editing ? <SplitDialog session={editing} onClose={() => setEditing(null)} /> : null}
    </>
  );
}

/** The agent's name for people, such as Claude Code for `claude-code`. */
const agentName: Record<string, string> = { "claude-code": "Claude Code", codex: "Codex", "github-action": "GitHub Action", cursor: "Cursor", "gemini-cli": "Gemini CLI" };

/** A contract rule for one agent, for "remember". */
function contractRule(agent: string, contract: string): NewRule {
  const escaped = agent.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return { name: `${agentName[agent] ?? agent} is on ${contract}`, signal: "agent_harness", pattern: `^${escaped}$`, tag_name: "contract", key: contract };
}

/**
 * Asked once: the open sessions with messages under no contract or from an
 * unknown provider, one row per agent. The answer goes on those sessions
 * by hand, and may be remembered as a rule for the agent.
 */
function ContractQuestion({ sessions }: { sessions: Session[] }) {
  const contracts = useQuery({ queryKey: ["contracts"], queryFn: api.contracts });
  const asked = sessions.filter((s) => s.no_contract || s.provider_unknown);
  const agents = [...new Set(asked.map((s) => s.agent_harness))].sort();
  if (!asked.length) return null;
  return (
    <Section
      title="Which contract?"
      hint="Some messages came under no contract, or without their provider. Pick the contract they were paid under; it goes on every message of these sessions."
    >
      <ul className="-mx-5 flex flex-col">
        {agents.map((agent) => (
          <ContractAnswer key={agent} agent={agent} sessions={asked.filter((s) => s.agent_harness === agent)} contracts={contracts.data ?? []} />
        ))}
      </ul>
    </Section>
  );
}

const NEW_CONTRACT = "__new";

function ContractAnswer({ agent, sessions, contracts }: { agent: string; sessions: Session[]; contracts: Contract[] }) {
  const [contract, setContract] = useState(contracts[0]?.key ?? NEW_CONTRACT);
  const [key, setKey] = useState("");
  const [provider, setProvider] = useState("");
  const [kind, setKind] = useState<ContractKind>("metered");
  const [remember, setRemember] = useState(true);
  const chosen = contracts.some((c) => c.key === contract) ? contract : NEW_CONTRACT;
  const name = agentName[agent] ?? agent;
  const cost = toDecimal(sum(sessions.map((s) => fromDecimal(s.cost, "USD")), "USD"));
  const save = useMutation({
    mutationFn: async () => {
      let target = chosen;
      if (chosen === NEW_CONTRACT) {
        await api.addTagValue("contract", { key: key.trim(), contract: { provider: provider.trim(), kind } });
        target = key.trim();
      }
      await api.assignContract(target, sessions.map((s) => s.session_id));
      if (remember) {
        const mine = await api.myRules();
        await api.setMyRules([...mine.map((r) => ({ name: r.name, signal: r.signal, pattern: r.pattern, tag_name: r.tag_name, key: r.key ?? null })), contractRule(agent, target)]);
      }
      return target;
    },
    onSuccess: async (target) => {
      await refreshAll();
      notify(`${name}: ${target}`, remember ? "And remembered as your rule." : undefined);
    },
  });
  return (
    <li className="flex flex-col gap-3 border-t border-line px-5 py-3 first:border-t-0">
      <form
        className="flex flex-wrap items-end gap-3"
        onSubmit={(e) => {
          e.preventDefault();
          save.mutate();
        }}
      >
        <div className="min-w-40 flex-1">
          <div className="text-body font-medium">{name}</div>
          <div className="text-small text-muted">
            {sessions.length === 1 ? "1 session" : `${String(sessions.length)} sessions`} · <Amount value={cost} />
          </div>
        </div>
        <Field label={`Contract for ${name}`} className="w-56">
          <Select value={chosen} onChange={(e) => setContract(e.target.value)}>
            {contracts.map((c) => <option key={c.key} value={c.key}>{c.title ?? c.key}</option>)}
            <option value={NEW_CONTRACT}>A new contract</option>
          </Select>
        </Field>
        {chosen === NEW_CONTRACT ? (
          <>
            <Field label="Contract key" className="w-40"><Input value={key} placeholder="anthropic-api" onChange={(e) => setKey(e.target.value)} /></Field>
            <Field label="Provider" className="w-32"><Input value={provider} placeholder="anthropic" onChange={(e) => setProvider(e.target.value)} /></Field>
            <Field label="Kind" className="w-28">
              <Select value={kind} onChange={(e) => setKind(e.target.value as ContractKind)}>
                <option value="metered">metered</option>
                <option value="flat">flat</option>
              </Select>
            </Field>
          </>
        ) : null}
        <CheckField checked={remember} onCheckedChange={setRemember} label="Remember" />
        <Hint label="About remember">A rule of yours: every {name} session goes under this contract, earlier ones too.</Hint>
        <Button type="submit" size="sm" disabled={save.isPending || (chosen === NEW_CONTRACT && (!key.trim() || !provider.trim()))}>Set</Button>
      </form>
      <FormError error={save.error} />
    </li>
  );
}

interface PartDraft {
  percent: string;
  name: string;
  key: string;
}

/** A personal rule from the session's signals, for "remember". */
function ruleFrom(session: Session, name: string, key: string): NewRule | null {
  const signals = session.signals as Record<string, unknown>;
  for (const signal of ["branch", "repository", "working_directory"]) {
    const value = signals[signal];
    if (typeof value === "string" && value) {
      const escaped = value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      return { name: `${signal} ${value} is ${name} ${key}`, signal, pattern: `^${escaped}$`, tag_name: name, key };
    }
  }
  return null;
}

function SplitDialog({ session, onClose }: { session: Session; onClose: () => void }) {
  const tags = useTags();
  const first = (session.parts[0]?.assigned ?? {});
  const [firstName = "feature", firstKey = ""] = Object.entries(first)[0] ?? [];
  const [parts, setParts] = useState<PartDraft[]>(
    session.parts.length > 1
      ? session.parts.map((p) => {
          const [name = "feature", key = ""] = Object.entries(p.assigned)[0] ?? [];
          return { percent: String(Math.round(Number(p.fraction) * 100)), name, key };
        })
      : [{ percent: "100", name: firstName, key: firstKey }],
  );
  const [remember, setRemember] = useState(false);
  const total = parts.reduce((sum, p) => sum + Number(p.percent || 0), 0);
  const save = useMutation({
    mutationFn: async () => {
      await api.split(
        session.session_id,
        parts.map((p) => ({ fraction: String(Number(p.percent) / 100), tags: p.key.trim() ? { [p.name]: p.key.trim() } : {} })),
      );
      const only = parts[0];
      if (remember && parts.length === 1 && only?.key.trim()) {
        const rule = ruleFrom(session, only.name, only.key.trim());
        if (rule) {
          const mine = await api.myRules();
          await api.setMyRules([...mine.map((r) => ({ name: r.name, signal: r.signal, pattern: r.pattern, tag_name: r.tag_name, key: r.key ?? null })), rule]);
        }
      }
    },
    onSuccess: async () => {
      await refreshAll();
      notify("Split saved", remember ? "And remembered as your rule." : undefined);
      onClose();
    },
  });
  const set = (i: number, change: Partial<PartDraft>) => setParts(parts.map((p, j) => (j === i ? { ...p, ...change } : p)));
  const onSubmit = (e: SubmitEvent<HTMLFormElement>) => {
    e.preventDefault();
    save.mutate();
  };
  return (
    <Dialog open onOpenChange={(o) => { if (!o) onClose(); }} title="Split the session" description={session.summary ?? session.session_id}>
      <form onSubmit={onSubmit} className="flex flex-col gap-4">
        {parts.map((p, i) => (
          <div key={i} className="grid grid-cols-[5rem_1fr_1fr] items-end gap-2">
            <Field label={`Percent ${String(i + 1)}`}>
              <Input inputMode="decimal" value={p.percent} onChange={(e) => set(i, { percent: e.target.value })} />
            </Field>
            <Field label={`Tag name ${String(i + 1)}`}>
              <Select value={p.name} onChange={(e) => set(i, { name: e.target.value })}>
                {(tags.data ?? []).map((n) => <option key={n.name} value={n.name}>{n.name}</option>)}
              </Select>
            </Field>
            <Field label={`Tag value ${String(i + 1)}`}>
              <Input list={`keys-${String(i)}`} value={p.key} placeholder="none" onChange={(e) => set(i, { key: e.target.value })} />
              <TagValueList id={`keys-${String(i)}`} name={p.name} />
            </Field>
          </div>
        ))}
        <div className="flex flex-wrap items-center gap-2">
          <Button size="sm" onClick={() => setParts([...parts, { percent: "", name: "feature", key: "" }])}>Add a part</Button>
          {parts.length > 1 ? <Button size="sm" variant="danger" onClick={() => setParts(parts.slice(0, -1))}>Remove the last part</Button> : null}
          <span className={`ml-auto text-body ${total === 100 ? "text-muted" : "text-exception"}`}>{total} % of 100 %</span>
        </div>
        {parts.length === 1 ? <CheckField checked={remember} onCheckedChange={setRemember} label="Remember as my rule" /> : null}
        <p className="text-caption text-muted">The project and the client follow from the feature. The split supersedes the earlier one; the old one stays in the log.</p>
        <FormError error={save.error} />
        <div className="flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={save.isPending || total !== 100}>Save split</Button>
        </div>
      </form>
    </Dialog>
  );
}
