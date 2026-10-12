import { useQuery } from "@tanstack/react-query";
import { Link, useRouterState } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { api } from "@/api/endpoints";
import { Badge } from "@/components/ui/badge";
import { Empty, ErrorBox, Kpi, KpiRow, Loading, PageHeader } from "@/components/ui/display";
import { formatDateTime } from "@/lib/format";
import { useAdmin } from "@/lib/queries";

// The admin area is for the operators of the hosted service: the people in
// TC_OPERATOR_EMAILS. The service answers 404 to everyone else, which the
// pages show as it is.

const sections = [
  { to: "/admin", label: "Overview" },
  { to: "/admin/support", label: "Support" },
  { to: "/admin/feedback", label: "Feedback" },
  { to: "/admin/customers", label: "Customers" },
  { to: "/admin/questions", label: "Docs gaps" },
  { to: "/admin/newsletter", label: "Newsletter" },
] as const;

/** The admin pages' own menu, and their header. */
export function AdminFrame({ title, lead, actions, children }: { title: string; lead?: ReactNode; actions?: ReactNode; children: ReactNode }) {
  const path = useRouterState({ select: (s) => s.location.pathname });
  const admin = useAdmin();
  // Without operators, a controller reads their own organization's feedback here, and nothing else.
  const operator = Boolean(admin.data);
  return (
    <>
      {operator ? (
        <nav aria-label="Admin" className="-mx-4 mb-6 overflow-x-auto px-4 sm:mx-0 sm:px-0">
          <ul className="flex gap-1 border-b border-line">
            {sections.map((s) => {
              const active = s.to === "/admin" ? path === "/admin" : path.startsWith(s.to);
              return (
                <li key={s.to}>
                  <Link
                    to={s.to}
                    aria-current={active ? "page" : undefined}
                    className={`relative -mb-px block whitespace-nowrap border-b-2 px-3 py-2 text-sm font-medium ${active ? "border-ink text-ink" : "border-transparent text-muted hover:text-ink"}`}
                  >
                    {s.label}
                  </Link>
                </li>
              );
            })}
          </ul>
        </nav>
      ) : null}
      <PageHeader title={title} lead={lead} actions={actions} />
      {children}
    </>
  );
}

export function AdminPage(): ReactNode {
  const admin = useAdmin();
  return (
    <AdminFrame title="Admin" lead="What waits for the operators of Token Controller.">
      {admin.isPending ? (
        <Loading />
      ) : admin.isError ? (
        <ErrorBox error={admin.error} />
      ) : !admin.data ? (
        <ErrorBox error={new Error("This page is for the operators of the service.")} />
      ) : (
        <KpiRow>
          <Kpi label="Support that needs a person" value={admin.data.support_escalated} tone={admin.data.support_escalated ? "exception" : undefined} note={<Link to="/admin/support" className="text-accent hover:underline">Support</Link>} />
          <Kpi label="Feedback not answered" value={admin.data.feedback_received} tone={admin.data.feedback_received ? "exception" : undefined} note={<Link to="/admin/feedback" search={{ status: "received" }} className="text-accent hover:underline">Feedback</Link>} />
          <Kpi label="Docs gaps, last 7 days" value={admin.data.assistant_missed_7_days} note={<Link to="/admin/questions" className="text-accent hover:underline">Questions the docs missed</Link>} />
          <Kpi label="Newsletter subscribers" value={admin.data.newsletter_subscribers} note={<Link to="/admin/newsletter" className="text-accent hover:underline">Newsletter</Link>} />
        </KpiRow>
      )}
    </AdminFrame>
  );
}

/** Questions the site assistant could not answer from the docs: the gaps to write. */
export function AdminQuestionsPage(): ReactNode {
  const questions = useQuery({ queryKey: ["admin", "questions"], queryFn: () => api.assistantQuestions(false) });
  return (
    <AdminFrame title="Docs gaps" lead="Questions on the website the docs did not answer. Each one is a page or a paragraph to write.">
      {questions.isPending ? (
        <Loading />
      ) : questions.isError ? (
        <ErrorBox error={questions.error} />
      ) : !questions.data.length ? (
        <Empty title="No gaps">The docs answered every question.</Empty>
      ) : (
        <ul className="divide-y divide-line rounded-lg border border-line" aria-label="Questions the docs missed">
          {questions.data.map((q) => (
            <li key={q.id} className="px-4 py-3">
              <div className="flex flex-wrap items-center gap-2 text-[13px] text-muted">
                <span>{formatDateTime(q.created_at)}</span>
                {q.page ? <Badge>{q.page}</Badge> : null}
              </div>
              <p className="mt-1 text-sm font-medium">{q.question}</p>
              <p className="mt-1 whitespace-pre-wrap text-[13px] text-muted">{q.answer}</p>
            </li>
          ))}
        </ul>
      )}
    </AdminFrame>
  );
}
