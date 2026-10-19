import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode, type SubmitEvent } from "react";
import type { NewRule, Session, Tags } from "@/api/contract";
import { api } from "@/api/endpoints";
import { Amount } from "@/components/controls";
import { Badge, LabelBadge, StatusBadge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { CheckField } from "@/components/ui/checkbox";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Loading, Note, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select } from "@/components/ui/form";
import { DataTable } from "@/components/ui/table";
import { TagRow, TagValueList } from "@/components/ui/tag";
import { notify } from "@/components/ui/toast";
import { formatDay, formatFraction, formatTime } from "@/lib/format";
import { refreshAll, useMe, usePersonName, useTags } from "@/lib/queries";
import { hasRole } from "@/lib/roles";

// Docs: Check and submit. Every session since the last submit, how it was
// split and why; fix the exceptions with a split, remember it as a rule, submit.
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
  return (
    <>
      <PageHeader
        title="Check and submit"
        lead="Your sessions and how each was split. Fix what a rule got wrong, then submit everything since your last submit."
        actions={
          <Button variant="primary" onClick={() => submit.mutate()} disabled={submit.isPending || !open.length}>
            Submit {open.length ? `${String(open.length)} sessions` : ""}
          </Button>
        }
      />
      {request ? <Note>Asked to submit by {formatDay(request.until)}{request.message ? `: ${request.message}` : "."}</Note> : null}
      <FormError error={submit.error} />
      {hasRole(me.person, "people_manager") ? (
        <div className="mb-4">
          <CheckField checked={everyone} onCheckedChange={setEveryone} label="Everyone I may view" />
        </div>
      ) : null}
      <Section title="Sessions">
        {sessions.error ? <ErrorBox error={sessions.error} /> : null}
        {!sessions.data ? (
          <Loading />
        ) : (
          <DataTable
            caption="Sessions"
            stack
            rows={rows}
            rowKey={(s) => s.session_id}
            empty="No sessions yet."
            columns={[
              { key: "when", header: "When", cell: (s) => `${formatDay(s.started_at)} ${formatTime(s.started_at)}`, csv: (s) => s.started_at },
              ...(everyone ? [{ key: "person", header: "Person", cell: (s: Session) => personName(s.person), csv: (s: Session) => s.person }] : []),
              { key: "summary", header: "Summary", cell: (s) => <span className="block max-w-xs">{s.summary}{s.agent_name ? <span className="text-muted"> · {s.agent_name}</span> : null}</span>, csv: (s) => s.summary ?? "" },
              {
                key: "cost",
                header: "Cost",
                align: "right",
                cell: (s) => (
                  <span className="flex flex-col items-end gap-0.5">
                    <Amount value={s.cost} />
                    {s.count_errors ? <Badge tone="exception">{s.count_errors} count error{s.count_errors > 1 ? "s" : ""}</Badge> : null}
                    {s.unknown_models.length ? <Badge tone="exception">unknown model</Badge> : null}
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
                        {s.parts.length > 1 ? <span className="num text-xs text-muted">{formatFraction(p.fraction)}</span> : null}
                        {Object.keys(p.assigned).length ? <TagRow tags={p.assigned as Tags} /> : <span className="text-xs text-muted">no project</span>}
                        <LabelBadge label={p.label} />
                      </span>
                    ))}
                  </span>
                ),
              },
              { key: "by", header: "By", wide: true, cell: (s) => (s.assigned_by === "default" ? "no rule matched" : s.assigned_by) },
              { key: "status", header: "Status", cell: (s) => <StatusBadge status={s.submitted} /> },
              { key: "fix", header: "", cell: (s) => <Button size="sm" onClick={() => setEditing(s)} aria-label={`Split ${s.summary ?? s.session_id}`}>Split</Button> },
            ]}
          />
        )}
      </Section>
      {editing ? <SplitDialog session={editing} onClose={() => setEditing(null)} /> : null}
    </>
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
  const first = (session.parts[0]?.assigned ?? {}) as Tags;
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
          {parts.length > 1 ? <Button size="sm" variant="ghost" onClick={() => setParts(parts.slice(0, -1))}>Remove the last part</Button> : null}
          <span className={`ml-auto text-sm ${total === 100 ? "text-muted" : "text-exception"}`}>{total} % of 100 %</span>
        </div>
        {parts.length === 1 ? <CheckField checked={remember} onCheckedChange={setRemember} label="Remember as my rule" /> : null}
        <p className="text-xs text-muted">The project and the client follow from the feature. The split supersedes the earlier one; the old one stays in the log.</p>
        <FormError error={save.error} />
        <div className="flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={save.isPending || total !== 100}>Save split</Button>
        </div>
      </form>
    </Dialog>
  );
}
