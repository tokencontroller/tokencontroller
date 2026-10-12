// Mock answers for feedback reports, support inside the app and the admin
// area (tokencontroller#45). The operators are the people in
// TC_OPERATOR_EMAILS; the mock starts with none, as a self-hosted instance
// does, and `POST /__mock/operator` makes the signed-in person one.

import { http, HttpResponse, type HttpResponseResolver } from "msw";
import type {
  AdminOverview,
  Conversation,
  ConversationView,
  CustomerNote,
  CustomerRecord,
  CustomerSummary,
  Diagnostics,
  FeedbackKind,
  FeedbackStatus,
  Issue,
  NewReport,
  Person,
  ReceivedReport,
  Report,
  ReportChange,
  Screenshot,
  SupportMessage,
} from "@/api/contract";
import type { Db } from "./seed";

export interface ReportRecord extends Report {
  person_id: string;
  screenshot: Screenshot | null;
}

export interface ConversationRecord extends Conversation {
  customer_id: string | null;
  escalation_reason: string | null;
  suggested_answer: string | null;
  thread: SupportMessage[];
}

/** Organizations other than the one signed in to, as the operators see them. */
interface OtherCustomer extends CustomerSummary {
  slug: string;
  reporting_currency: string;
}

export interface CustomerRelations {
  operators: string[];
  reports: ReportRecord[];
  support: ConversationRecord[];
  notes: (CustomerNote & { customer_id: string })[];
  others: OtherCustomer[];
  questions: { id: string; question: string; answer: string; answered: boolean; page: string | null; links: string[]; created_at: string }[];
  newsletter: { on: boolean; confirmed: number; pending: number; issues: Issue[] };
}

const noDiagnostics: Required<Diagnostics> = { page: "", app_version: "", browser: "", screen: "", errors: [], request_ids: [] };

