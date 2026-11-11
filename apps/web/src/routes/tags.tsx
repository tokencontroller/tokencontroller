import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { TagValue, TagSource } from "@/api/contract";
import { api } from "@/api/endpoints";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { Empty, ErrorBox, Loading, Note, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select, Textarea } from "@/components/ui/form";
import { Menu } from "@/components/ui/menu";
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
  const [naming, setNaming] = useState(false);
  const addName = useMutation({
    mutationFn: () => api.addTagName(newName.trim()),
    onSuccess: async () => {
      await refreshAll();
      notify("Tag name added", newName);
      setNewName("");
      setNaming(false);
    },
  });
  const removeName = useMutation({ mutationFn: api.removeTagName, onSuccess: refreshAll });
  return (
    <>
      <PageHeader title="Tags" actions={orgManager ? <Button variant="primary" onClick={() => setNaming(true)}>Add a tag name</Button> : null} />
      {tags.error ? <ErrorBox error={tags.error} /> : null}
      {!tags.data ? <Loading /> : null}
      {removeName.error ? <FormError error={removeName.error} /> : null}
      {tags.data?.map((name) => (
        <Section
          key={name.name}
          title={name.name}
          flush
          badge={name.built_in ? <Badge>built in</Badge> : null}
          hint={name.name === "feature" ? "A feature brings its project; a project brings its client." : undefined}
          actions={
            <>
              <Button size="sm" onClick={() => setAdding(name.name)} aria-label={`Add a ${name.name} value`}>Add a value</Button>
              {orgManager && !name.built_in ? <Menu label={`More for ${name.name}`} items={[{ label: "Remove the name", danger: true, onClick: () => removeName.mutate(name.name) }]} /> : null}
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
              { key: "key", header: "Key", className: "sm:w-40", cell: (v) => <span className="font-mono text-small">{v.key}</span>, csv: (v) => v.key },
              { key: "title", header: "Title", cell: (v) => <span className="inline-flex max-w-sm flex-col items-end text-right sm:items-start sm:text-left"><span>{v.title ?? ""}</span>{v.description ? <span className="text-caption text-muted">{v.description}</span> : null}</span>, csv: (v) => v.title ?? "" },
              { key: "defaults", header: "Default tags", className: "sm:w-56", cell: (v) => (Object.keys(v.defaults).length ? <TagRow tags={v.defaults} /> : null) },
              { key: "source", header: "Source", wide: true, className: "sm:w-48", cell: (v) => (v.source_id ? <span className="flex items-center gap-1.5"><span className="font-mono text-caption">{v.source_id}</span>{v.source_status ? <Badge>{v.source_status}</Badge> : null}</span> : null) },
            ]}
          />
        </Section>
      ))}
      {orgManager ? <JiraSection /> : null}
      {adding ? <ValueDialog name={adding} onClose={() => setAdding(null)} /> : null}
      <Dialog open={naming} onOpenChange={setNaming} title="Add a tag name" description="Such as purpose or environment.">
        <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); addName.mutate(); }}>
          <Field label="New tag name"><Input value={newName} onChange={(e) => setNewName(e.target.value)} /></Field>
          <FormError error={addName.error} />
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={() => setNaming(false)}>Cancel</Button>
            <Button type="submit" variant="primary" disabled={!newName.trim() || addName.isPending}>Add</Button>
          </div>
        </form>
      </Dialog>
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
        <Field label="Key" hint="Such as KD-214 or shop"><Input value={key} onChange={(e) => setKey(e.target.value)} /></Field>
        <Field label="Title"><Input value={title} onChange={(e) => setTitle(e.target.value)} /></Field>
        <Field label="Description" hint="The estimator reads it"><Textarea value={description} onChange={(e) => setDescription(e.target.value)} /></Field>
        <Field label="Default tags" hint="Such as project:shop"><Input value={defaults} onChange={(e) => setDefaults(e.target.value)} /></Field>
        <FormError error={save.error} />
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={save.isPending || !key.trim()}>Save</Button>
        </div>
      </form>
    </Dialog>
  );
}

