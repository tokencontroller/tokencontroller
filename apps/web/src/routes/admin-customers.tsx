import { useMutation, useQuery } from "@tanstack/react-query";
import { getRouteApi, Link, useNavigate } from "@tanstack/react-router";
import { useState, type ReactNode } from "react";
import type { CustomerMember, CustomerRecord, CustomerSummary } from "@/api/contract";
import { api } from "@/api/endpoints";
import { kindText } from "@/components/feedback-panel";
import { AdminFrame } from "@/routes/admin";
import { ReportStatusBadge } from "@/routes/feedback";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ErrorBox, Kpi, KpiRow, Loading, Section } from "@/components/ui/display";
import { FormError, Textarea } from "@/components/ui/form";
import { DataTable, type Column } from "@/components/ui/table";
import { formatDate, formatDateTime } from "@/lib/format";
import { refreshAll } from "@/lib/queries";
import { roleLabel } from "@/lib/roles";

const customerRoute = getRouteApi("/app/admin/customers/$id");

const planText = (plan: CustomerSummary["plan"]) => (plan === "team" ? "Team plan" : plan === "solo" ? "Solo" : "no billing");

export function AdminCustomersPage(): ReactNode {
  const navigate = useNavigate();
  const customers = useQuery({ queryKey: ["admin", "customers"], queryFn: api.customers });
  const columns: Column<CustomerSummary>[] = [
    { key: "name", header: "Organization", cell: (c) => <span className="font-medium">{c.name}</span>, csv: (c) => c.name },
    { key: "plan", header: "Plan", cell: (c) => <Badge tone={c.plan === "team" ? "posted" : "neutral"}>{planText(c.plan)}</Badge>, csv: (c) => c.plan ?? "" },
    { key: "members", header: "Members", align: "right", cell: (c) => c.members, csv: (c) => c.members },
    { key: "support", header: "Open support", align: "right", wide: true, cell: (c) => c.open_support || "", csv: (c) => c.open_support },
    { key: "feedback", header: "Open feedback", align: "right", wide: true, cell: (c) => c.open_feedback || "", csv: (c) => c.open_feedback },
    { key: "since", header: "Since", wide: true, cell: (c) => formatDate(c.created_at), csv: (c) => c.created_at },
  ];
  return (
    <AdminFrame title="Customers" lead="One record per organization: plan, members, support, feedback and notes.">
      {customers.isPending ? (
        <Loading />
      ) : customers.isError ? (
        <ErrorBox error={customers.error} />
      ) : (
        <DataTable
          stack
          columns={columns}
          rows={customers.data}
          rowKey={(c) => c.id}
          caption="Customers"
          csvName="customers.csv"
          empty="No organizations yet."
          onRowClick={(c) => void navigate({ to: "/admin/customers/$id", params: { id: c.id } })}
        />
      )}
    </AdminFrame>
  );
}

function Notes({ record }: { record: CustomerRecord }) {
  const [text, setText] = useState("");
  const add = useMutation({
    mutationFn: () => api.addCustomerNote(record.customer.id, text.trim()),
    onSuccess: async () => {
      await refreshAll();
      setText("");
    },
  });
  return (
    <Section title="Notes" lead="Only operators see them.">
      {record.notes.length ? (
        <ul className="mb-3 space-y-2">
          {record.notes.map((n) => (
            <li key={n.id} className="rounded-md bg-sheet p-3 text-sm">
              <div className="mb-1 text-xs text-muted">
                {n.author_email} · {formatDateTime(n.created_at)}
              </div>
              <p className="whitespace-pre-wrap">{n.body}</p>
            </li>
          ))}
        </ul>
      ) : null}
      <form className="space-y-2" onSubmit={(e) => { e.preventDefault(); add.mutate(); }}>
        <Textarea aria-label="New note" rows={3} value={text} onChange={(e) => setText(e.target.value)} placeholder="Such as: asked about the Enterprise plan for 200 people." />
        <FormError error={add.error} />
        <div className="flex justify-end">
          <Button type="submit" disabled={!text.trim() || add.isPending}>
            Add note
          </Button>
        </div>
      </form>
    </Section>
  );
}

