import { call } from "./client";

// The Enterprise plan's settings API. Only the Enterprise image serves it, so
// it is not in the service's OpenAPI document: the open image answers 404 to
// every path here. Every call needs a signed-in controller, and all but the
// license answer 403 `enterprise_plan_needed` once a license has ended
// (docs: Self-hosting > OIDC and SCIM; Enterprise plan instances).

/** Where the license stands. `null` without a key. */
export type Standing = "active" | "renewal_due" | "grace" | "ended";

export interface License {
  /** `enterprise` while the features run, otherwise `open`. */
  edition: string;
  standing: Standing | null;
  organization: string | null;
  /** Dates, `YYYY-MM-DD`. */
  support_ends: string | null;
  /** The last day the features run. */
  features_end: string | null;
  /** For controllers, near or after the end of the license. */
  notice: string | null;
  /** Why a key that is set was refused. */
  problem: string | null;
}

/** Sign-in with the organization's identity provider (OIDC). */
export interface Oidc {
  configured: boolean;
  label: string | null;
  issuer_url: string | null;
  client_id: string | null;
  /** What to register at the identity provider as the redirect URI. */
  redirect_url: string;
}

export interface OidcInput {
  label?: string;
  issuer_url: string;
  client_id: string;
  /** Left out, the secret already saved stays. */
  client_secret?: string;
}

export interface ScimToken {
  id: string;
  created_at: string;
  last_used_at: string | null;
}

export interface ScimTokens {
  scim_url: string;
  tokens: ScimToken[];
}

/** A new SCIM token. The token itself is shown this once. */
export interface CreatedScimToken {
  id: string;
  token: string;
  scim_url: string;
}

export interface AuditEntry {
  id: number;
  recorded_at: string;
  /** The member who made the change; null for the service or the identity provider. */
  actor_id: string | null;
  /** A dotted name, such as `member.removed`. */
  action: string;
  target_id: string | null;
}

export type CloudProvider = "bedrock" | "vertex" | "foundry";

export interface ConnectorRun {
  provider: CloudProvider;
  started_at: string;
  period_start: string;
  /** The last day fetched: the month's last, or yesterday for this month. */
  period_end: string;
  lines: number;
  error: string | null;
}

export interface Connectors {
  connectors: CloudProvider[];
  runs: ConnectorRun[];
}

export const enterprise = {
  license: () => call<License>("GET", "/enterprise/license"),
  oidc: () => call<Oidc>("GET", "/enterprise/oidc"),
  setOidc: (body: OidcInput) => call<Oidc>("PUT", "/enterprise/oidc", { body }),
  removeOidc: () => call<undefined>("DELETE", "/enterprise/oidc"),
  scimTokens: () => call<ScimTokens>("GET", "/enterprise/scim/tokens"),
  createScimToken: () => call<CreatedScimToken>("POST", "/enterprise/scim/tokens"),
  revokeScimToken: (id: string) => call<undefined>("DELETE", `/enterprise/scim/tokens/${encodeURIComponent(id)}`),
  auditLog: (before?: number) => call<{ entries: AuditEntry[] }>("GET", "/enterprise/audit-log", { query: { before, limit: 100 } }),
  connectors: () => call<Connectors>("GET", "/enterprise/connectors"),
};