export function createCustomerRelations(people: Person[], organizationId: string): CustomerRelations {
  const person = (email: string) => people.find((p) => p.email === email);
  const mara = person("m.ortner@kestrel.example");
  const priya = person("p.shah@kestrel.example");
  const kai = person("k.novak@kestrel.example");
  const report = (id: number, who: Person | undefined, kind: FeedbackKind, text: string, page: string, at: string, answer: string | null = null): ReportRecord => ({
    id: `fb000000-0000-4000-8000-00000000000${String(id)}`,
    person_id: who?.id ?? "",
    kind,
    text,
    status: answer ? "answered" : "received",
    answer,
    fixed_in: null,
    has_screenshot: false,
    screenshot: null,
    diagnostics: { ...noDiagnostics, page, app_version: "1.0.0", browser: "Chrome 141", screen: "1512×982" },
    created_at: at,
    updated_at: at,
  });
  return {
    operators: [],
    reports: [
      report(1, mara, "bug", "On my phone the ticket list in the token entry dialog cuts off after KD-240, so I cannot pick CP-12.", "/review", "2026-10-11T15:02:00Z"),
      report(2, priya, "idea", "Show the token budget of a ticket next to it in Approval, so I see right away when a ticket runs over.", "/approval", "2026-10-09T09:30:00Z", "Thank you. Token budget vs actual has it per ticket today; we are looking at a column in Approval."),
    ],
    support: [
      {
        id: "5c000000-0000-4000-8000-000000000001",
        channel: "contact_form",
        contact_email: "l.marx@northwind.example",
        contact_name: "Lea Marx",
        subject: "Enterprise plan for 220 people",
        kind: "sales",
        state: "escalated",
        messages: 1,
        created_at: "2026-10-11T18:40:00Z",
        updated_at: "2026-10-11T18:41:00Z",
        customer_id: "c5000000-0000-4000-8000-000000000002",
        escalation_reason: "Asks for a price and a call; the docs do not answer that.",
        suggested_answer: "Hello Lea,\n\nthank you for writing. The Enterprise plan is for organizations above 150 members, so Northwind Studio fits it. Can you tell me where your instance should run and whether you need OIDC and SCIM?\n\nJan",
        thread: [
          { id: "5c000000-0000-4000-8001-000000000001", author: "customer", body: "We are on the Team plan and grow to about 220 people this winter. What does the Enterprise plan cost, and can we talk this week?", links: [], created_at: "2026-10-11T18:40:00Z", sent_at: null },
        ],
      },
      {
        id: "5c000000-0000-4000-8000-000000000002",
        channel: "app",
        contact_email: kai?.email ?? "",
        contact_name: kai?.name ?? null,
        subject: "Hand over a CI token",
        kind: "question",
        state: "answered",
        messages: 2,
        created_at: "2026-10-10T11:12:00Z",
        updated_at: "2026-10-10T11:14:00Z",
        customer_id: organizationId,
        escalation_reason: null,
        suggested_answer: null,
        thread: [
          { id: "5c000000-0000-4000-8001-000000000002", author: "customer", body: "Felix left and his release-notes bot still reports. How do I give it to someone else?", links: [], created_at: "2026-10-10T11:12:00Z", sent_at: null },
          {
            id: "5c000000-0000-4000-8001-000000000003",
            author: "assistant",
            body: "A controller moves the bot's service token to another person under Settings > API tokens. Only its future token entries follow.\n\nThis answer comes from an AI assistant that reads the docs.",
            links: ["https://docs.tokencontroller.com/account/roles"],
            created_at: "2026-10-10T11:14:00Z",
            sent_at: "2026-10-10T11:14:30Z",
          },
        ],
      },
    ],
    notes: [{ id: "a0000000-0000-4000-8000-000000000001", customer_id: "c5000000-0000-4000-8000-000000000002", author_email: "a.operator@example.com", body: "Came through a LinkedIn post. Team plan since September.", created_at: "2026-10-01T10:00:00Z" }],
    others: [
      { id: "c5000000-0000-4000-8000-000000000002", name: "Northwind Studio", slug: "northwind", reporting_currency: "EUR", plan: "team", members: 138, open_support: 1, open_feedback: 0, created_at: "2026-09-02T12:00:00Z" },
      { id: "c5000000-0000-4000-8000-000000000003", name: "Harbor Labs", slug: "harbor", reporting_currency: "USD", plan: "solo", members: 1, open_support: 0, open_feedback: 0, created_at: "2026-10-08T07:30:00Z" },
    ],
    questions: [
      { id: "9a000000-0000-4000-8000-000000000001", question: "Does Token Controller read Cursor's team usage export?", answer: "The docs do not say. Cursor sessions arrive through the CLI; a bill from Cursor can be typed in or uploaded as CSV.", answered: false, page: "/integrations", links: [], created_at: "2026-10-11T20:15:00Z" },
      { id: "9a000000-0000-4000-8000-000000000002", question: "Can a people manager see prompts?", answer: "The docs do not answer this directly. The core drops prompt text, so nobody sees prompts in Token Controller.", answered: false, page: "/", links: [], created_at: "2026-10-10T08:05:00Z" },
    ],
    newsletter: {
      on: true,
      confirmed: 42,
      pending: 3,
      issues: [{ id: "15500000-0000-4000-8000-000000000001", subject: "Token Controller is live", body: "Token Controller counts every token your AI agents use and matches it to your providers' bills.\n\nSolo is free forever for one person.", status: "draft", recipients: 0, delivered: 0, created_at: "2026-10-11T21:00:00Z", send_started_at: null, sent_at: null }],
    },
  };
}

type Member = (handler: (ctx: { db: Db; me: Person; url: URL; params: Record<string, string>; request: Request }) => Response | Promise<Response>) => HttpResponseResolver;
type Fail = (status: number, code: string, message: string) => Response;

const DIAGNOSTICS = ["page", "app_version", "browser", "screen", "errors", "request_ids"];
const MAX_SCREENSHOT = 3 * 1024 * 1024;

function toReport(r: ReportRecord): Report {
  return { id: r.id, kind: r.kind, text: r.text, status: r.status, answer: r.answer ?? null, fixed_in: r.fixed_in ?? null, has_screenshot: r.has_screenshot, diagnostics: r.diagnostics, created_at: r.created_at, updated_at: r.updated_at };
}

function toSummary(o: OtherCustomer): CustomerSummary {
  return { id: o.id, name: o.name, plan: o.plan ?? null, members: o.members, open_support: o.open_support, open_feedback: o.open_feedback, created_at: o.created_at };
}

function toNote(n: CustomerRelations["notes"][number]): CustomerNote {
  return { id: n.id, author_email: n.author_email, body: n.body, created_at: n.created_at };
}

function toConversation(c: ConversationRecord): Conversation {
  return { id: c.id, channel: c.channel, contact_email: c.contact_email, contact_name: c.contact_name ?? null, subject: c.subject, kind: c.kind, state: c.state, messages: c.thread.length, created_at: c.created_at, updated_at: c.updated_at };
}

