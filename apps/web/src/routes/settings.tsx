import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { ApiTokenKind } from "@/api/contract";
import { api } from "@/api/endpoints";
import { EnterpriseSettings } from "@/components/enterprise-settings";
import { Button } from "@/components/ui/button";
import { Loading, Note, PageHeader, Section } from "@/components/ui/display";
import { Badge } from "@/components/ui/badge";
import { Dialog } from "@/components/ui/dialog";
import { Field, FormError, Input, Select, SettingRow } from "@/components/ui/form";
import { Switch } from "@/components/ui/switch";
import { DataTable } from "@/components/ui/table";
import { notify } from "@/components/ui/toast";
import { additions } from "@/lib/additions";
import { downloadFile } from "@/lib/csv";
import { formatDateTime } from "@/lib/format";
import { refreshAll, useMe, usePersonName } from "@/lib/queries";
import { hasRole } from "@/lib/roles";

const tokenKinds: { kind: ApiTokenKind; label: string; orgManager?: boolean }[] = [
  { kind: "service", label: "Service token: CI and automation report as you" },
  { kind: "read", label: "Read token: reads what you may view" },
  { kind: "organization", label: "Organization token", orgManager: true },
];

// Docs: Organizations; Roles (the person of a message). The organization's
// settings, API tokens and the raw export of the log.
export function SettingsPage(): ReactNode {
  const me = useMe();
  const orgManager = hasRole(me.person, "org_manager");
  return (
    <>
      <PageHeader title="Settings" />
      {orgManager ? <OrganizationSettings /> : null}
      {orgManager && additions.parts.SettingsPlan ? <additions.parts.SettingsPlan /> : null}
      <ApiTokens />
      {orgManager ? <RawExport /> : null}
      {orgManager ? <EnterpriseSettings /> : null}
    </>
  );
}

function OrganizationSettings() {
  const me = useMe();
  const org = me.organization;
  const [name, setName] = useState(org.name);
  const [currency, setCurrency] = useState(org.reporting_currency);
  const [domains, setDomains] = useState(org.sign_in_domains.join(", "));
  const save = useMutation({
    mutationFn: () => api.updateOrganization({ name, reporting_currency: currency.toUpperCase(), sign_in_domains: domains.split(/[\s,]+/).filter(Boolean) }),
    onSuccess: async () => {
      await refreshAll();
      notify("Saved");
    },
  });
  const approval = useMutation({
    mutationFn: (on: boolean) => api.updateOrganization({ approval: on }),
    onSuccess: async (o) => {
      await refreshAll();
      notify(o.approval ? "Approval is on" : "Approval is off");
    },
  });
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        save.mutate();
      }}
    >
      <Section
        title="Organization"
        footer={
          <>
            <FormError error={save.error} />
            <Button type="submit" variant="primary" disabled={save.isPending}>Save changes</Button>
          </>
        }
      >
        <SettingRow label="Name" htmlFor="org-name">
          <Input id="org-name" aria-label="Name" value={name} onChange={(e) => setName(e.target.value)} />
        </SettingRow>
        <SettingRow label="Reporting currency" htmlFor="org-currency" description="Reports show cost in it.">
          <Input id="org-currency" aria-label="Reporting currency" className="w-28" value={currency} onChange={(e) => setCurrency(e.target.value)} />
        </SettingRow>
        <SettingRow label="Sign-in domains" htmlFor="org-domains" description="Empty: any.">
          <Input id="org-domains" aria-label="Sign-in domains" value={domains} onChange={(e) => setDomains(e.target.value)} />
        </SettingRow>
        <SettingRow label="Submissions need approval" description="A people manager approves each submission.">
          <Switch checked={org.approval} onCheckedChange={(on) => approval.mutate(on)} label="Submissions need approval" />
          <FormError error={approval.error} />
        </SettingRow>
      </Section>
    </form>
  );
}

function ApiTokens() {
  const me = useMe();
  const tokens = useQuery({ queryKey: ["api-tokens"], queryFn: api.apiTokens });
  const personName = usePersonName();
  const [kind, setKind] = useState<ApiTokenKind>("service");
  const [name, setName] = useState("");
  const [created, setCreated] = useState<string | null>(null);
  const [open, setOpen] = useState(false);
  const create = useMutation({
    mutationFn: () => api.createApiToken(kind, name),
    onSuccess: async (t) => {
      await refreshAll();
      setCreated(t.token);
      setName("");
      setOpen(false);
    },
  });
  const revoke = useMutation({ mutationFn: api.revokeApiToken, onSuccess: refreshAll });
  return (
    <Section
      title="API tokens"
      flush
      hint="Messages sent with a service token have the token's person: whoever created it."
      actions={<Button size="sm" onClick={() => setOpen(true)}>Create a token</Button>}
    >
      {created ? <div className="px-5 [&>*]:mb-3"><Note>Store it now; it is never shown again: <code className="break-all">{created}</code></Note></div> : null}
      {!tokens.data ? (
        <div className="px-5 pb-5"><Loading /></div>
      ) : (
        <DataTable
          caption="API tokens"
          stack
          rows={tokens.data}
          rowKey={(t) => t.id}
          empty="No API tokens."
          columns={[
            { key: "name", header: "Name", cell: (t) => <span className="font-medium">{t.name}</span> },
            { key: "kind", header: "Access", cell: (t) => <Badge>{t.kind}</Badge> },
            { key: "person", header: "Person", cell: (t) => personName(t.person_id) },
            { key: "prefix", header: "Starts with", wide: true, cell: (t) => <span className="font-mono text-caption text-muted">{t.shown_prefix}</span> },
            { key: "used", header: "Last used", wide: true, cell: (t) => <span className="text-muted">{t.last_used_at ? formatDateTime(t.last_used_at) : "never"}</span> },
            { key: "act", header: "", align: "right", cell: (t) => <Button size="sm" variant="danger" onClick={() => revoke.mutate(t.id)}>Revoke</Button> },
          ]}
        />
      )}
      <div className="px-5 [&>*]:mb-3"><FormError error={revoke.error} /></div>
      <Dialog open={open} onOpenChange={setOpen} title="Create a token" description="It is shown once, right after you create it.">
        <form
          className="flex flex-col gap-4"
          onSubmit={(e) => {
            e.preventDefault();
            create.mutate();
          }}
        >
          <Field label="Access">
            <Select value={kind} onChange={(e) => setKind(e.target.value as ApiTokenKind)}>
              {tokenKinds.filter((k) => !k.orgManager || hasRole(me.person, "org_manager")).map((k) => <option key={k.kind} value={k.kind}>{k.label}</option>)}
            </Select>
          </Field>
          <Field label="Token name"><Input value={name} onChange={(e) => setName(e.target.value)} /></Field>
          <FormError error={create.error} />
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={() => setOpen(false)}>Cancel</Button>
            <Button type="submit" variant="primary" disabled={create.isPending}>Create a token</Button>
          </div>
        </form>
      </Dialog>
    </Section>
  );
}

function RawExport() {
  const get = useMutation({
    mutationFn: api.rawExport,
    onSuccess: (blob) => downloadFile("tokencontroller-log.zip", blob, "application/zip"),
  });
  return (
    <Section title="Raw export" hint="A reader folds it back to the same figures.">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <p className="text-small text-muted">The whole log as a zip of JSON files.</p>
        <Button onClick={() => get.mutate()} disabled={get.isPending}>Download the log</Button>
      </div>
      <FormError error={get.error} />
    </Section>
  );
}
