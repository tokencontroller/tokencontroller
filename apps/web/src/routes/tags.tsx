import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { TagValue, TicketSystem } from "@/api/contract";
import { api } from "@/api/endpoints";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { Empty, ErrorBox, Loading, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Textarea } from "@/components/ui/form";
import { DataTable } from "@/components/ui/table";
import { TagRow } from "@/components/ui/tag";
import { notify } from "@/components/ui/toast";
import { filterFromText, formatDateTime } from "@/lib/format";
import { refreshAll, useMe, useTags } from "@/lib/queries";
import { hasRole } from "@/lib/roles";

// Docs: Tags. Tag values are records: key, title, description, source id
// and default tags. Features sync from Jira through the sync map.
export function TagsPage(): ReactNode {
  const me = useMe();
  const tags = useTags();
  const orgManager = hasRole(me.person, "org_manager");
  const [adding, setAdding] = useState<string | null>(null);
  const [newName, setNewName] = useState("");
  const addName = useMutation({
    mutationFn: () => api.addTagName(newName.trim()),
    onSuccess: async () => {
      await refreshAll();
      notify("Tag name added", newName);
      setNewName("");
    },
  });
  const removeName = useMutation({ mutationFn: api.removeTagName, onSuccess: refreshAll });
  return (
    <>
      <PageHeader title="Tags" lead="A tag value brings its default tags: a feature brings its project, a project brings its client." />
      {tags.error ? <ErrorBox error={tags.error} /> : null}
      {!tags.data ? <Loading /> : null}
      {tags.data?.map((name) => (
        <Section
          key={name.name}
          title={name.name}
          lead={name.built_in ? "Built in" : "Your own tag name"}
          actions={
            <>
              <Button size="sm" onClick={() => setAdding(name.name)} aria-label={`Add a ${name.name} value`}>Add a value</Button>
              {orgManager && !name.built_in ? <Button size="sm" variant="ghost" onClick={() => removeName.mutate(name.name)}>Remove the name</Button> : null}
            </>
          }
        >
          <DataTable
            caption={`Values of ${name.name}`}
            stack
            rows={name.values}
            rowKey={(v) => v.id}
            empty="No values yet."
            columns={[
              { key: "key", header: "Key", cell: (v) => <span className="font-mono text-[13px]">{v.key}</span>, csv: (v) => v.key },
              { key: "title", header: "Title", cell: (v) => <span className="block max-w-sm">{v.title ?? ""}{v.description ? <span className="block text-xs text-muted">{v.description}</span> : null}</span>, csv: (v) => v.title ?? "" },
              { key: "defaults", header: "Default tags", cell: (v) => <TagRow tags={v.defaults} /> },
              { key: "source", header: "Source id", wide: true, cell: (v) => (v.source_id ? <span className="flex items-center gap-1.5"><span className="font-mono text-xs">{v.source_id}</span>{v.source_status ? <Badge>{v.source_status}</Badge> : null}</span> : <span className="text-muted">by hand</span>) },
            ]}
          />
        </Section>
      ))}
      {orgManager ? (
        <Section title="Add a tag name" lead="Such as purpose or environment. Org managers keep the list.">
          <form className="flex max-w-md items-end gap-2" onSubmit={(e) => { e.preventDefault(); addName.mutate(); }}>
            <Field label="New tag name" className="flex-1"><Input value={newName} onChange={(e) => setNewName(e.target.value)} /></Field>
            <Button type="submit" disabled={!newName.trim()}>Add</Button>
          </form>
          <FormError error={addName.error ?? removeName.error} />
        </Section>
      ) : null}
      {orgManager ? <JiraSection /> : null}
      {adding ? <ValueDialog name={adding} onClose={() => setAdding(null)} /> : null}
    </>
  );
}

function ValueDialog({ name, onClose, value }: { name: string; onClose: () => void; value?: TagValue }) {
  const [key, setKey] = useState(value?.key ?? "");
  const [title, setTitle] = useState(value?.title ?? "");
  const [description, setDescription] = useState(value?.description ?? "");
  const [defaults, setDefaults] = useState(name === "feature" ? "project:" : name === "project" ? "client:" : "");
  const save = useMutation({
    mutationFn: () => {
      const d = filterFromText(defaults.replace(/\s+/g, ""));
      const clean = Object.fromEntries(Object.entries(d).filter(([, v]) => v));
      return api.addTagValue(name, { key: key.trim(), ...(title ? { title } : {}), ...(description ? { description } : {}), defaults: clean });
    },
    onSuccess: async () => {
      await refreshAll();
      notify("Tag value saved", `${name}: ${key}`);
      onClose();
    },
  });
  return (
    <Dialog open onOpenChange={(o) => { if (!o) onClose(); }} title={`Add a ${name} value`} description="The same key again supersedes the earlier record.">
      <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
        <Field label="Key" hint="The short name used in tags, such as KD-214 or shop."><Input value={key} onChange={(e) => setKey(e.target.value)} /></Field>
        <Field label="Title"><Input value={title} onChange={(e) => setTitle(e.target.value)} /></Field>
        <Field label="Description" hint="A feature's description is what the estimator reads."><Textarea value={description} onChange={(e) => setDescription(e.target.value)} /></Field>
        <Field label="Default tags" hint="Such as project:shop for a feature, client:acme for a project."><Input value={defaults} onChange={(e) => setDefaults(e.target.value)} /></Field>
        <FormError error={save.error} />
        <div className="flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={save.isPending || !key.trim()}>Save</Button>
        </div>
      </form>
    </Dialog>
  );
}

