import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { ApiToken, ApiTokenKind, CostSource, TicketSystem } from "@/api/contract";
import { api } from "@/api/endpoints";
import { costSources } from "@/routes/sign-in";
import { CadenceSelect, describeCadence } from "@/routes/projects";
import { costSourceText } from "@/routes/bills";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { Loading, Note, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select } from "@/components/ui/form";
import { Switch } from "@/components/ui/switch";
import { DataTable, type Column } from "@/components/ui/table";
import { notify } from "@/components/ui/toast";
import { downloadFile } from "@/lib/csv";
import { formatDate, formatDateTime } from "@/lib/format";
import { refreshAll, useMe, usePeople, usePersonName } from "@/lib/queries";
import { hasRole } from "@/lib/roles";
import { today } from "@/lib/today";

function Row({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-2 border-b border-line py-4 last:border-0 sm:flex-row sm:items-center sm:justify-between sm:gap-6">
      <div className="max-w-md">
        <div className="text-sm font-medium">{label}</div>
        {hint ? <div className="mt-0.5 text-[13px] text-muted">{hint}</div> : null}
      </div>
      <div className="sm:w-96 sm:shrink-0">{children}</div>
    </div>
  );
}

function OrganizationSettings() {
  const me = useMe();
  const org = me.organization;
  const cadence = useQuery({ queryKey: ["org-cadence"], queryFn: api.organizationCadence });
  const update = useMutation({ mutationFn: api.updateOrganization, onSuccess: async () => { await refreshAll(); notify("Saved"); } });
  const setCadence = useMutation({ mutationFn: api.setOrganizationCadence, onSuccess: async () => { await refreshAll(); notify("Cadence saved"); } });
  const [domains, setDomains] = useState(org.sign_in_domains.join(", "));
  return (
    <div className="rounded-lg border border-line px-4">
      <Row label="Cost source" hint={`The close reconciles against ${costSourceText[org.cost_source].against}.`}>
        <Select aria-label="Cost source" value={org.cost_source} onChange={(e) => update.mutate({ cost_source: e.target.value as CostSource })}>
          {costSources.map((c) => (
            <option key={c.value} value={c.value}>
              {c.label}
            </option>
          ))}
        </Select>
      </Row>
      <Row label="Reporting currency" hint="The one currency the organization computes in. Bills in other currencies are booked at the rate of their date.">
        <Select aria-label="Reporting currency" value={org.reporting_currency} onChange={(e) => update.mutate({ reporting_currency: e.target.value })}>
          {["USD", "EUR", "GBP", "CHF"].map((c) => (
            <option key={c}>{c}</option>
          ))}
        </Select>
      </Row>
      <Row label="Approval" hint="With approval on, a period cannot be closed while a token entry in it is unapproved.">
        <Switch label="Approval" checked={org.approval} onCheckedChange={(approval) => update.mutate({ approval })} />
      </Row>
      <Row label="Cadence for the whole organization" hint={`Token sheets are due ${describeCadence(cadence.data ?? null).toLowerCase()}. A project's own cadence wins.`}>
        {cadence.isPending ? <Loading /> : <CadenceSelect value={cadence.data ?? null} onChange={(c) => setCadence.mutate(c)} />}
      </Row>
      <Row label="Sign-in domains" hint="Magic link sign-in limited to these email domains. Empty means any.">
        <form className="flex gap-2" onSubmit={(e) => { e.preventDefault(); update.mutate({ sign_in_domains: domains.split(/[\s,]+/).filter(Boolean) }); }}>
          <Input aria-label="Sign-in domains" value={domains} onChange={(e) => setDomains(e.target.value)} placeholder="company.com" />
          <Button type="submit">Save</Button>
        </form>
      </Row>
      <Row label="Token forecasts" hint="Compare new tickets with the summaries of past token entries. A self-hosted instance can run without them.">
        <Switch label="Token forecasts" checked={org.forecasts} onCheckedChange={(forecasts) => update.mutate({ forecasts })} />
      </Row>
      <FormError error={update.error ?? setCadence.error} />
    </div>
  );
}

const systemName: Record<TicketSystem["kind"], string> = { jira: "Jira Cloud", azure_devops: "Azure DevOps", github_issues: "GitHub Issues" };