function JiraSection() {
  const systems = useQuery({ queryKey: ["tag-sources"], queryFn: api.tagSources });
  const [open, setOpen] = useState(false);
  const [site, setSite] = useState("");
  const [email, setEmail] = useState("");
  const [token, setToken] = useState("");
  const connect = useMutation({
    mutationFn: () => api.connectTagSource({ kind: "jira", site, email, token }),
    onSuccess: async () => {
      await refreshAll();
      notify("Jira connected");
      setOpen(false);
    },
  });
  const connected = Boolean(systems.data?.length);
  return (
    <Section
      title="Jira"
      badge={systems.data && !connected ? <Badge>not connected</Badge> : null}
      hint="Every feature becomes a copy of its Jira issue, kept in step every 15 minutes. Whoever connects Jira answers for that data flow."
      actions={systems.data && !connected ? <Button size="sm" onClick={() => setOpen(true)}>Connect Jira</Button> : null}
    >
      {connected ? systems.data?.map((s) => <JiraConnection key={s.id} system={s} />) : <p className="text-small text-muted">Optional. Features can come from Jira.</p>}
      <Dialog open={open} onOpenChange={setOpen} title="Connect Jira">
        <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); connect.mutate(); }}>
          <Field label="Jira site"><Input value={site} placeholder="https://acme.atlassian.net" onChange={(e) => setSite(e.target.value)} /></Field>
          <Field label="Atlassian account email"><Input type="email" value={email} onChange={(e) => setEmail(e.target.value)} /></Field>
          <Field label="API token"><Input type="password" value={token} onChange={(e) => setToken(e.target.value)} /></Field>
          <FormError error={connect.error} />
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={() => setOpen(false)}>Cancel</Button>
            <Button type="submit" variant="primary" disabled={connect.isPending}>Connect Jira</Button>
          </div>
        </form>
      </Dialog>
    </Section>
  );
}

function JiraConnection({ system }: { system: TagSource }) {
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
  const disconnect = useMutation({ mutationFn: () => api.disconnectTagSource(system.id), onSuccess: refreshAll });
  return (
    <div>
      <div className="mb-4 flex flex-wrap items-center gap-x-3 gap-y-2">
        <span className="text-body font-medium">{system.site}</span>
        <Badge tone={system.status === "ok" ? "posted" : "exception"}>{system.status}</Badge>
        <span className="text-small text-muted">{system.last_sync_at ? `synced ${formatDateTime(system.last_sync_at)}` : "not synced yet"}</span>
        <span className="ml-auto flex gap-2">
          <Button size="sm" onClick={() => sync.mutate()}>Sync now</Button>
          <Button size="sm" variant="danger" onClick={() => disconnect.mutate()}>Disconnect</Button>
        </span>
      </div>
      {system.message ? <Note tone="exception">{system.message}</Note> : null}
      {projects.data?.length ? (
        <div className="flex max-w-lg flex-col gap-3">
          <div className="text-small font-medium text-ink-soft">Sync map</div>
          {projects.data.map((p) => (
            <label key={p.key} className="grid grid-cols-2 items-center gap-3 text-body">
              <span>{p.key} · {p.name}</span>
              <Select aria-label={`Project for ${p.key}`} value={map[p.key] ?? ""} onChange={(e) => setMap({ ...map, [p.key]: e.target.value })}>
                <option value="">not synced</option>
                {ours.map((v) => <option key={v.key} value={v.key}>{v.title ?? v.key}</option>)}
              </Select>
            </label>
          ))}
          <div className="flex justify-end"><Button onClick={() => save.mutate()}>Save the sync map</Button></div>
        </div>
      ) : (
        <Empty title="No Jira projects found" />
      )}
      <FormError error={save.error ?? sync.error ?? disconnect.error} />
    </div>
  );
}
