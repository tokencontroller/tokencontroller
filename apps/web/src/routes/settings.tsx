import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { ApiToken, ApiTokenKind, CostSource, TicketSystem } from "@/api/contract";
import { api } from "@/api/endpoints";
import { CadenceSelect, describeCadence, type CadenceChoice } from "@/components/cadence-select";
import { EnterpriseSettings } from "@/components/enterprise-settings";
import { currencies } from "@/routes/sign-in";
import { saveCadence, useCadences } from "@/routes/projects";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { Loading, Note, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select } from "@/components/ui/form";
import { Switch } from "@/components/ui/switch";
import { DataTable, type Column } from "@/components/ui/table";
import { notify } from "@/components/ui/toast";
import { costSourceOrder, costSourceText, describeCostSource } from "@/lib/cost-source";
import { additions } from "@/lib/additions";
import { downloadFile } from "@/lib/csv";
import { displayName, formatDate, formatDateTime } from "@/lib/format";
import { refreshAll, useMe, usePeople, usePersonName, useProjects } from "@/lib/queries";
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
  const cadences = useCadences();
  const current = cadences.data?.find((c) => !c.project) ?? null;
  const update = useMutation({ mutationFn: api.updateOrganization, onSuccess: async () => { await refreshAll(); notify("Saved"); } });
  const setCadence = useMutation({
    mutationFn: (choice: CadenceChoice | null) => saveCadence(cadences.data ?? [], null, choice),
    onSuccess: async () => { await refreshAll(); notify("Cadence saved"); },
  });
  const [name, setName] = useState(org.name);
  const [domains, setDomains] = useState(org.sign_in_domains.join(", "));
  return (
    <div className="rounded-lg border border-line px-4">
      <Row label="Name" hint="Shown in the sidebar, in invites and on the cost sheet.">
        <form className="flex gap-2" onSubmit={(e) => { e.preventDefault(); update.mutate({ name }); }}>
          <Input aria-label="Organization name" required value={name} onChange={(e) => setName(e.target.value)} />
          <Button type="submit">Save</Button>
        </form>
      </Row>
      <Row label="Cost source" hint={`The close reconciles against ${describeCostSource(org.cost_source).against}.`}>
        <Select aria-label="Cost source" value={org.cost_source ?? ""} onChange={(e) => update.mutate({ cost_source: (e.target.value || null) as CostSource | null })}>
          {costSourceOrder.map((source) => (
            <option key={source} value={source}>
              {costSourceText[source].name}
            </option>
          ))}
          <option value="">None yet</option>
        </Select>
      </Row>
      <Row label="Reporting currency" hint="The one currency the organization computes in. Bills in other currencies are booked at the rate of their date.">
        <Select aria-label="Reporting currency" value={org.reporting_currency} onChange={(e) => update.mutate({ reporting_currency: e.target.value })}>
          {currencies.map((c) => (
            <option key={c}>{c}</option>
          ))}
        </Select>
      </Row>
      <Row label="Approval" hint="With approval on, a period cannot be closed while a token entry in it is unapproved.">
        <Switch label="Approval" checked={org.approval} onCheckedChange={(approval) => update.mutate({ approval })} />
      </Row>
      <Row label="Cadence for the whole organization" hint={`Token sheets are due ${describeCadence(current).toLowerCase()}. A project's own cadence wins.`}>
        {cadences.isPending ? <Loading /> : <CadenceSelect value={current} onChange={(c) => setCadence.mutate(c)} />}
      </Row>
      <Row label="Sign-in domains" hint="Magic link sign-in limited to these email domains. Empty means any.">
        <form className="flex gap-2" onSubmit={(e) => { e.preventDefault(); update.mutate({ sign_in_domains: domains.split(/[\s,]+/).filter(Boolean) }); }}>
          <Input aria-label="Sign-in domains" value={domains} onChange={(e) => setDomains(e.target.value)} placeholder="company.com" />
          <Button type="submit">Save</Button>
        </form>
      </Row>
      <FormError error={update.error ?? setCadence.error} />
    </div>
  );
}

const systemName: Record<TicketSystem["kind"], string> = { jira: "Jira Cloud", azure_devops: "Azure DevOps", github_issues: "GitHub Issues" };