function TicketSystems() {
  const systems = useQuery({ queryKey: ["ticket-systems"], queryFn: api.ticketSystems });
  const sync = useMutation({ mutationFn: api.syncTickets, onSuccess: async () => { await refreshAll(); notify("Tickets synced"); } });
  const [form, setForm] = useState<{ kind: TicketSystem["kind"]; site: string; token: string } | null>(null);
  const connect = useMutation({
    mutationFn: () => api.connectTicketSystem(form ?? { kind: "jira", site: "", token: "" }),
    onSuccess: async () => {
      await refreshAll();
      notify("Connected", "Map its projects to yours; tickets sync every 15 minutes.");
      setForm(null);
    },
  });
  return (
    <>
      {systems.data?.length ? (
        <ul className="mb-3 divide-y divide-line rounded-lg border border-line">
          {systems.data.map((s) => (
            <li key={s.id} className="flex flex-col gap-2 px-4 py-3 sm:flex-row sm:items-center sm:justify-between">
              <div>
                <div className="text-sm font-medium">
                  {systemName[s.kind]} · <span className="font-mono text-[13px]">{s.site}</span>
                </div>
                <div className="text-xs text-muted">
                  {s.mappings.map((m) => `${m.external} → ${m.project}`).join(", ") || "No projects mapped yet"} · last sync {s.last_sync_at ? formatDateTime(s.last_sync_at) : "never"}
                </div>
              </div>
              <Button size="sm" onClick={() => sync.mutate(s.id)} disabled={sync.isPending}>
                Sync now
              </Button>
            </li>
          ))}
        </ul>
      ) : (
        <p className="mb-3 text-sm text-muted">No ticket system connected. Tickets can also be created by hand in a project.</p>
      )}
      <Button onClick={() => setForm({ kind: "jira", site: "", token: "" })}>Connect a ticket system</Button>
      <Dialog open={Boolean(form)} onOpenChange={(o) => !o && setForm(null)} title="Connect a ticket system" description="Whoever connects a ticket system answers for that data flow. Only titles, descriptions, parents and assignees sync.">
        {form ? (
          <form className="space-y-4" onSubmit={(e) => { e.preventDefault(); connect.mutate(); }}>
            <Field label="System">
              <Select value={form.kind} onChange={(e) => setForm({ ...form, kind: e.target.value as TicketSystem["kind"] })}>
                {Object.entries(systemName).map(([k, v]) => (
                  <option key={k} value={k}>
                    {v}
                  </option>
                ))}
              </Select>
            </Field>
            <Field label={form.kind === "github_issues" ? "Repository or organization" : form.kind === "azure_devops" ? "Organization URL" : "Site"}>
              <Input required value={form.site} onChange={(e) => setForm({ ...form, site: e.target.value })} placeholder={form.kind === "jira" ? "company.atlassian.net" : form.kind === "azure_devops" ? "dev.azure.com/company" : "company/repository"} />
            </Field>
            <Field label="API token" hint="A read-only token is enough.">
              <Input type="password" required autoComplete="off" value={form.token} onChange={(e) => setForm({ ...form, token: e.target.value })} />
            </Field>
            <FormError error={connect.error} />
            <div className="flex justify-end gap-2">
              <Button onClick={() => setForm(null)}>Cancel</Button>
              <Button type="submit" variant="primary" disabled={connect.isPending}>
                Connect
              </Button>
            </div>
          </form>
        ) : null}
      </Dialog>
    </>
  );
}

const kindText: Record<ApiTokenKind, { name: string; hint: string }> = {
  svc: { name: "Service token", hint: "For an agent that reports token entries, such as a CI bot. You become its agent manager." },
  read: { name: "Read token", hint: "For the reporting API in Excel, Google Sheets or Power BI. It sees what your role sees." },
  org: { name: "Organization token", hint: "For telemetry push from managed settings." },
};

