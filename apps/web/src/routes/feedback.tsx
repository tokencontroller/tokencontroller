import type { ReactNode } from "react";
import type { FeedbackStatus, Report } from "@/api/contract";
import { FeedbackButton, kindText } from "@/components/feedback-panel";
import { Badge } from "@/components/ui/badge";
import { buttonClass } from "@/components/ui/button";
import { Empty, ErrorBox, Loading, PageHeader } from "@/components/ui/display";
import { formatDate } from "@/lib/format";
import { useFeedback } from "@/lib/queries";
import { NotFoundPage } from "@/routes/not-found";

export function statusText(report: Pick<Report, "status" | "fixed_in">): string {
  const text: Record<FeedbackStatus, string> = { received: "received", answered: "answered", fixed: report.fixed_in ? `fixed in version ${report.fixed_in}` : "fixed" };
  return text[report.status];
}

export function ReportStatusBadge({ report }: { report: Pick<Report, "status" | "fixed_in"> }) {
  const tone = report.status === "received" ? "neutral" : report.status === "answered" ? "accent" : "posted";
  return <Badge tone={tone}>{statusText(report)}</Badge>;
}

/**
 * "My feedback": the reports a member sent and the answers. The service mails
 * a link here. Where the service has no feedback reports, the page is not there.
 */
export function FeedbackPage(): ReactNode {
  const reports = useFeedback();
  if (reports.data === null) return <NotFoundPage />;
  return (
    <>
      <PageHeader
        title="Feedback"
        lead="Your bug reports, ideas and questions, and the answers. When one is answered or fixed, you get an email with a link here."
        actions={<FeedbackButton className={buttonClass("primary")}>Send feedback</FeedbackButton>}
      />
      {reports.isPending ? (
        <Loading />
      ) : reports.isError ? (
        <ErrorBox error={reports.error} />
      ) : !reports.data.length ? (
        <Empty title="No feedback yet">The feedback button sends a bug, an idea or a question.</Empty>
      ) : (
        <ul className="space-y-3" aria-label="Your feedback">
          {reports.data.map((r) => (
            <li key={r.id} className="rounded-lg border border-line p-4">
              <div className="mb-2 flex flex-wrap items-center gap-2 text-[13px] text-muted">
                <Badge>{kindText[r.kind]}</Badge>
                <ReportStatusBadge report={r} />
                <span>{formatDate(r.created_at)}</span>
                {r.diagnostics.page ? <span className="font-mono">{r.diagnostics.page}</span> : null}
              </div>
              <p className="whitespace-pre-wrap text-sm">{r.text}</p>
              {r.answer ? (
                <div className="mt-3 rounded-md bg-sheet p-3 text-sm">
                  <div className="mb-1 text-xs font-medium text-muted">Answer</div>
                  <p className="whitespace-pre-wrap">{r.answer}</p>
                </div>
              ) : null}
            </li>
          ))}
        </ul>
      )}
    </>
  );
}
