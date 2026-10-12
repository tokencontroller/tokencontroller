import { useMutation, useQuery } from "@tanstack/react-query";
import { getRouteApi, Link, useNavigate } from "@tanstack/react-router";
import { useState, type ReactNode } from "react";
import type { FeedbackStatus, ReceivedReport, ReportChange } from "@/api/contract";
import { API_PREFIX } from "@/api/client";
import { api } from "@/api/endpoints";
import { kindText } from "@/components/feedback-panel";
import { AdminFrame } from "@/routes/admin";
import { ReportStatusBadge } from "@/routes/feedback";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ErrorBox, Loading, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select, Textarea } from "@/components/ui/form";
import { DataTable, type Column } from "@/components/ui/table";
import { notify } from "@/components/ui/toast";
import { formatDateTime } from "@/lib/format";
import { refreshAll, useAdmin } from "@/lib/queries";

const listRoute = getRouteApi("/app/admin/feedback");
const reportRoute = getRouteApi("/app/admin/feedback/$id");

export const feedbackStatuses: FeedbackStatus[] = ["received", "answered", "fixed"];

export function AdminFeedbackPage(): ReactNode {
  const { status } = listRoute.useSearch();
  const navigate = useNavigate();
  const operator = Boolean(useAdmin().data);
  const reports = useQuery({ queryKey: ["admin", "feedback", status ?? ""], queryFn: () => api.receivedFeedback(status) });
  const columns: Column<ReceivedReport>[] = [
    { key: "status", header: "Status", cell: (r) => <ReportStatusBadge report={r.report} />, csv: (r) => r.report.status },
    { key: "kind", header: "Kind", cell: (r) => kindText[r.report.kind], csv: (r) => r.report.kind },
    { key: "text", header: "Text", cell: (r) => <span className="line-clamp-2">{r.report.text}</span>, csv: (r) => r.report.text },
    { key: "from", header: "From", wide: true, cell: (r) => `${r.reporter.name || r.reporter.email}${operator ? `, ${r.organization.name}` : ""}`, csv: (r) => r.reporter.email },
    { key: "screenshot", header: "", wide: true, cell: (r) => (r.report.has_screenshot ? <Badge>screenshot</Badge> : null), csv: (r) => (r.report.has_screenshot ? "yes" : "") },
    { key: "created", header: "Sent", wide: true, cell: (r) => formatDateTime(r.report.created_at), csv: (r) => r.report.created_at },
  ];
  return (
    <AdminFrame
      title="Feedback reports"
      lead={operator ? "Bugs, ideas and questions from the feedback button, from every organization." : "Bugs, ideas and questions your organization's members sent with the feedback button."}
      actions={
        <Select aria-label="Show" value={status ?? ""} onChange={(e) => void navigate({ to: "/admin/feedback", search: e.target.value ? { status: e.target.value as FeedbackStatus } : {} })} className="w-auto">
          <option value="">All reports</option>
          {feedbackStatuses.map((s) => (
            <option key={s} value={s}>
              {s}
            </option>
          ))}
        </Select>
      }
    >
      {reports.isPending ? (
        <Loading />
      ) : reports.isError ? (
        <ErrorBox error={reports.error} />
      ) : (
        <DataTable
          stack
          columns={columns}
          rows={reports.data}
          rowKey={(r) => r.report.id}
          caption="Feedback reports"
          empty="No reports."
          onRowClick={(r) => void navigate({ to: "/admin/feedback/$id", params: { id: r.report.id } })}
        />
      )}
    </AdminFrame>
  );
}

