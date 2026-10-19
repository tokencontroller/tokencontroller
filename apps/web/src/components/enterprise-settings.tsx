import { useInfiniteQuery, useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import { enterprise, type AuditEntry, type CloudProvider, type ConnectorRun, type License, type ScimToken, type Standing } from "@/api/enterprise";
import { api } from "@/api/endpoints";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Loading, Note, Section } from "@/components/ui/display";
import { Field, FormError, Input } from "@/components/ui/form";
import { DataTable, type Column } from "@/components/ui/table";
import { notify } from "@/components/ui/toast";
import { displayName, formatDate, formatDateTime, formatDay } from "@/lib/format";
import { refreshAll, useMe, usePeople, usePersonName } from "@/lib/queries";
import { hasRole } from "@/lib/roles";

// The Enterprise plan's settings: the license, sign-in with the identity
// provider, SCIM, the audit log and cloud bill connectors. Only an Enterprise
// plan instance has them (docs: Self-hosting > OIDC and SCIM).

/** Whether this instance runs the Enterprise plan's features. */
export function useEnterprise(): boolean {
  const health = useQuery({ queryKey: ["health"], queryFn: api.health, staleTime: Infinity });
  return health.data?.edition === "enterprise";
}

/** The license, for org managers on an Enterprise plan instance. */
function useLicense() {
  const me = useMe();
  const on = useEnterprise() && hasRole(me.person, "org_manager");
  return useQuery({ queryKey: ["enterprise", "license"], queryFn: enterprise.license, enabled: on, staleTime: 5 * 60_000 });
}

/** Near or after the end of the license, a line for org managers on every page. */
export function LicenseNotice(): ReactNode {
  const license = useLicense();
  const notice = license.data?.notice;
  if (!notice) return null;
  return <Note tone={license.data?.standing === "grace" ? "exception" : "accent"}>{notice}</Note>;
}

const standingText: Record<Standing, { label: string; tone: "posted" | "accent" | "exception" }> = {
  active: { label: "Active", tone: "posted" },
  renewal_due: { label: "Renewal due", tone: "accent" },
  grace: { label: "Ended, features still running", tone: "exception" },
  ended: { label: "Ended", tone: "exception" },
};

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1 border-b border-line py-3 last:border-0 sm:flex-row sm:items-center sm:justify-between sm:gap-6">
      <div className="text-sm font-medium">{label}</div>
      <div className="text-sm sm:text-right">{children}</div>
    </div>
  );
}

function LicenseFacts({ license }: { license: License }) {
  const standing = license.standing ? standingText[license.standing] : null;
  return (
    <>
      {license.problem ? <Note tone="exception">The license key was refused: {license.problem}</Note> : null}
      <div className="rounded-lg border border-line px-4">
        <Row label="License">{standing ? <Badge tone={standing.tone}>{standing.label}</Badge> : <Badge tone="exception">No license key</Badge>}</Row>
        {license.organization ? <Row label="Licensed to">{license.organization}</Row> : null}
        {license.support_ends ? <Row label="Support until">{formatDate(license.support_ends)}</Row> : null}
        {license.features_end ? <Row label="Features until">{formatDate(license.features_end)}</Row> : null}
      </div>
    </>
  );
}

function CopyField({ label, value, hint }: { label: string; value: string; hint: string }) {
  return (
    <div>
      <div className="flex items-end gap-2">
        <Field label={label} className="min-w-0 flex-1">
          <Input readOnly value={value} className="font-mono text-[13px]" onFocus={(e) => e.target.select()} />
        </Field>
        <Button onClick={() => void navigator.clipboard.writeText(value).then(() => notify("Copied"))}>Copy</Button>
      </div>
      <p className="mt-1 text-xs text-muted">{hint}</p>
    </div>
  );
}

