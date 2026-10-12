import { useMutation, useQuery } from "@tanstack/react-query";
import { getRouteApi, Link, useNavigate } from "@tanstack/react-router";
import { useState, type ReactNode } from "react";
import type { Conversation, ConversationView } from "@/api/contract";
import { api } from "@/api/endpoints";
import { AdminFrame } from "@/routes/admin";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ErrorBox, Loading, Note, Section } from "@/components/ui/display";
import { Field, FormError, Select, Textarea } from "@/components/ui/form";
import { DataTable, type Column } from "@/components/ui/table";
import { notify } from "@/components/ui/toast";
import { formatDateTime } from "@/lib/format";
import { refreshAll } from "@/lib/queries";

const listRoute = getRouteApi("/app/admin/support");
const conversationRoute = getRouteApi("/app/admin/support/$id");

export const supportStates = ["new", "answered", "escalated", "closed"] as const;

const channelText: Record<string, string> = { mail: "email", contact_form: "contact form", app: "web app" };
const authorText: Record<string, string> = { customer: "Customer", assistant: "AI assistant", operator: "Token Controller support" };

function StateBadge({ state }: { state: string }) {
  const tone = state === "escalated" ? "exception" : state === "new" ? "accent" : state === "answered" ? "posted" : "neutral";
  return <Badge tone={tone}>{state}</Badge>;
}

export function AdminSupportPage(): ReactNode {
  const { state } = listRoute.useSearch();
  const navigate = useNavigate();
  const conversations = useQuery({ queryKey: ["admin", "support", state ?? ""], queryFn: () => api.supportConversations(state) });
  const columns: Column<Conversation>[] = [
    { key: "state", header: "State", cell: (c) => <StateBadge state={c.state} />, csv: (c) => c.state },
    { key: "subject", header: "Subject", cell: (c) => <span className="font-medium">{c.subject}</span>, csv: (c) => c.subject },
    { key: "from", header: "From", cell: (c) => c.contact_name ?? c.contact_email, csv: (c) => c.contact_email },
    { key: "kind", header: "Kind", wide: true, cell: (c) => c.kind.replace("_", " "), csv: (c) => c.kind },
    { key: "channel", header: "Came by", wide: true, cell: (c) => channelText[c.channel] ?? c.channel, csv: (c) => c.channel },
    { key: "messages", header: "Messages", align: "right", wide: true, cell: (c) => c.messages, csv: (c) => c.messages },
    { key: "updated", header: "Last change", wide: true, cell: (c) => formatDateTime(c.updated_at), csv: (c) => c.updated_at },
  ];
  return (
    <AdminFrame
      title="Support"
      lead="Mail, the contact form and the web app. The AI assistant answers from the docs; what it escalates waits here, first."
      actions={
        <Select aria-label="Show" value={state ?? ""} onChange={(e) => void navigate({ to: "/admin/support", search: e.target.value ? { state: e.target.value } : {} })} className="w-auto">
          <option value="">Everything not closed</option>
          {supportStates.map((s) => (
            <option key={s} value={s}>
              {s}
            </option>
          ))}
        </Select>
      }
    >
      {conversations.isPending ? (
        <Loading />
      ) : conversations.isError ? (
        <ErrorBox error={conversations.error} />
      ) : (
        <DataTable
          stack
          columns={columns}
          rows={conversations.data}
          rowKey={(c) => c.id}
          caption="Support conversations"
          empty="Nothing waits."
          onRowClick={(c) => void navigate({ to: "/admin/support/$id", params: { id: c.id } })}
        />
      )}
    </AdminFrame>
  );
}

function Conversation({ view }: { view: ConversationView }) {
  const c = view.conversation;
  const [text, setText] = useState(view.suggested_answer ?? "");
  const reply = useMutation({
    mutationFn: () => api.replyToSupport(c.id, text.trim()),
    onSuccess: async () => {
      await refreshAll();
      setText("");
      notify("Reply saved", "It goes out by mail, signed Token Controller support.");
    },
  });
  const close = useMutation({ mutationFn: () => api.closeSupport(c.id), onSuccess: async () => { await refreshAll(); notify("Closed"); } });
  return (
    <>
      <div className="mb-6 flex flex-wrap items-center gap-2 text-[13px] text-muted">
        <StateBadge state={c.state} />
        <span>{c.kind.replace("_", " ")}</span>
        <span>·</span>
        <span>
          {c.contact_name ? `${c.contact_name}, ` : ""}
          {c.contact_email}
        </span>
        <span>·</span>
        <span>by {channelText[c.channel] ?? c.channel}</span>
        {view.customer ? (
          <>
            <span>·</span>
            <Link to="/admin/customers/$id" params={{ id: view.customer.id }} className="text-accent hover:underline">
              {view.customer.name}
            </Link>
          </>
        ) : null}
      </div>
      {view.escalation_reason ? <Note tone="exception">Escalated: {view.escalation_reason}</Note> : null}
      <ol className="mb-8 space-y-3" aria-label="Messages">
        {view.messages.map((m) => (
          <li key={m.id} className={`rounded-lg border p-4 ${m.author === "customer" ? "border-line" : "border-accent/30 bg-accent-tint/40"}`}>
            <div className="mb-1 flex flex-wrap gap-2 text-xs text-muted">
              <span className="font-medium text-ink-soft">{authorText[m.author] ?? m.author}</span>
              <span>{formatDateTime(m.created_at)}</span>
              {m.author !== "customer" ? <span>{m.sent_at ? `sent ${formatDateTime(m.sent_at)}` : "waiting to be sent"}</span> : null}
            </div>
            <p className="whitespace-pre-wrap text-sm">{m.body}</p>
            {m.links.length ? (
              <ul className="mt-2 flex flex-wrap gap-2 text-xs">
                {m.links.map((l) => (
                  <li key={l}>
                    <a href={l} target="_blank" rel="noreferrer" className="text-accent hover:underline">
                      {l.replace(/^https?:\/\//, "")}
                    </a>
                  </li>
                ))}
              </ul>
            ) : null}
          </li>
        ))}
      </ol>
      {c.state !== "closed" ? (
        <Section title="Reply">
          <form className="space-y-3" onSubmit={(e) => { e.preventDefault(); reply.mutate(); }}>
            <Field label="Answer" hint={view.suggested_answer ? "Prefilled with the assistant's proposal. Change what you like." : undefined}>
              <Textarea required rows={8} value={text} onChange={(e) => setText(e.target.value)} />
            </Field>
            <FormError error={reply.error ?? close.error} />
            <div className="flex flex-wrap justify-end gap-2">
              <Button onClick={() => close.mutate()} disabled={close.isPending}>
                Close without reply
              </Button>
              <Button type="submit" variant="primary" disabled={!text.trim() || reply.isPending}>
                Send reply
              </Button>
            </div>
          </form>
        </Section>
      ) : null}
    </>
  );
}

export function AdminConversationPage(): ReactNode {
  const { id } = conversationRoute.useParams();
  const view = useQuery({ queryKey: ["admin", "support", "one", id], queryFn: () => api.supportConversation(id) });
  return (
    <AdminFrame title={view.data?.conversation.subject ?? "Support conversation"}>
      {view.isPending ? <Loading /> : view.isError ? <ErrorBox error={view.error} /> : <Conversation key={view.data.conversation.id} view={view.data} />}
    </AdminFrame>
  );
}
