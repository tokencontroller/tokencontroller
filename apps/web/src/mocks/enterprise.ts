// Mock answers for the Enterprise plan's settings, which only the Enterprise
// image serves. The mock starts as the open edition, as most instances are;
// `POST /__mock/edition {"edition": "enterprise"}` turns it into an
// Enterprise plan instance with a license that is due for renewal.

import { http, HttpResponse, type HttpResponseResolver } from "msw";
import type { Person } from "@/api/contract";
import type { AuditEntry, ConnectorRun, Oidc, OidcInput, ScimToken } from "@/api/enterprise";
import type { Db } from "./seed";

type Member = (handler: (ctx: { db: Db; me: Person; url: URL; params: Record<string, string>; request: Request }) => Response | Promise<Response>) => HttpResponseResolver;
type Fail = (status: number, code: string, message: string) => Response;

interface EnterpriseState {
  edition: "open" | "enterprise";
  oidc: (OidcInput & { label: string; client_secret: string }) | null;
  scim: ScimToken[];
  audit: AuditEntry[];
  nextId: number;
}

const KEY = "tc-mock-enterprise";
/** The member limit of the example license. */
export const LICENSE_MEMBERS = 250;
const ORIGIN = "https://tokencontroller.kestrel.example";

function fresh(): EnterpriseState {
  return { edition: "open", oidc: null, scim: [], audit: [], nextId: 1 };
}

function load(): EnterpriseState {
  try {
    const saved = sessionStorage.getItem(KEY);
    if (saved) return JSON.parse(saved) as EnterpriseState;
  } catch {
    // No storage here; start fresh.
  }
  return fresh();
}

function save(state: EnterpriseState): void {
  try {
    sessionStorage.setItem(KEY, JSON.stringify(state));
  } catch {
    // Memory only.
  }
}

/** `open` or `enterprise`, as the health endpoint reports it. */
export function mockEdition(): EnterpriseState["edition"] {
  return load().edition;
}

/** A few weeks of the audit log of Kestrel Digital, newest first. */
function exampleAudit(db: Db): AuditEntry[] {
  const id = (email: string) => db.people.find((p) => p.email === email)?.id ?? null;
  const nadia = id("n.berger@kestrel.example");
  const priya = id("p.shah@kestrel.example");
  const entries: Omit<AuditEntry, "id">[] = [
    { recorded_at: "2026-10-12T08:14:00Z", actor_id: id("m.ortner@kestrel.example"), action: "member.signed_in_with_identity_provider", target_id: null },
    { recorded_at: "2026-10-09T15:40:00Z", actor_id: nadia, action: "organization.rules_changed", target_id: null },
    { recorded_at: "2026-10-08T10:02:00Z", actor_id: null, action: "member.added", target_id: id("o.nowak@kestrel.example") },
    { recorded_at: "2026-10-07T09:31:00Z", actor_id: priya, action: "member.filters_changed", target_id: id("j.weber@kestrel.example") },
    { recorded_at: "2026-10-02T11:20:00Z", actor_id: nadia, action: "tag_map.changed", target_id: null },
    { recorded_at: "2026-10-01T08:00:00Z", actor_id: nadia, action: "api_token.created", target_id: null },
  ];
  return entries.map((e, i) => ({ ...e, id: entries.length - i }));
}

const RUNS: ConnectorRun[] = [
  { provider: "bedrock", started_at: "2026-10-12T04:10:00Z", period_start: "2026-10-01", period_end: "2026-10-11", lines: 22, error: null },
  { provider: "bedrock", started_at: "2026-10-12T04:10:00Z", period_start: "2026-09-01", period_end: "2026-09-30", lines: 64, error: null },
];