function SignInProvider() {
  const oidc = useQuery({ queryKey: ["enterprise", "oidc"], queryFn: enterprise.oidc, retry: false });
  const [form, setForm] = useState<{ label: string; issuer_url: string; client_id: string; client_secret: string } | null>(null);
  const [removing, setRemoving] = useState(false);
  const shown = form ?? { label: oidc.data?.label ?? "", issuer_url: oidc.data?.issuer_url ?? "", client_id: oidc.data?.client_id ?? "", client_secret: "" };
  const set = (key: keyof typeof shown, value: string) => setForm({ ...shown, [key]: value });
  const save = useMutation({
    mutationFn: () =>
      enterprise.setOidc({
        issuer_url: shown.issuer_url,
        client_id: shown.client_id,
        ...(shown.label.trim() ? { label: shown.label.trim() } : {}),
        ...(shown.client_secret ? { client_secret: shown.client_secret } : {}),
      }),
    onSuccess: async () => {
      await refreshAll();
      setForm(null);
      notify("Sign-in saved", "Members now see the button on the sign-in page.");
    },
  });
  const remove = useMutation({
    mutationFn: enterprise.removeOidc,
    onSuccess: async () => {
      await refreshAll();
      setForm(null);
      setRemoving(false);
      notify("Sign-in removed", "Members sign in by magic link.");
    },
  });
  if (oidc.isPending) return <Loading />;
  if (oidc.isError) return <ErrorBox error={oidc.error} />;
  const configured = oidc.data.configured;
  return (
    <div className="space-y-4 rounded-lg border border-line p-4">
      <CopyField label="Redirect URI" value={oidc.data.redirect_url} hint="Register Token Controller with your provider as a web app with this redirect URI." />
      <form className="space-y-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
        <div className="grid gap-3 sm:grid-cols-2">
          <Field label="Issuer URL">
            <Input required type="url" value={shown.issuer_url} onChange={(e) => set("issuer_url", e.target.value)} placeholder="https://login.microsoftonline.com/…/v2.0" />
          </Field>
          <Field label="Client ID">
            <Input required value={shown.client_id} onChange={(e) => set("client_id", e.target.value)} autoComplete="off" />
          </Field>
          <Field label="Client secret" {...(configured ? { hint: "Leave it empty to keep the saved secret." } : {})}>
            <Input type="password" required={!configured} value={shown.client_secret} onChange={(e) => set("client_secret", e.target.value)} autoComplete="off" />
          </Field>
          <Field label="Button label">
            <Input value={shown.label} onChange={(e) => set("label", e.target.value)} placeholder="Sign in with your company account" />
          </Field>
        </div>
        <p className="text-[13px] text-muted">The provider must send the member's email address. Only members can sign in this way: invite people, or let SCIM add them.</p>
        <FormError error={save.error} />
        <div className="flex flex-wrap justify-end gap-2">
          {configured ? <Button onClick={() => setRemoving(true)}>Remove</Button> : null}
          <Button type="submit" variant="primary" disabled={save.isPending}>
            {configured ? "Save sign-in" : "Add sign-in"}
          </Button>
        </div>
      </form>
      <Dialog open={removing} onOpenChange={setRemoving} title="Remove sign-in with your provider?" description="The button leaves the sign-in page. Members sign in by magic link.">
        <FormError error={remove.error} />
        <div className="mt-2 flex justify-end gap-2">
          <Button onClick={() => setRemoving(false)}>Cancel</Button>
          <Button variant="primary" onClick={() => remove.mutate()} disabled={remove.isPending}>
            Remove
          </Button>
        </div>
      </Dialog>
    </div>
  );
}

