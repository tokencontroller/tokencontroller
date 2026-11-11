import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { Submission } from "@/api/contract";
import { api } from "@/api/endpoints";
import { Amount } from "@/components/controls";
import { Badge, StatusBadge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Loading, Note, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Textarea } from "@/components/ui/form";
import { DataTable } from "@/components/ui/table";
import { notify } from "@/components/ui/toast";
import { formatDate, formatRange } from "@/lib/format";
import { refreshAll, useMe, usePersonName } from "@/lib/queries";
import { hasRole } from "@/lib/roles";
import { today } from "@/lib/today";

// Docs: Submit and approval. A submission is all of a person's messages
// inside a window. With approval on, a people manager approves it or rejects
// it with a reason; never their own.
export function SubmissionsPage(): ReactNode {
  const me = useMe();
  const submissions = useQuery({ queryKey: ["submissions"], queryFn: api.submissions });
  const requests = useQuery({ queryKey: ["submit-requests"], queryFn: api.submitRequests });
  const personName = usePersonName();
  const [rejecting, setRejecting] = useState<Submission | null>(null);
  const [asking, setAsking] = useState(false);
  const approval = me.organization.approval;
  const mayApprove = approval && hasRole(me.person, "people_manager");
  const approve = useMutation({
    mutationFn: (id: string) => api.approve(id),
    onSuccess: async () => {
      await refreshAll();
      notify("Approved");
    },
  });
  // What waits for a decision comes first; settled rows are muted.
  const order: Record<string, number> = { submitted: 0, rejected: 1, open: 1, approved: 2 };
  const rows = submissions.data?.toSorted((a, b) => (order[a.status] ?? 1) - (order[b.status] ?? 1) || b.to.localeCompare(a.to));
  const waiting = submissions.data?.filter((s) => s.status === "submitted").length ?? 0;
  return (
    <>
      <PageHeader
        title="Submissions"
        lead={<Badge tone={approval ? "accent" : "neutral"}>{approval ? "Approval on" : "Approval off"}</Badge>}
        actions={hasRole(me.person, "org_manager") ? <Button variant="primary" onClick={() => setAsking(true)}>Ask everyone to submit</Button> : null}
      />
      {requests.data?.length ? (
        <Note>
          {requests.data.map((r) => `Asked to submit by ${formatDate(r.until)}${r.message ? `: ${r.message}` : ""}`).join(" · ")}
        </Note>
      ) : null}
      <Section
        title={mayApprove ? "To approve" : "Submitted"}
        badge={mayApprove && waiting ? <Badge tone="accent">{waiting}</Badge> : null}
        hint={approval ? "Reports can show approved messages only." : "Submitted messages count in reports as they are."}
        flush
      >
        {submissions.error ? <div className="px-5 pb-5"><ErrorBox error={submissions.error} /></div> : null}
        {!rows ? (
          <div className="px-5 pb-5"><Loading /></div>
        ) : (
          <DataTable
            caption="Submissions"
            stack
            rows={rows}
            rowKey={(s) => s.id}
            rowClassName={(s) => (s.status === "submitted" ? "" : "text-ink-soft")}
            empty="Nothing was submitted yet."
            columns={[
              { key: "person", header: "Person", cell: (s) => <span className="font-medium text-ink">{personName(s.person)}</span> },
              { key: "window", header: "Window", cell: (s) => formatRange(s.from, s.to) },
              { key: "messages", header: "Messages", align: "right", wide: true, cell: (s) => <span className="num">{s.messages}</span> },
              { key: "cost", header: "Cost", align: "right", cell: (s) => <Amount value={s.cost} /> },
              { key: "status", header: "Status", cell: (s) => <span className="inline-flex flex-col items-end gap-0.5 sm:items-start"><StatusBadge status={s.status} />{s.reason ? <span className="text-caption text-muted">{s.reason}</span> : null}</span> },
              {
                key: "act",
                header: "",
                align: "right",
                cell: (s) =>
                  mayApprove && s.status === "submitted" && s.person !== me.person.email ? (
                    <span className="inline-flex gap-2">
                      <Button size="sm" variant="danger" onClick={() => setRejecting(s)} aria-label={`Reject ${personName(s.person)}`}>Reject</Button>
                      <Button size="sm" onClick={() => approve.mutate(s.id)} aria-label={`Approve ${personName(s.person)}`}>Approve</Button>
                    </span>
                  ) : null,
              },
            ]}
          />
        )}
        {approve.error ? <div className="px-5 py-3"><FormError error={approve.error} /></div> : null}
      </Section>
      {rejecting ? <RejectDialog submission={rejecting} onClose={() => setRejecting(null)} /> : null}
      <AskDialog open={asking} onClose={() => setAsking(false)} />
    </>
  );
}

function RejectDialog({ submission, onClose }: { submission: Submission; onClose: () => void }) {
  const [reason, setReason] = useState("");
  const reject = useMutation({
    mutationFn: () => api.reject(submission.id, reason),
    onSuccess: async () => {
      await refreshAll();
      notify("Rejected", "It goes back to their check list.");
      onClose();
    },
  });
  return (
    <Dialog open onOpenChange={(o) => { if (!o) onClose(); }} title="Reject the submission" description="Say what to fix. It goes back to its person's check list.">
      <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); reject.mutate(); }}>
        <Field label="Reason">
          <Textarea value={reason} onChange={(e) => setReason(e.target.value)} />
        </Field>
        <FormError error={reject.error} />
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="danger" disabled={reject.isPending}>Reject</Button>
        </div>
      </form>
    </Dialog>
  );
}

function AskDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [until, setUntil] = useState(today());
  const [message, setMessage] = useState("");
  const ask = useMutation({
    mutationFn: () => api.askToSubmit(until, message),
    onSuccess: async () => {
      await refreshAll();
      notify("Asked everyone to submit");
      onClose();
    },
  });
  return (
    <Dialog open={open} onOpenChange={(o) => { if (!o) onClose(); }} title="Ask everyone to submit" description="Everyone sees the request and gets one email. Nobody is blocked.">
      <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); ask.mutate(); }}>
        <Field label="Submit up to"><Input type="date" value={until} onChange={(e) => setUntil(e.target.value)} /></Field>
        <Field label="Message"><Input value={message} onChange={(e) => setMessage(e.target.value)} /></Field>
        <FormError error={ask.error} />
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={ask.isPending}>Ask</Button>
        </div>
      </form>
    </Dialog>
  );
}