export function enterpriseHandlers({ API, member, fail, now }: { API: string; member: Member; fail: Fail; now: () => string }) {
  /** Every enterprise call: 404 on the open edition, controllers only. */
  const controller = (handler: (state: EnterpriseState, ctx: { db: Db; me: Person; request: Request; params: Record<string, string>; url: URL }) => Response | Promise<Response>) =>
    member(async (ctx) => {
      const state = load();
      if (state.edition !== "enterprise") return fail(404, "not_found", "There is nothing here.");
      if (ctx.me.role !== "controller") return fail(403, "forbidden", "only controllers can do this");
      if (!state.audit.length) state.audit = exampleAudit(ctx.db);
      const answer = await handler(state, ctx);
      save(state);
      return answer;
    });
  const record = (state: EnterpriseState, me: Person, action: string, target: string | null = null) => {
    const id = Math.max(0, ...state.audit.map((e) => e.id)) + 1;
    state.audit.unshift({ id, recorded_at: now(), actor_id: me.id, action, target_id: target });
  };
  const oidcView = (state: EnterpriseState): Oidc => ({
    configured: Boolean(state.oidc),
    label: state.oidc?.label ?? null,
    issuer_url: state.oidc?.issuer_url ?? null,
    client_id: state.oidc?.client_id ?? null,
    redirect_url: `${ORIGIN}/api/v1/sign-in/providers/oidc/callback`,
  });

  return [
    http.post(`${API}/__mock/edition`, async ({ request }) => {
      const { edition } = (await request.json()) as { edition: EnterpriseState["edition"] };
      save({ ...load(), edition });
      return new HttpResponse(null, { status: 204 });
    }),

    http.get(`${API}/sign-in/providers`, () => {
      const state = load();
      return HttpResponse.json(state.edition === "enterprise" && state.oidc ? [{ id: "oidc", label: state.oidc.label, url: `${API}/sign-in/providers/oidc` }] : []);
    }),

    http.get(`${API}/enterprise/license`, controller(() =>
      HttpResponse.json({
        edition: "enterprise",
        standing: "renewal_due",
        organization: "Kestrel Digital",
        member_limit: LICENSE_MEMBERS,
        support_ends: "2026-11-30",
        features_end: "2026-12-30",
        notice: "Your Enterprise plan license ends on 30 November 2026. Renew it to keep sign-in with your identity provider, the audit log, telemetry passthrough, cloud bill connectors and the member limit of 250.",
        problem: null,
      }),
    )),

    http.get(`${API}/enterprise/oidc`, controller((state) => HttpResponse.json(oidcView(state)))),
    http.put(`${API}/enterprise/oidc`, controller(async (state, { me, request }) => {
      const input = (await request.json()) as OidcInput;
      const issuer_url = input.issuer_url.trim().replace(/\/+$/, "");
      if (!issuer_url.startsWith("https://")) return fail(400, "bad_request", "the issuer URL must start with https://");
      const client_id = input.client_id.trim();
      if (!client_id) return fail(400, "bad_request", "the client ID is required");
      const client_secret = input.client_secret?.trim() || state.oidc?.client_secret;
      if (!client_secret) return fail(400, "bad_request", "the client secret is required");
      state.oidc = { label: input.label?.trim() || "Sign in with your company account", issuer_url, client_id, client_secret };
      record(state, me, "sign_in.provider_changed");
      return HttpResponse.json(oidcView(state));
    })),
    http.delete(`${API}/enterprise/oidc`, controller((state, { me }) => {
      state.oidc = null;
      record(state, me, "sign_in.provider_removed");
      return new HttpResponse(null, { status: 204 });
    })),

    http.get(`${API}/enterprise/scim/tokens`, controller((state) => HttpResponse.json({ scim_url: `${ORIGIN}/scim/v2`, tokens: state.scim }))),
    http.post(`${API}/enterprise/scim/tokens`, controller((state, { me }) => {
      state.nextId += 1;
      const id = `5c1a0000-0000-4000-8000-${String(state.nextId).padStart(12, "0")}`;
      state.scim.push({ id, created_at: now(), last_used_at: null });
      record(state, me, "scim.token_created", id);
      return HttpResponse.json({ id, token: `tc_scim_${Math.random().toString(36).slice(2)}${Math.random().toString(36).slice(2)}`, scim_url: `${ORIGIN}/scim/v2` }, { status: 201 });
    })),
    http.delete(`${API}/enterprise/scim/tokens/:id`, controller((state, { me, params }) => {
      const id = params.id ?? "";
      if (!state.scim.some((t) => t.id === id)) return fail(404, "not_found", "not found");
      state.scim = state.scim.filter((t) => t.id !== id);
      record(state, me, "scim.token_revoked", id);
      return new HttpResponse(null, { status: 204 });
    })),

    http.get(`${API}/enterprise/audit-log`, controller((state, { url }) => {
      const before = Number(url.searchParams.get("before") ?? Infinity);
      const limit = Math.min(500, Math.max(1, Number(url.searchParams.get("limit") ?? 100)));
      return HttpResponse.json({ entries: state.audit.filter((e) => e.id < before).slice(0, limit) });
    })),

    http.get(`${API}/enterprise/connectors`, controller(() => HttpResponse.json({ connectors: ["bedrock"], runs: RUNS }))),
  ];
}