function Scim() {
  const tokens = useQuery({ queryKey: ["enterprise", "scim"], queryFn: enterprise.scimTokens, retry: false });
  const [secret, setSecret] = useState<string | null>(null);
  const create = useMutation({
    mutationFn: enterprise.createScimToken,
    onSuccess: async (created) => {
      await refreshAll();
      setSecret(created.token);
    },
  });
  const revoke = useMutation({ mutationFn: enterprise.revokeScimToken, onSuccess: async () => { await refreshAll(); notify("SCIM token revoked"); } });
  if (tokens.isPending) return <Loading />;
  if (tokens.isError) return <ErrorBox error={tokens.error} />;
  const columns: Column<ScimToken>[] = [
    { key: "created", header: "Created", cell: (t) => formatDate(t.created_at), csv: (t) => t.created_at },
    { key: "used", header: "Last used", cell: (t) => (t.last_used_at ? formatDateTime(t.last_used_at) : "never"), csv: (t) => t.last_used_at ?? "" },
    { key: "actions", header: "", cell: (t) => <span className="flex justify-end"><Button size="sm" variant="ghost" onClick={() => revoke.mutate(t.id)}>Revoke</Button></span> },
  ];
  return (
    <>
      <div className="mb-4 rounded-lg border border-line p-4">
        <CopyField label="SCIM URL" value={tokens.data.scim_url} hint="Give it with a SCIM token to your identity provider. Their role and tags are set here." />
      </div>
      <DataTable stack columns={columns} rows={tokens.data.tokens} rowKey={(t) => t.id} caption="SCIM tokens" empty="No SCIM token yet." />
      <FormError error={create.error ?? revoke.error} />
      <div className="mt-3">
        <Button onClick={() => create.mutate()} disabled={create.isPending}>
          New SCIM token
        </Button>
      </div>
      <Dialog open={Boolean(secret)} onOpenChange={() => setSecret(null)} title="Copy your SCIM token now" description="It is shown once. Paste it into your identity provider with the SCIM URL.">
        <code className="block break-all rounded-md bg-sand p-3 font-mono text-[13px]" data-testid="scim-token">
          {secret}
        </code>
        <div className="mt-4 flex justify-end gap-2">
          <Button onClick={() => void navigator.clipboard.writeText(secret ?? "").then(() => notify("Copied"))}>Copy</Button>
          <Button variant="primary" onClick={() => setSecret(null)}>
            Done
          </Button>
        </div>
      </Dialog>
    </>
  );
}

/** Each change the audit log records, in words. Unknown ones show their name. */
const actionText: Record<string, string> = {
  "organization.created": "Created the organization",
  "organization.settings_changed": "Changed the organization's settings",
  "organization.rules_changed": "Changed the organization's rules",
  "member.invited": "Invited a member",
  "member.added": "Added a member",
  "member.changed": "Changed a member",
  "member.filters_changed": "Changed a member's filters",
  "member.removed": "Removed a member",
  "member.signed_in": "Signed in",
  "member.signed_in_with_identity_provider": "Signed in with the identity provider",
  "api_token.created": "Created an API token",
  "api_token.revoked": "Revoked an API token",
  "api_token.handed_over": "Moved a service token",
  "tag_name.added": "Added a tag name",
  "tag_name.removed": "Removed a tag name",
  "tag_values.merged": "Merged tag values",
  "tag_value.added": "Added a tag value",
  "grant.added": "Handed on a grant",
  "sign_in.provider_changed": "Changed sign-in with the identity provider",
  "sign_in.provider_removed": "Removed sign-in with the identity provider",
  "scim.token_created": "Created a SCIM token",
  "scim.token_revoked": "Revoked a SCIM token",
  "settings.changed": "Changed the Enterprise plan settings",
};