function Customer({ record }: { record: CustomerRecord }) {
  const c = record.customer;
  const s = record.subscription;
  const members: Column<CustomerMember>[] = [
    { key: "name", header: "Name", cell: (m) => <span className={m.removed_at ? "text-muted line-through" : "font-medium"}>{m.name || m.email}</span>, csv: (m) => m.name },
    { key: "email", header: "Email", wide: true, cell: (m) => m.email, csv: (m) => m.email },
    { key: "role", header: "Role", cell: (m) => roleLabel[m.role], csv: (m) => m.role },
    { key: "state", header: "", cell: (m) => (m.removed_at ? <Badge>removed</Badge> : !m.joined_at ? <Badge tone="accent">invited</Badge> : null), csv: (m) => (m.removed_at ? "removed" : m.joined_at ? "joined" : "invited") },
    { key: "newsletter", header: "Newsletter", wide: true, cell: (m) => m.newsletter ?? "", csv: (m) => m.newsletter ?? "" },
  ];
  return (
    <>
      <KpiRow>
        <Kpi label="Plan" value={planText(c.plan)} note={s ? [s.status, s.interval === "year" ? "yearly" : s.interval === "month" ? "monthly" : null, s.current_period_end ? `${s.cancel_at_period_end ? "ends" : "renews"} ${formatDate(s.current_period_end)}` : null].filter(Boolean).join(" · ") : "never started a checkout"} />
        <Kpi label="Members" value={c.members} note={`since ${formatDate(c.created_at)}`} />
        <Kpi label="Open support" value={c.open_support} />
        <Kpi label="Open feedback" value={c.open_feedback} />
      </KpiRow>
      <p className="-mt-4 mb-8 text-[13px] text-muted">
        <span className="font-mono">{record.slug}</span> · reports in {record.reporting_currency}
      </p>
      <Section title="Members">
        <DataTable stack columns={members} rows={record.members} rowKey={(m) => m.id} caption="Members" />
      </Section>
      <div className="grid gap-6 lg:grid-cols-2">
        <Section title="Support">
          {record.support.length ? (
            <ul className="divide-y divide-line rounded-lg border border-line">
              {record.support.map((t) => (
                <li key={t.id}>
                  <Link to="/admin/support/$id" params={{ id: t.id }} className="block px-4 py-2.5 text-sm hover:bg-sand">
                    <span className="font-medium">{t.subject}</span>
                    <span className="block text-xs text-muted">
                      {t.state} · {formatDate(t.updated_at)}
                    </span>
                  </Link>
                </li>
              ))}
            </ul>
          ) : (
            <p className="text-sm text-muted">No conversations.</p>
          )}
        </Section>
        <Section title="Feedback">
          {record.feedback.length ? (
            <ul className="divide-y divide-line rounded-lg border border-line">
              {record.feedback.map(({ report: r }) => (
                <li key={r.id}>
                  <Link to="/admin/feedback/$id" params={{ id: r.id }} className="flex items-center gap-2 px-4 py-2.5 text-sm hover:bg-sand">
                    <Badge>{kindText[r.kind]}</Badge>
                    <span className="min-w-0 flex-1 truncate">{r.text}</span>
                    <ReportStatusBadge report={r} />
                  </Link>
                </li>
              ))}
            </ul>
          ) : (
            <p className="text-sm text-muted">No reports.</p>
          )}
        </Section>
      </div>
      <Notes record={record} />
    </>
  );
}

export function AdminCustomerPage(): ReactNode {
  const { id } = customerRoute.useParams();
  const record = useQuery({ queryKey: ["admin", "customers", id], queryFn: () => api.customer(id) });
  return (
    <AdminFrame title={record.data?.customer.name ?? "Customer"}>
      {record.isPending ? <Loading /> : record.isError ? <ErrorBox error={record.error} /> : <Customer record={record.data} />}
    </AdminFrame>
  );
}