/** What the connect form asks for the address, per system (the service checks it). */
const siteField: Record<TicketSystem["kind"], { label: string; placeholder: string; hint?: string }> = {
  jira: { label: "Jira Cloud site", placeholder: "https://company.atlassian.net" },
  azure_devops: { label: "Organization URL", placeholder: "https://dev.azure.com/company" },
  github_issues: { label: "API address", placeholder: "https://api.github.com", hint: "Keep it as it is, unless your company's GitHub has its own address, such as https://api.company.ghe.com." },
};

const GITHUB_API = "https://api.github.com";

type ConnectForm = { kind: TicketSystem["kind"]; site: string; email: string; token: string };

/** `company.atlassian.net` works as well as the full address. */
function withScheme(site: string): string {
  const trimmed = site.trim();
  return /^[a-z][a-z0-9+.-]*:\/\//i.test(trimmed) ? trimmed : `https://${trimmed}`;
}

function TicketSystems() {
  const systems = useQuery({ queryKey: ["ticket-systems"], queryFn: api.ticketSystems });
  const sync = useMutation({ mutationFn: api.syncTickets, onSuccess: async () => { await refreshAll(); notify("Sync started", "New tickets show up in a minute."); } });
  const disconnect = useMutation({ mutationFn: api.disconnectTicketSystem, onSuccess: async () => { await refreshAll(); setLeaving(null); notify("Disconnected", "Its tickets stay and stop syncing."); } });
  const [form, setForm] = useState<ConnectForm | null>(null);
  const [mapping, setMapping] = useState<TicketSystem | null>(null);
  const [leaving, setLeaving] = useState<TicketSystem | null>(null);
  const connect = useMutation({
    mutationFn: (f: ConnectForm) =>
      api.connectTicketSystem({ kind: f.kind, site: withScheme(f.site), token: f.token, ...(f.kind === "jira" ? { email: f.email.trim() } : {}) }),
    onSuccess: async (system) => {
      await refreshAll();
      setForm(null);
      setMapping(system);
    },
  });
  const pick = (kind: TicketSystem["kind"]) => {
    if (!form) return;
    connect.reset();
    setForm({ ...form, kind, site: kind === "github_issues" ? GITHUB_API : form.site === GITHUB_API ? "" : form.site });
  };
  return (
    <>
      {systems.data?.length ? (
        <ul className="mb-3 divide-y divide-line rounded-lg border border-line">
          {systems.data.map((s) => (
            <li key={s.id} className="flex flex-col gap-2 px-4 py-3 sm:flex-row sm:items-center sm:justify-between">
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2 text-sm font-medium">
                  <span>{systemName[s.kind]}</span>
                  <span className="break-all font-mono text-[13px] font-normal">{s.site}</span>
                  {s.status === "failing" ? <Badge tone="exception">failing</Badge> : null}
                </div>
                <div className="text-xs text-muted">
                  {s.mappings.map((m) => `${m.external_name ?? m.external} → ${m.project}`).join(", ") || "No projects mapped yet, so nothing syncs"} · last sync {s.last_sync_at ? formatDateTime(s.last_sync_at) : "never"} · token …{s.key_hint}
                </div>
                {s.status === "failing" && s.message ? <div className="mt-1 text-xs text-exception">{s.message}</div> : null}
              </div>
              <div className="flex shrink-0 gap-2">
                <Button size="sm" onClick={() => setMapping(s)}>
                  Map projects
                </Button>
                <Button size="sm" onClick={() => sync.mutate(s.id)} disabled={sync.isPending || !s.mappings.length}>
                  Sync now
                </Button>
                <Button size="sm" onClick={() => setLeaving(s)}>
                  Disconnect
                </Button>
              </div>
            </li>
          ))}
        </ul>
      ) : (
        <p className="mb-3 text-sm text-muted">No ticket system connected. Tickets can also be created by hand in a project.</p>
      )}
      <FormError error={sync.error} />
      <Button onClick={() => { connect.reset(); setForm({ kind: "jira", site: "", email: "", token: "" }); }}>Connect a ticket system</Button>
      <Dialog open={Boolean(form)} onOpenChange={(o) => !o && setForm(null)} title="Connect a ticket system" description="Whoever connects a ticket system answers for that data flow. Only titles, descriptions, parents and assignees sync.">
        {form ? (
          <form className="space-y-4" onSubmit={(e) => { e.preventDefault(); connect.mutate(form); }}>
            <Field label="System">
              <Select value={form.kind} onChange={(e) => pick(e.target.value as TicketSystem["kind"])}>
                {Object.entries(systemName).map(([k, v]) => (
                  <option key={k} value={k}>
                    {v}
                  </option>
                ))}
              </Select>
            </Field>
            <Field label={siteField[form.kind].label} hint={siteField[form.kind].hint}>
              <Input required value={form.site} onChange={(e) => setForm({ ...form, site: e.target.value })} placeholder={siteField[form.kind].placeholder} />
            </Field>
            {form.kind === "jira" ? (
              <Field label="Email of the Atlassian account" hint="Jira Cloud signs in with the token and the email of the account it belongs to.">
                <Input type="email" required autoComplete="off" value={form.email} onChange={(e) => setForm({ ...form, email: e.target.value })} />
              </Field>
            ) : null}
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
      {mapping ? <MapProjects system={mapping} onClose={() => setMapping(null)} /> : null}
      <Dialog open={Boolean(leaving)} onOpenChange={(o) => !o && setLeaving(null)} title={`Disconnect ${leaving ? systemName[leaving.kind] : ""}?`} description="Its tickets stay, with their token entries, and stop syncing. You can connect it again later.">
        <FormError error={disconnect.error} />
        <div className="flex justify-end gap-2">
          <Button onClick={() => setLeaving(null)}>Cancel</Button>
          <Button variant="primary" disabled={disconnect.isPending} onClick={() => leaving && disconnect.mutate(leaving.id)}>
            Disconnect
          </Button>
        </div>
      </Dialog>
    </>
  );
}

/** Which of the ticket system's projects feed which of ours. Unmapped ones do not sync. */
function MapProjects({ system, onClose }: { system: TicketSystem; onClose: () => void }) {
  const external = useQuery({ queryKey: ["ticket-systems", system.id, "projects"], queryFn: () => api.externalProjects(system.id), retry: false });
  const projects = useProjects();
  const [chosen, setChosen] = useState<Record<string, string>>(() => Object.fromEntries(system.mappings.map((m) => [m.external, m.project])));
  const save = useMutation({
    mutationFn: () => {
      // In the ticket system's order; a mapping the token no longer sees stays.
      const listed = external.data ?? [];
      const known = [...listed, ...system.mappings.filter((m) => !listed.some((p) => p.key === m.external)).map((m) => ({ key: m.external, name: m.external_name ?? m.external }))];
      const mappings = known.flatMap((p) => (chosen[p.key] ? [{ external: p.key, external_name: p.name, project: chosen[p.key] ?? "" }] : []));
      return api.mapProjects(system.id, mappings);
    },
    onSuccess: async () => {
      await refreshAll();
      notify("Projects mapped", "Their tickets sync now, then every 15 minutes.");
      onClose();
    },
  });
  const ours = (projects.data ?? []).filter((p) => !p.archived);
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()} title={`Map ${systemName[system.kind]} projects`} description="Pick the Token Controller project each one feeds. A project without a ticket key pattern gets the one its tickets use. Unmapped projects do not sync.">
      {external.isPending || projects.isPending ? (
        <Loading />
      ) : external.error ? (
        <FormError error={external.error} />
      ) : external.data.length ? (
        <form className="space-y-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
          <ul className="divide-y divide-line rounded-lg border border-line">
            {external.data.map((p) => (
              <li key={p.key} className="flex flex-col gap-2 px-3 py-2.5 sm:flex-row sm:items-center sm:justify-between">
                <div className="min-w-0 text-sm">
                  <span className="font-medium">{p.name}</span> <span className="break-all font-mono text-[13px] text-muted">{p.key}</span>
                </div>
                <Select aria-label={`Project for ${p.name}`} className="sm:w-52" value={chosen[p.key] ?? ""} onChange={(e) => setChosen({ ...chosen, [p.key]: e.target.value })}>
                  <option value="">Not mapped</option>
                  {ours.map((o) => (
                    <option key={o.id} value={o.name}>
                      {o.name}
                    </option>
                  ))}
                </Select>
              </li>
            ))}
          </ul>
          <FormError error={save.error} />
          <div className="flex justify-end gap-2">
            <Button onClick={onClose}>Cancel</Button>
            <Button type="submit" variant="primary" disabled={save.isPending}>
              Save mapping
            </Button>
          </div>
        </form>
      ) : (
        <p className="text-sm text-muted">The token sees no projects. Give it read access to the projects you want to sync.</p>
      )}
    </Dialog>
  );
}