function Report({ received }: { received: ReceivedReport }) {
  const operator = Boolean(useAdmin().data);
  const r = received.report;
  const [answer, setAnswer] = useState(r.answer ?? "");
  const [fixedIn, setFixedIn] = useState(r.fixed_in ?? "");
  const update = useMutation({
    mutationFn: (change: ReportChange) => api.updateReport(r.id, change),
    onSuccess: async () => {
      await refreshAll();
      notify("Saved", "The sender gets an email with a link to their report, never the answer itself.");
    },
  });
  const d = r.diagnostics;
  const diagnostics: [string, ReactNode][] = [
    ["Page", d.page ? <span className="font-mono">{d.page}</span> : "not sent"],
    ["App version", d.app_version || "not sent"],
    ["Browser", d.browser || "not sent"],
    ["Screen size", d.screen || "not sent"],
    ["Last error messages", d.errors?.length ? <ul className="list-disc pl-4">{d.errors.map((e, i) => <li key={i}>{e}</li>)}</ul> : "none"],
    ["Last request IDs", d.request_ids?.length ? <span className="font-mono text-xs">{d.request_ids.join(" ")}</span> : "none"],
  ];
  return (
    <>
      <div className="mb-4 flex flex-wrap items-center gap-2 text-[13px] text-muted">
        <Badge>{kindText[r.kind]}</Badge>
        <ReportStatusBadge report={r} />
        <span>{formatDateTime(r.created_at)}</span>
        <span>·</span>
        <span>
          {received.reporter.name || received.reporter.email}, {received.reporter.email}
        </span>
        <span>·</span>
        {operator ? (
          <Link to="/admin/customers/$id" params={{ id: received.organization.id }} className="text-accent hover:underline">
            {received.organization.name}
          </Link>
        ) : (
          <span>{received.organization.name}</span>
        )}
      </div>
      <p className="mb-6 whitespace-pre-wrap rounded-lg border border-line p-4 text-sm">{r.text}</p>
      <div className="grid gap-6 lg:grid-cols-2">
        <Section title="Screenshot">
          {r.has_screenshot ? (
            <a href={`${API_PREFIX}/feedback/${r.id}/screenshot`} target="_blank" rel="noreferrer">
              <img src={`${API_PREFIX}/feedback/${r.id}/screenshot`} alt="The screenshot sent with the report" className="w-full rounded-md border border-line" />
            </a>
          ) : (
            <p className="text-sm text-muted">None sent.</p>
          )}
        </Section>
        <Section title="Diagnostics">
          <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-1.5 text-sm">
            {diagnostics.map(([label, value]) => (
              <div key={label} className="contents">
                <dt className="text-muted">{label}</dt>
                <dd className="break-words">{value}</dd>
              </div>
            ))}
          </dl>
        </Section>
      </div>
      <Section title="Answer" lead="The sender reads it under Feedback in the web app. Their email only links there.">
        <form className="space-y-3" onSubmit={(e) => { e.preventDefault(); update.mutate({ answer: answer.trim() }); }}>
          <Textarea aria-label="Answer" rows={5} value={answer} onChange={(e) => setAnswer(e.target.value)} />
          <div className="flex justify-end">
            <Button type="submit" variant="primary" disabled={!answer.trim() || answer.trim() === (r.answer ?? "") || update.isPending}>
              Save answer
            </Button>
          </div>
        </form>
      </Section>
      <Section title="Fixed" lead="The version that has the fix. The report then shows as fixed.">
        <form className="flex max-w-sm items-end gap-2" onSubmit={(e) => { e.preventDefault(); update.mutate({ fixed_in: fixedIn.trim() }); }}>
          <Field label="Fixed in version" className="flex-1">
            <Input required pattern="\d+\.\d+\.\d+" title="A version such as 1.0.1" value={fixedIn} onChange={(e) => setFixedIn(e.target.value)} placeholder="1.0.1" />
          </Field>
          <Button type="submit" disabled={!fixedIn.trim() || update.isPending}>
            Mark fixed
          </Button>
        </form>
      </Section>
      <FormError error={update.error} />
    </>
  );
}

export function AdminReportPage(): ReactNode {
  const { id } = reportRoute.useParams();
  const report = useQuery({ queryKey: ["admin", "feedback", "one", id], queryFn: () => api.receivedReport(id) });
  return (
    <AdminFrame title="Feedback report">
      {report.isPending ? <Loading /> : report.isError ? <ErrorBox error={report.error} /> : <Report key={report.data.report.id} received={report.data} />}
    </AdminFrame>
  );
}
