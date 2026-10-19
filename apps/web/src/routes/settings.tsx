import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { ApiTokenKind } from "@/api/contract";
import { api } from "@/api/endpoints";
import { EnterpriseSettings } from "@/components/enterprise-settings";
import { Button } from "@/components/ui/button";
import { Loading, Note, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select } from "@/components/ui/form";
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
      <PageHeader title="Settings" lead={me.organization.name} />
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
    <Section title="Organization">
      <form
        className="grid max-w-2xl gap-3 sm:grid-cols-3 sm:items-end"
        onSubmit={(e) => {
          e.preventDefault();
          save.mutate();
        }}
      >
        <Field label="Name"><Input value={name} onChange={(e) => setName(e.target.value)} /></Field>
        <Field label="Reporting currency"><Input value={currency} onChange={(e) => setCurrency(e.target.value)} /></Field>
        <Field label="Sign-in domains" hint="Empty: any."><Input value={domains} onChange={(e) => setDomains(e.target.value)} /></Field>
        <div className="sm:col-span-3"><Button type="submit" disabled={save.isPending}>Save</Button></div>
      </form>
      <FormError error={save.error} />
      <div className="mt-5 flex items-center gap-3 text-sm">
        <Switch checked={org.approval} onCheckedChange={(on) => approval.mutate(on)} label="Submissions need approval" />
        <span>Submissions need approval</span>
      </div>
      <p className="mt-1 text-xs text-muted">With approval on, a people manager approves or rejects each submission, and reports can show approved messages only.</p>
      <FormError error={approval.error} />
    </Section>
  );
}

function ApiTokens() {
  const me = useMe();
  const tokens = useQuery({ queryKey: ["api-tokens"], queryFn: api.apiTokens });
  const personName = usePersonName();
  const [kind, setKind] = useState<ApiTokenKind>("service");
  const [name, setName] = useState("");
  const [created, setCreated] = useState<string | null>(null);
  const create = useMutation({
    mutationFn: () => api.createApiToken(kind, name),
    onSuccess: async (t) => {
      await refreshAll();
      setCreated(t.token);
      setName("");
    },
  });
  const revoke = useMutation({ mutationFn: api.revokeApiToken, onSuccess: refreshAll });
  return (
    <Section title="API tokens" lead="Messages sent with a service token have the token's person: whoever created it.">
      {created ? <Note>Store it now; it is never shown again: <code className="break-all">{created}</code></Note> : null}
      {!tokens.data ? (
        <Loading />
      ) : (
        <DataTable
          caption="API tokens"
          stack
          rows={tokens.data}
          rowKey={(t) => t.id}
          empty="No API tokens."
          columns={[
            { key: "name", header: "Name", cell: (t) => t.name },
            { key: "kind", header: "Kind", cell: (t) => t.kind },
            { key: "person", header: "Person", cell: (t) => personName(t.person_id) },
            { key: "prefix", header: "Starts with", wide: true, cell: (t) => <span className="font-mono text-xs">{t.shown_prefix}</span> },
            { key: "used", header: "Last used", wide: true, cell: (t) => (t.last_used_at ? formatDateTime(t.last_used_at) : "never") },
            { key: "act", header: "", cell: (t) => <Button size="sm" variant="ghost" onClick={() => revoke.mutate(t.id)}>Revoke</Button> },
          ]}
        />
      )}
      <form
        className="mt-4 grid max-w-2xl gap-3 sm:grid-cols-3 sm:items-end"
        onSubmit={(e) => {
          e.preventDefault();
          create.mutate();
        }}
      >
        <Field label="Token kind">
          <Select value={kind} onChange={(e) => setKind(e.target.value as ApiTokenKind)}>
            {tokenKinds.filter((k) => !k.orgManager || hasRole(me.person, "org_manager")).map((k) => <option key={k.kind} value={k.kind}>{k.label}</option>)}
          </Select>
        </Field>
        <Field label="Token name"><Input value={name} onChange={(e) => setName(e.target.value)} /></Field>
        <Button type="submit" disabled={create.isPending}>Create a token</Button>
      </form>
      <FormError error={create.error ?? revoke.error} />
    </Section>
  );
}

function RawExport() {
  const get = useMutation({
    mutationFn: api.rawExport,
    onSuccess: (blob) => downloadFile("tokencontroller-log.zip", blob, "application/zip"),
  });
  return (
    <Section title="Raw export" lead="The whole log as a zip of JSON files, one per kind of record. A reader folds it back to the same figures.">
      <Button onClick={() => get.mutate()} disabled={get.isPending}>Download the log</Button>
      <FormError error={get.error} />
    </Section>
  );
}