function ApiTokens() {
  const me = useMe();
  const tokens = useQuery({ queryKey: ["api-tokens"], queryFn: api.apiTokens });
  const people = usePeople();
  const name = usePersonName();
  const [creating, setCreating] = useState<{ kind: ApiTokenKind; name: string } | null>(null);
  const [secret, setSecret] = useState<string | null>(null);
  const [moving, setMoving] = useState<ApiToken | null>(null);
  const [owner, setOwner] = useState("");
  const create = useMutation({
    mutationFn: () => api.createApiToken(creating?.kind ?? "svc", creating?.name ?? ""),
    onSuccess: async (r) => {
      await refreshAll();
      setCreating(null);
      setSecret(r.secret);
    },
  });
  const revoke = useMutation({ mutationFn: api.revokeApiToken, onSuccess: async () => { await refreshAll(); notify("Token revoked"); } });
  const move = useMutation({
    mutationFn: () => api.moveApiToken(moving?.id ?? "", owner),
    onSuccess: async () => {
      await refreshAll();
      notify("Service token moved", "Only future token entries follow.");
      setMoving(null);
    },
  });
  const controller = hasRole(me.person, "controller");
  const columns: Column<ApiToken>[] = [
    { key: "name", header: "Name", cell: (t) => <span className="font-medium">{t.name}</span>, csv: (t) => t.name },
    { key: "kind", header: "Kind", cell: (t) => <Badge>{kindText[t.kind].name}</Badge>, csv: (t) => t.kind },
    { key: "prefix", header: "Starts with", wide: true, cell: (t) => <span className="font-mono text-[13px]">{t.prefix}…</span>, csv: (t) => t.prefix },
    { key: "owner", header: "Agent manager", cell: (t) => (t.owner ? name(t.owner) : <Badge tone="exception">nobody</Badge>), csv: (t) => t.owner ?? "" },
    { key: "used", header: "Last used", wide: true, cell: (t) => (t.last_used_at ? formatDate(t.last_used_at) : "never"), csv: (t) => t.last_used_at ?? "" },
    {
      key: "actions",
      header: "",
      cell: (t) => (
        <span className="flex justify-end gap-1">
          {controller && t.kind === "svc" ? <Button size="sm" variant="ghost" onClick={() => { setOwner(t.owner ?? ""); setMoving(t); }}>Move</Button> : null}
          <Button size="sm" variant="ghost" onClick={() => revoke.mutate(t.id)}>Revoke</Button>
        </span>
      ),
    },
  ];
  return (
    <>
      {tokens.data?.some((t) => t.owner === null) ? <Note tone="exception">A service token belongs to nobody, so its token entries have no agent manager. Move it to someone.</Note> : null}
      {tokens.isPending ? <Loading /> : <DataTable stack columns={columns} rows={tokens.data ?? []} rowKey={(t) => t.id} caption="API tokens" empty="No tokens yet." />}
      <div className="mt-3 flex flex-wrap gap-2">
        <Button onClick={() => setCreating({ kind: "svc", name: "" })}>New service token</Button>
        <Button onClick={() => setCreating({ kind: "read", name: "" })}>New read token</Button>
        {controller ? <Button onClick={() => setCreating({ kind: "org", name: "Telemetry push" })}>New organization token</Button> : null}
      </div>
      <Dialog open={Boolean(creating)} onOpenChange={(o) => !o && setCreating(null)} title={creating ? `New ${kindText[creating.kind].name.toLowerCase()}` : ""} description={creating ? kindText[creating.kind].hint : undefined}>
        {creating ? (
          <form className="space-y-4" onSubmit={(e) => { e.preventDefault(); create.mutate(); }}>
            <Field label="Name">
              <Input required autoFocus value={creating.name} onChange={(e) => setCreating({ ...creating, name: e.target.value })} placeholder={creating.kind === "svc" ? "review bot (GitHub Action)" : "Power BI"} />
            </Field>
            <FormError error={create.error} />
            <div className="flex justify-end gap-2">
              <Button onClick={() => setCreating(null)}>Cancel</Button>
              <Button type="submit" variant="primary" disabled={create.isPending}>
                Create token
              </Button>
            </div>
          </form>
        ) : null}
      </Dialog>
      <Dialog open={Boolean(secret)} onOpenChange={() => setSecret(null)} title="Copy your token now" description="It is shown once. Store it where the agent or tool reads it.">
        <code className="block break-all rounded-md bg-sand p-3 font-mono text-[13px]" data-testid="token-secret">
          {secret}
        </code>
        <div className="mt-4 flex justify-end gap-2">
          <Button onClick={() => void navigator.clipboard.writeText(secret ?? "").then(() => notify("Copied"))}>Copy</Button>
          <Button variant="primary" onClick={() => setSecret(null)}>
            Done
          </Button>
        </div>
      </Dialog>
      <Dialog open={Boolean(moving)} onOpenChange={(o) => !o && setMoving(null)} title={`Move ${moving?.name ?? ""}`} description="The new person becomes the agent manager of its future token entries. Past ones keep theirs.">
        <Field label="Agent manager">
          <Select value={owner} onChange={(e) => setOwner(e.target.value)}>
            <option value="">Pick a person</option>
            {(people.data ?? []).filter((p) => p.status === "active").map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </Select>
        </Field>
        <FormError error={move.error} />
        <div className="mt-5 flex justify-end gap-2">
          <Button onClick={() => setMoving(null)}>Cancel</Button>
          <Button variant="primary" disabled={!owner || move.isPending} onClick={() => move.mutate()}>
            Move token
          </Button>
        </div>
      </Dialog>
    </>
  );
}

function Plan() {
  const me = useMe();
  const org = me.organization;
  const [interval, setInterval] = useState<"month" | "year">("month");
  const checkout = useMutation({ mutationFn: () => api.checkout(interval), onSuccess: ({ url }) => window.location.assign(url) });
  const portal = useMutation({ mutationFn: api.billingPortal, onSuccess: ({ url }) => window.location.assign(url) });
  const name = { solo: "Solo", team: "Team plan", enterprise: "Enterprise plan", self_hosted: "Self-hosted" }[org.plan];
  return (
    <div className="rounded-lg border border-line p-4">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <div className="text-sm font-medium">{name}</div>
          <div className="text-[13px] text-muted">
            {org.plan === "solo"
              ? "Free forever for one person, every feature, no invites."
              : org.plan === "team"
                ? `${org.members} of ${org.member_limit ?? 150} members. Roles, review, approval, monthly close, ticket sync.`
                : "Billed by invoice."}
          </div>
        </div>
        {org.plan === "solo" ? (
          <div className="flex flex-wrap items-center gap-2">
            <Select aria-label="Billing interval" value={interval} onChange={(e) => setInterval(e.target.value as "month" | "year")} className="w-auto">
              <option value="month">Monthly</option>
              <option value="year">Yearly</option>
            </Select>
            <Button variant="primary" onClick={() => checkout.mutate()} disabled={checkout.isPending}>
              Choose Team plan
            </Button>
          </div>
        ) : org.plan === "team" ? (
          <Button onClick={() => portal.mutate()} disabled={portal.isPending}>
            Manage payment and invoices
          </Button>
        ) : null}
      </div>
      {org.plan === "solo" ? <p className="mt-3 text-xs text-muted">The Team plan is paid by card through Stripe. Customers in the EU pay in EUR, everyone else in USD. The price shows at checkout.</p> : null}
      <FormError error={checkout.error ?? portal.error} />
    </div>
  );
}

export function SettingsPage(): ReactNode {
  const me = useMe();
  const controller = hasRole(me.person, "controller");
  const exportRaw = useMutation({
    mutationFn: api.rawExport,
    onSuccess: (blob) => downloadFile(`token-controller-export-${today()}.${blob.type.includes("zip") ? "zip" : "json"}`, blob, blob.type),
  });
  return (
    <>
      <PageHeader title="Settings" lead={controller ? "What a controller sets up once, plus tokens for agents and spreadsheets." : "Tokens for your agents and spreadsheets."} />
      {controller ? (
        <Section title="Organization">
          <OrganizationSettings />
        </Section>
      ) : null}
      {controller ? (
        <Section title="Ticket systems" lead="Jira Cloud, Azure DevOps and GitHub Issues. Tickets sync every 15 minutes, or when you ask.">
          <TicketSystems />
        </Section>
      ) : null}
      <Section title="API tokens" lead="Service tokens report token entries, read tokens read reports, the organization token takes telemetry.">
        <ApiTokens />
      </Section>
      {controller && me.organization.billing ? (
        <Section title="Plan">
          <Plan />
        </Section>
      ) : null}
      {controller ? (
        <Section title="Raw export" lead="Everything your organization has, as a zip of JSON: every token entry, split, tag, bill, rule, ticket and period.">
          <Button onClick={() => exportRaw.mutate()} disabled={exportRaw.isPending}>
            Download raw export
          </Button>
          <FormError error={exportRaw.error} />
        </Section>
      ) : null}
    </>
  );
}