function JiraSection() {
  const systems = useQuery({ queryKey: ["ticket-systems"], queryFn: api.ticketSystems });
  const [site, setSite] = useState("");
  const [email, setEmail] = useState("");
  const [token, setToken] = useState("");
  const connect = useMutation({
    mutationFn: () => api.connectTicketSystem({ kind: "jira", site, email, token }),
    onSuccess: async () => {
      await refreshAll();
      notify("Jira connected");
    },
  });
  return (
    <Section title="Jira" lead="Optional. Every feature becomes a copy of its Jira issue, kept in step every 15 minutes. Whoever connects Jira answers for that data flow.">
      {systems.data?.length ? (
        systems.data.map((s) => <JiraConnection key={s.id} system={s} />)
      ) : (
        <form className="grid max-w-2xl gap-3 sm:grid-cols-3 sm:items-end" onSubmit={(e) => { e.preventDefault(); connect.mutate(); }}>
          <Field label="Jira site"><Input value={site} placeholder="https://acme.atlassian.net" onChange={(e) => setSite(e.target.value)} /></Field>
          <Field label="Atlassian account email"><Input type="email" value={email} onChange={(e) => setEmail(e.target.value)} /></Field>
          <Field label="API token"><Input type="password" value={token} onChange={(e) => setToken(e.target.value)} /></Field>
          <div className="sm:col-span-3"><Button type="submit" disabled={connect.isPending}>Connect Jira</Button></div>
          <div className="sm:col-span-3"><FormError error={connect.error} /></div>
        </form>
      )}
    </Section>
  );
}

function JiraConnection({ system }: { system: TicketSystem }) {
  const projects = useQuery({ queryKey: ["jira-projects", system.id], queryFn: () => api.externalProjects(system.id) });
  const tags = useTags();
  const ours = tags.data?.find((n) => n.name === "project")?.values ?? [];
  const [map, setMap] = useState<Record<string, string>>(Object.fromEntries(system.mappings.map((m) => [m.external, m.project])));
  const save = useMutation({
    mutationFn: () => api.mapProjects(system.id, Object.entries(map).filter(([, p]) => p).map(([external, project]) => ({ external, project }))),
    onSuccess: async () => {
      await refreshAll();
      notify("Sync map saved");
    },
  });
  const sync = useMutation({ mutationFn: () => api.syncTickets(system.id), onSuccess: async () => { await refreshAll(); notify("Sync started"); } });
  const disconnect = useMutation({ mutationFn: () => api.disconnectTicketSystem(system.id), onSuccess: refreshAll });
  return (
    <div className="rounded-lg border border-line p-4">
      <div className="mb-3 flex flex-wrap items-center gap-2 text-sm">
        <span className="font-medium">{system.site}</span>
        <Badge tone={system.status === "ok" ? "posted" : "exception"}>{system.status}</Badge>
        <span className="text-muted">{system.last_sync_at ? `last sync ${formatDateTime(system.last_sync_at)}` : "not synced yet"}</span>
        <span className="ml-auto flex gap-2">
          <Button size="sm" onClick={() => sync.mutate()}>Sync now</Button>
          <Button size="sm" variant="ghost" onClick={() => disconnect.mutate()}>Disconnect</Button>
        </span>
      </div>
      {system.message ? <p className="mb-2 text-sm text-exception">{system.message}</p> : null}
      <h3 className="mb-2 text-sm font-medium">Sync map</h3>
      {projects.data?.length ? (
        <div className="flex max-w-lg flex-col gap-2">
          {projects.data.map((p) => (
            <label key={p.key} className="grid grid-cols-2 items-center gap-2 text-sm">
              <span>{p.key} · {p.name}</span>
              <select aria-label={`Project for ${p.key}`} className="h-8 rounded-md border border-line-strong bg-page px-2" value={map[p.key] ?? ""} onChange={(e) => setMap({ ...map, [p.key]: e.target.value })}>
                <option value="">not synced</option>
                {ours.map((v) => <option key={v.key} value={v.key}>{v.title ?? v.key}</option>)}
              </select>
            </label>
          ))}
          <div><Button size="sm" onClick={() => save.mutate()}>Save the sync map</Button></div>
        </div>
      ) : (
        <Empty title="No Jira projects found" />
      )}
      <FormError error={save.error ?? sync.error ?? disconnect.error} />
    </div>
  );
}