export function customerRelationsHandlers({ API, member, fail, now }: { API: string; member: Member; fail: Fail; now: () => string }) {
  const isOperator = (db: Db, me: Person) => db.cr.operators.includes(me.email);
  const notFound = () => fail(404, "not_found", "There is nothing here.");

  /** Operators only: everyone else gets the 404 the service answers. */
  const operator = (handler: Parameters<Member>[0]): HttpResponseResolver => member((ctx) => (isOperator(ctx.db, ctx.me) ? handler(ctx) : notFound()));

  /** Operators, or the controllers where there are none. */
  const reviewer = (handler: Parameters<Member>[0]): HttpResponseResolver =>
    member((ctx) => (isOperator(ctx.db, ctx.me) || (!ctx.db.cr.operators.length && ctx.me.role === "controller") ? handler(ctx) : notFound()));

  const received = (db: Db, r: ReportRecord): ReceivedReport => {
    const p = db.people.find((x) => x.id === r.person_id);
    return { report: toReport(r), reporter: { person_id: r.person_id, email: p?.email ?? "", name: p?.name ?? "" }, organization: { id: db.organization.id, name: db.organization.name } };
  };

  const kestrel = (db: Db): CustomerSummary => ({
    id: db.organization.id,
    name: db.organization.name,
    plan: db.billing.enabled ? db.billing.plan : null,
    members: db.people.filter((p) => p.status !== "removed").length,
    open_support: db.cr.support.filter((c) => c.customer_id === db.organization.id && c.state !== "closed").length,
    open_feedback: db.cr.reports.filter((r) => r.status === "received").length,
    created_at: db.organization.created_at,
  });

  return [
    http.post(`${API}/__mock/operator`, member(({ db, me }) => {
      if (!isOperator(db, me)) db.cr.operators.push(me.email);
      return new HttpResponse(null, { status: 204 });
    })),

    // Every member
    http.get(`${API}/feedback`, member(({ db, me }) => HttpResponse.json(db.cr.reports.filter((r) => r.person_id === me.id).map(toReport).reverse()))),
    http.post(`${API}/feedback`, member(async ({ db, me, request }) => {
      const body = (await request.json()) as NewReport;
      if (!["bug", "idea", "question"].includes(body.kind)) return fail(400, "bad_request", "A report is a bug, an idea or a question.");
      const text = body.text.trim();
      if (!text) return fail(400, "bad_request", "Write what happened or what you would like.");
      if (text.length > 5000) return fail(400, "bad_request", "The text is longer than 5000 characters.");
      const extra = Object.keys(body.diagnostics ?? {}).filter((k) => !DIAGNOSTICS.includes(k));
      if (extra.length) return fail(400, "bad_request", `A report carries no ${extra.join(", ")}.`);
      if (body.screenshot && (!["image/png", "image/jpeg"].includes(body.screenshot.type) || (body.screenshot.data.length * 3) / 4 > MAX_SCREENSHOT)) {
        return fail(400, "bad_request", "The screenshot is a PNG or JPEG image in base64, at most 3 MB.");
      }
      const d = body.diagnostics ?? {};
      const record: ReportRecord = {
        id: `fb000000-0000-4000-8000-${String(Date.now()).padStart(12, "0").slice(-12)}`,
        person_id: me.id,
        kind: body.kind,
        text,
        status: "received",
        answer: null,
        fixed_in: null,
        has_screenshot: Boolean(body.screenshot),
        screenshot: body.screenshot ?? null,
        diagnostics: { ...noDiagnostics, ...d, page: (d.page ?? "").split(/[?#]/)[0] ?? "", errors: (d.errors ?? []).slice(-10), request_ids: (d.request_ids ?? []).slice(-10) },
        created_at: now(),
        updated_at: now(),
      };
      db.cr.reports.push(record);
      return HttpResponse.json(toReport(record), { status: 201 });
    })),
    http.get(`${API}/feedback/:id/screenshot`, member(({ db, me, params }) => {
      const r = db.cr.reports.find((x) => x.id === params.id);
      const allowed = r && (r.person_id === me.id || isOperator(db, me) || (!db.cr.operators.length && me.role === "controller"));
      if (!r?.screenshot || !allowed) return notFound();
      const bytes = Uint8Array.from(atob(r.screenshot.data), (c) => c.charCodeAt(0));
      return new HttpResponse(bytes, { headers: { "content-type": r.screenshot.type } });
    })),
    http.get(`${API}/support`, member(({ me }) => HttpResponse.json({ answer_to: me.email }))),
    http.post(`${API}/support/messages`, member(async ({ db, me, request }) => {
      const { message } = (await request.json()) as { message: string };
      if (!message.trim()) return fail(400, "bad_request", "Write a message.");
      const id = `5c000000-0000-4000-8000-${String(Date.now()).slice(-12)}`;
      db.cr.support.push({
        id,
        channel: "app",
        contact_email: me.email,
        contact_name: me.name || null,
        subject: message.trim().split("\n")[0]?.slice(0, 80) ?? "Message",
        kind: "question",
        state: "new",
        messages: 1,
        created_at: now(),
        updated_at: now(),
        customer_id: db.organization.id,
        escalation_reason: null,
        suggested_answer: null,
        thread: [{ id: `${id}-1`, author: "customer", body: message.trim(), links: [], created_at: now(), sent_at: null }],
      });
      return new HttpResponse(null, { status: 202 });
    })),

    // The admin area
    http.get(`${API}/admin`, operator(({ db }) =>
      HttpResponse.json({
        support_escalated: db.cr.support.filter((c) => c.state === "escalated").length,
        feedback_received: db.cr.reports.filter((r) => r.status === "received").length,
        assistant_missed_7_days: db.cr.questions.filter((q) => !q.answered).length,
        newsletter_subscribers: db.cr.newsletter.confirmed,
      } satisfies AdminOverview),
    )),
    http.get(`${API}/admin/support`, operator(({ db, url }) => {
      const state = url.searchParams.get("state");
      const list = db.cr.support.filter((c) => (state ? c.state === state : c.state !== "closed")).sort((a, b) => Number(b.state === "escalated") - Number(a.state === "escalated") || b.updated_at.localeCompare(a.updated_at));
      return HttpResponse.json(list.map(toConversation));
    })),
    http.get(`${API}/admin/support/:id`, operator(({ db, params }) => {
      const c = db.cr.support.find((x) => x.id === params.id);
      if (!c) return notFound();
      const customer = c.customer_id === db.organization.id ? { id: db.organization.id, name: db.organization.name } : db.cr.others.find((o) => o.id === c.customer_id);
      return HttpResponse.json({
        conversation: toConversation(c),
        customer: customer ? { id: customer.id, name: customer.name } : null,
        escalation_reason: c.escalation_reason,
        suggested_answer: c.suggested_answer,
        messages: c.thread,
      } satisfies ConversationView);
    })),
    http.post(`${API}/admin/support/:id/replies`, operator(async ({ db, params, request }) => {
      const c = db.cr.support.find((x) => x.id === params.id);
      if (!c) return notFound();
      const { text } = (await request.json()) as { text: string };
      if (!text.trim()) return fail(400, "bad_request", "Write the reply.");
      const message: SupportMessage = { id: `${c.id}-${String(c.thread.length + 1)}`, author: "operator", body: text.trim(), links: [], created_at: now(), sent_at: null };
      c.thread.push(message);
      Object.assign(c, { state: "answered", suggested_answer: null, updated_at: now() });
      return HttpResponse.json(message, { status: 201 });
    })),
    http.post(`${API}/admin/support/:id/close`, operator(({ db, params }) => {
      const c = db.cr.support.find((x) => x.id === params.id);
      if (!c) return notFound();
      Object.assign(c, { state: "closed", updated_at: now() });
      return HttpResponse.json(toConversation(c));
    })),
    http.get(`${API}/admin/feedback`, reviewer(({ db, url }) => {
      const status = url.searchParams.get("status") as FeedbackStatus | null;
      return HttpResponse.json(db.cr.reports.filter((r) => !status || r.status === status).map((r) => received(db, r)).reverse());
    })),
    http.get(`${API}/admin/feedback/:id`, reviewer(({ db, params }) => {
      const r = db.cr.reports.find((x) => x.id === params.id);
      return r ? HttpResponse.json(received(db, r)) : notFound();
    })),
    http.patch(`${API}/admin/feedback/:id`, reviewer(async ({ db, params, request }) => {
      const r = db.cr.reports.find((x) => x.id === params.id);
      if (!r) return notFound();
      const change = (await request.json()) as ReportChange;
      if (change.fixed_in && !/^\d{1,5}\.\d{1,5}\.\d{1,5}$/.test(change.fixed_in)) return fail(400, "bad_request", "A version is three numbers, such as 1.0.1.");
      if (change.answer !== undefined) Object.assign(r, { answer: change.answer, status: change.answer ? "answered" : r.status });
      if (change.fixed_in) Object.assign(r, { fixed_in: change.fixed_in, status: "fixed" });
      if (change.status) r.status = change.status;
      r.updated_at = now();
      return HttpResponse.json(received(db, r));
    })),
    http.get(`${API}/admin/customers`, operator(({ db }) => HttpResponse.json([kestrel(db), ...db.cr.others.map(toSummary)]))),
    http.get(`${API}/admin/customers/:id`, operator(({ db, params }) => {
      const id = params.id ?? "";
      const notes = db.cr.notes.filter((n) => n.customer_id === id).map(toNote);
      const support = db.cr.support.filter((c) => c.customer_id === id).map(toConversation);
      if (id === db.organization.id) {
        return HttpResponse.json({
          customer: kestrel(db),
          slug: db.organization.slug,
          reporting_currency: db.organization.reporting_currency,
          subscription: db.billing.enabled && db.billing.plan === "team" ? { status: db.billing.status, interval: db.billing.interval, current_period_end: db.billing.current_period_end, cancel_at_period_end: db.billing.cancel_at_period_end } : null,
          members: db.people.map((p) => ({ id: p.id, name: p.name, email: p.email, role: p.role, invited_at: p.invited_at, joined_at: p.joined_at ?? null, removed_at: p.status === "removed" ? "2026-09-30T17:00:00Z" : null, newsletter: p.email === "p.shah@kestrel.example" ? "confirmed" : null })),
          support,
          feedback: db.cr.reports.map((r) => received(db, r)),
          notes,
        } satisfies CustomerRecord);
      }
      const other = db.cr.others.find((o) => o.id === id);
      if (!other) return notFound();
      return HttpResponse.json({ customer: toSummary(other), slug: other.slug, reporting_currency: other.reporting_currency, subscription: null, members: [], support, feedback: [], notes } satisfies CustomerRecord);
    })),
    http.post(`${API}/admin/customers/:id/notes`, operator(async ({ db, me, params, request }) => {
      const { text } = (await request.json()) as { text: string };
      if (!text.trim()) return fail(400, "bad_request", "Write the note.");
      const note = { id: `a0000000-0000-4000-8000-${String(Date.now()).slice(-12)}`, customer_id: params.id ?? "", author_email: me.email, body: text.trim(), created_at: now() };
      db.cr.notes.push(note);
      return HttpResponse.json(toNote(note), { status: 201 });
    })),
    http.get(`${API}/admin/assistant/questions`, operator(({ db, url }) => {
      const answered = url.searchParams.get("answered");
      return HttpResponse.json(db.cr.questions.filter((q) => answered === null || String(q.answered) === answered));
    })),
    http.get(`${API}/admin/newsletter`, operator(({ db }) => HttpResponse.json(db.cr.newsletter))),
    http.post(`${API}/admin/newsletter/issues`, operator(async ({ db, request }) => {
      const { subject, body } = (await request.json()) as { subject: string; body: string };
      const issue: Issue = { id: `15500000-0000-4000-8000-${String(Date.now()).slice(-12)}`, subject, body, status: "draft", recipients: 0, delivered: 0, created_at: now(), send_started_at: null, sent_at: null };
      db.cr.newsletter.issues.unshift(issue);
      return HttpResponse.json(issue, { status: 201 });
    })),
    http.patch(`${API}/admin/newsletter/issues/:id`, operator(async ({ db, params, request }) => {
      const issue = db.cr.newsletter.issues.find((i) => i.id === params.id);
      if (!issue) return notFound();
      if (issue.status !== "draft") return fail(409, "conflict", "Only a draft can change.");
      const change = (await request.json()) as { subject?: string | null; body?: string | null };
      if (change.subject) issue.subject = change.subject;
      if (change.body) issue.body = change.body;
      return HttpResponse.json(issue);
    })),
    http.post(`${API}/admin/newsletter/issues/:id/send`, operator(({ db, params }) => {
      const issue = db.cr.newsletter.issues.find((i) => i.id === params.id);
      if (!issue) return notFound();
      if (issue.status !== "draft" || !db.cr.newsletter.on) return fail(409, "conflict", "Only a draft can be sent, and only while the newsletter is on.");
      Object.assign(issue, { status: "sent", recipients: db.cr.newsletter.confirmed, delivered: db.cr.newsletter.confirmed, send_started_at: now(), sent_at: now() });
      return HttpResponse.json(issue);
    })),
  ];
}