function AuditLog() {
  const name = usePersonName();
  const people = usePeople();
  const log = useInfiniteQuery({
    queryKey: ["enterprise", "audit-log"],
    queryFn: ({ pageParam }) => enterprise.auditLog(pageParam),
    initialPageParam: undefined as number | undefined,
    // Newest first, 100 at a time: older ones come before the last id shown.
    getNextPageParam: (last) => (last.entries.length === 100 ? last.entries.at(-1)?.id : undefined),
    retry: false,
  });
  if (log.isPending) return <Loading />;
  if (log.isError) return <ErrorBox error={log.error} />;
  const rows = log.data.pages.flatMap((p) => p.entries);
  const who = (id: string | null) => (id ? name(id) : "Token Controller");
  const member = (id: string | null) => {
    const found = id ? people.data?.find((p) => p.id === id) : undefined;
    return found ? displayName(found) : "";
  };
  const columns: Column<AuditEntry>[] = [
    { key: "when", header: "When", className: "whitespace-nowrap", cell: (e) => formatDateTime(e.recorded_at), csv: (e) => e.recorded_at },
    { key: "who", header: "Who", cell: (e) => who(e.actor_id), csv: (e) => e.actor_id ?? "" },
    { key: "what", header: "What", cell: (e) => actionText[e.action] ?? e.action, csv: (e) => e.action },
    // The member a change was about; other records (tokens, settings) need no name here.
    { key: "member", header: "Member", wide: true, cell: (e) => member(e.target_id), csv: (e) => e.target_id ?? "" },
  ];
  return (
    <>
      <DataTable stack columns={columns} rows={rows} rowKey={(e) => String(e.id)} csvName="audit-log.csv" caption="Audit log" empty="Nothing recorded yet." />
      {log.hasNextPage ? (
        <div className="mt-3">
          <Button onClick={() => void log.fetchNextPage()} disabled={log.isFetchingNextPage}>
            Show older
          </Button>
        </div>
      ) : null}
    </>
  );
}

const cloudName: Record<CloudProvider, string> = { bedrock: "Bedrock", vertex: "Vertex", foundry: "Foundry" };

function CloudConnectors() {
  const connectors = useQuery({ queryKey: ["enterprise", "connectors"], queryFn: enterprise.connectors, retry: false });
  if (connectors.isPending) return <Loading />;
  if (connectors.isError) return <ErrorBox error={connectors.error} />;
  const { connectors: set, runs } = connectors.data;
  const columns: Column<ConnectorRun>[] = [
    { key: "provider", header: "Cloud", cell: (r) => cloudName[r.provider], csv: (r) => r.provider },
    { key: "period", header: "Days", cell: (r) => `${formatDay(r.period_start)} to ${formatDay(r.period_end)}`, csv: (r) => `${r.period_start}/${r.period_end}` },
    { key: "lines", header: "Lines", align: "right", wide: true, cell: (r) => r.lines, csv: (r) => r.lines },
    { key: "when", header: "Pulled", wide: true, cell: (r) => formatDateTime(r.started_at), csv: (r) => r.started_at },
    { key: "result", header: "Result", cell: (r) => (r.error ? <span className="text-exception">{r.error}</span> : <Badge tone="posted">bill in</Badge>), csv: (r) => r.error ?? "ok" },
  ];
  return (
    <>
      <p className="mb-3 text-sm">
        {set.length ? <>Set up: {set.map((c) => cloudName[c]).join(", ")}. Each pulls the cloud's bill every day.</> : <>No cloud bill connector is set up. They come with the Enterprise plan on request.</>}
      </p>
      {runs.length ? <DataTable stack columns={columns} rows={runs} rowKey={(r) => `${r.provider}${r.started_at}`} csvName="cloud-bill-pulls.csv" caption="Cloud bill pulls" /> : null}
    </>
  );
}

/** The Enterprise plan sections of Settings, for org managers on an Enterprise plan instance. */
export function EnterpriseSettings(): ReactNode {
  const license = useLicense();
  if (!license.data) return license.isError ? <ErrorBox error={license.error} /> : null;
  const on = license.data.edition === "enterprise";
  return (
    <>
      <Section title="Enterprise plan">
        <LicenseFacts license={license.data} />
      </Section>
      {on ? (
        <>
          <Section title="Sign-in" lead="Members sign in with your identity provider, such as Entra ID, Okta or Google Workspace. The magic link keeps working.">
            <SignInProvider />
          </Section>
          <Section title="SCIM" lead="People you add or remove at your identity provider are added or removed as members.">
            <Scim />
          </Section>
          <Section title="Audit log" lead="Who changed what, newest first.">
            <AuditLog />
          </Section>
          <Section title="Cloud bill connectors" lead="Bills from Bedrock, Vertex and Foundry, pulled from the cloud.">
            <CloudConnectors />
          </Section>
        </>
      ) : null}
    </>
  );
}