const kindText: Record<ApiTokenKind, { name: string; hint: string }> = {
  service: { name: "Service token", hint: "For an agent that reports token entries, such as a CI bot. You become its agent manager." },
  read: { name: "Read token", hint: "For the reporting API in Excel, Google Sheets or Power BI. It sees what your role sees." },
  organization: { name: "Organization token", hint: "For telemetry push from managed settings." },
  cli: { name: "CLI token", hint: "Made by tc connect on one computer. Revoke it to sign that computer out." },
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
    mutationFn: () => api.createApiToken(creating?.kind ?? "service", creating?.name ?? ""),
    onSuccess: async (r) => {
      await refreshAll();
      setCreating(null);
      setSecret(r.token);
    },
  });
  const revoke = useMutation({ mutationFn: api.revokeApiToken, onSuccess: async () => { await refreshAll(); notify("Token revoked"); } });
  const move = useMutation({
    mutationFn: () => api.handOverApiToken(moving?.id ?? "", owner),
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
    { key: "prefix", header: "Starts with", wide: true, cell: (t) => <span className="font-mono text-[13px]">{t.shown_prefix}…</span>, csv: (t) => t.shown_prefix },
    {
      key: "owner",
      header: "Belongs to",
      cell: (t) => (t.needs_owner ? <Badge tone="exception">{name(t.person_id)} left</Badge> : name(t.person_id)),
      csv: (t) => t.person_id,
    },
    { key: "used", header: "Last used", wide: true, cell: (t) => (t.last_used_at ? formatDate(t.last_used_at) : "never"), csv: (t) => t.last_used_at ?? "" },
    {
      key: "actions",
      header: "",
      cell: (t) => (
        <span className="flex justify-end gap-1">
          {controller && t.kind === "service" ? <Button size="sm" variant="ghost" onClick={() => { setOwner(""); setMoving(t); }}>Move</Button> : null}
          <Button size="sm" variant="ghost" onClick={() => revoke.mutate(t.id)}>Revoke</Button>
        </span>
      ),
    },
  ];
  return (
    <>
      {tokens.data?.some((t) => t.needs_owner && t.kind === "service") ? (
        <Note tone="exception">A service token belongs to someone who left, so its token entries show as an error. {controller ? "Move it to someone." : "A controller can move it to someone."}</Note>
      ) : null}
      {tokens.isPending ? <Loading /> : <DataTable stack columns={columns} rows={tokens.data ?? []} rowKey={(t) => t.id} caption="API tokens" empty="No tokens yet." />}
      <div className="mt-3 flex flex-wrap gap-2">
        <Button onClick={() => setCreating({ kind: "service", name: "" })}>New service token</Button>
        <Button onClick={() => setCreating({ kind: "read", name: "" })}>New read token</Button>
        {controller ? <Button onClick={() => setCreating({ kind: "organization", name: "Telemetry push" })}>New organization token</Button> : null}
      </div>
      <Dialog open={Boolean(creating)} onOpenChange={(o) => !o && setCreating(null)} title={creating ? `New ${kindText[creating.kind].name.toLowerCase()}` : ""} description={creating ? kindText[creating.kind].hint : undefined}>
        {creating ? (
          <form className="space-y-4" onSubmit={(e) => { e.preventDefault(); create.mutate(); }}>
            <Field label="Name">
              <Input required autoFocus value={creating.name} onChange={(e) => setCreating({ ...creating, name: e.target.value })} placeholder={creating.kind === "service" ? "review bot (GitHub Action)" : "Power BI"} />
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
            {(people.data ?? []).filter((p) => p.status === "joined" && p.id !== moving?.person_id).map((p) => (
              <option key={p.id} value={p.id}>
                {displayName(p)}
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

export function SettingsPage(): ReactNode {
  const me = useMe();
  const controller = hasRole(me.person, "controller");
  const { SettingsPlan } = additions.parts;
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
      {controller ? <EnterpriseSettings /> : null}
      {controller ? (
        <Section title="Ticket systems" lead="Jira Cloud, Azure DevOps and GitHub Issues. Tickets sync every 15 minutes, or when you ask.">
          <TicketSystems />
        </Section>
      ) : null}
      <Section id="api-tokens" title="API tokens" lead="Service tokens report token entries, read tokens read reports, the organization token takes telemetry.">
        <ApiTokens />
      </Section>
      {controller && SettingsPlan ? <SettingsPlan /> : null}
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
