import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { Issue, NewsletterView } from "@/api/contract";
import { api } from "@/api/endpoints";
import { AdminFrame } from "@/routes/admin";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Kpi, KpiRow, Loading, Note, Section } from "@/components/ui/display";
import { Field, FormError, Input, Textarea } from "@/components/ui/form";
import { notify } from "@/components/ui/toast";
import { formatDateTime } from "@/lib/format";
import { refreshAll } from "@/lib/queries";

/** A new issue, or a draft being changed. */
function IssueForm({ issue, onDone }: { issue: Issue | null; onDone: () => void }) {
  const [subject, setSubject] = useState(issue?.subject ?? "");
  const [body, setBody] = useState(issue?.body ?? "");
  const save = useMutation({
    mutationFn: () => (issue ? api.updateIssue(issue.id, { subject: subject.trim(), body }) : api.createIssue(subject.trim(), body)),
    onSuccess: async () => {
      await refreshAll();
      notify(issue ? "Draft saved" : "Draft created");
      onDone();
    },
  });
  return (
    <form className="space-y-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
      <Field label="Subject">
        <Input required value={subject} onChange={(e) => setSubject(e.target.value)} />
      </Field>
      <Field label="Text" hint="Plain text. Every mail adds the one-click unsubscribe link below it.">
        <Textarea required rows={14} value={body} onChange={(e) => setBody(e.target.value)} className="font-mono text-[13px]" />
      </Field>
      <FormError error={save.error} />
      <div className="flex justify-end gap-2">
        <Button onClick={onDone}>Cancel</Button>
        <Button type="submit" variant="primary" disabled={!subject.trim() || !body.trim() || save.isPending}>
          Save draft
        </Button>
      </div>
    </form>
  );
}

function SendIssue({ issue, confirmed, onClose }: { issue: Issue; confirmed: number; onClose: () => void }) {
  const send = useMutation({
    mutationFn: () => api.sendIssue(issue.id),
    onSuccess: async () => {
      await refreshAll();
      notify("Sending", `To ${String(confirmed)} confirmed subscribers.`);
      onClose();
    },
  });
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()} title={`Send “${issue.subject}”?`} description={`It goes to every confirmed subscriber: ${String(confirmed)} people. A sent issue cannot be changed or called back.`}>
      <FormError error={send.error} />
      <div className="mt-4 flex justify-end gap-2">
        <Button onClick={onClose}>Cancel</Button>
        <Button variant="primary" disabled={send.isPending} onClick={() => send.mutate()}>
          Send to {confirmed} subscribers
        </Button>
      </div>
    </Dialog>
  );
}

function Issues({ view }: { view: NewsletterView }) {
  const [editing, setEditing] = useState<Issue | "new" | null>(null);
  const [sending, setSending] = useState<Issue | null>(null);
  return (
    <>
      <KpiRow>
        <Kpi label="Confirmed subscribers" value={view.confirmed} />
        <Kpi label="Waiting for confirmation" value={view.pending} note="deleted after 7 days" />
      </KpiRow>
      {!view.on ? <Note tone="exception">Sending is off: the service has no mail key for the newsletter yet. Drafts can still be written.</Note> : null}
      <Section title="Issues" lead="Newest first." actions={<Button variant="primary" onClick={() => setEditing("new")}>New issue</Button>}>
        {view.issues.length ? (
          <ul className="divide-y divide-line rounded-lg border border-line" aria-label="Issues">
            {view.issues.map((issue) => (
              <li key={issue.id} className="flex flex-col gap-2 px-4 py-3 sm:flex-row sm:items-center">
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <Badge tone={issue.status === "sent" ? "posted" : issue.status === "sending" ? "accent" : "neutral"}>{issue.status}</Badge>
                    <span className="truncate text-sm font-medium">{issue.subject}</span>
                  </div>
                  <div className="mt-0.5 text-xs text-muted">
                    {issue.status === "draft"
                      ? `written ${formatDateTime(issue.created_at)}`
                      : `${String(issue.delivered)} of ${String(issue.recipients)} delivered${issue.sent_at ? ` · sent ${formatDateTime(issue.sent_at)}` : ""}`}
                  </div>
                </div>
                {issue.status === "draft" ? (
                  <div className="flex gap-1">
                    <Button size="sm" variant="ghost" onClick={() => setEditing(issue)}>
                      Edit
                    </Button>
                    <Button size="sm" disabled={!view.on || !view.confirmed} onClick={() => setSending(issue)}>
                      Send
                    </Button>
                  </div>
                ) : null}
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-sm text-muted">No issues yet.</p>
        )}
      </Section>
      <Dialog open={editing !== null} onOpenChange={(o) => !o && setEditing(null)} title={editing === "new" ? "New issue" : "Edit draft"} wide>
        {editing !== null ? <IssueForm key={editing === "new" ? "new" : editing.id} issue={editing === "new" ? null : editing} onDone={() => setEditing(null)} /> : null}
      </Dialog>
      {sending ? <SendIssue issue={sending} confirmed={view.confirmed} onClose={() => setSending(null)} /> : null}
    </>
  );
}

export function AdminNewsletterPage(): ReactNode {
  const view = useQuery({ queryKey: ["admin", "newsletter"], queryFn: api.newsletter });
  return (
    <AdminFrame title="Newsletter" lead="People who signed up on the website and confirmed by email. Drafts first; sending goes to every confirmed subscriber.">
      {view.isPending ? <Loading /> : view.isError ? <ErrorBox error={view.error} /> : <Issues view={view.data} />}
    </AdminFrame>
  );
}
