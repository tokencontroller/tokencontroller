import { useMutation } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { Filter, Person, Role, Tags } from "@/api/contract";
import { api } from "@/api/endpoints";
import { FilterBuilder } from "@/routes/budgets";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Loading, Note, PageHeader } from "@/components/ui/display";
import { Field, FormError, Input, Select } from "@/components/ui/form";
import { Menu } from "@/components/ui/menu";
import { DataTable, type Column } from "@/components/ui/table";
import { Tag, TagRow } from "@/components/ui/tag";
import { notify } from "@/components/ui/toast";
import { describeFilter } from "@/lib/format";
import { refreshAll, useMe, usePeople } from "@/lib/queries";
import { hasRole, roleLabel } from "@/lib/roles";

const roles: Role[] = ["agent_manager", "people_manager", "controller"];

function tagsFromText(text: string): Tags {
  const tags: Tags = {};
  for (const part of text.split(",")) {
    const [k, ...rest] = part.split(":");
    const v = rest.join(":").trim();
    if (k?.trim() && v) tags[k.trim()] = v;
  }
  return tags;
}

function Invite({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [form, setForm] = useState({ name: "", email: "", role: "agent_manager" as Role, tags: "" });
  const invite = useMutation({
    mutationFn: () => api.invite({ name: form.name, email: form.email, role: form.role, tags: tagsFromText(form.tags) }),
    onSuccess: async () => {
      await refreshAll();
      notify("Invite sent", `${form.email} gets a sign-in link.`);
      setForm({ name: "", email: "", role: "agent_manager", tags: "" });
      onClose();
    },
  });
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()} title="Invite a person" description="They get a sign-in link by email. Their tags pass to their new token entries.">
      <form className="space-y-4" onSubmit={(e) => { e.preventDefault(); invite.mutate(); }}>
        <div className="grid gap-3 sm:grid-cols-2">
          <Field label="Name">
            <Input required value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} />
          </Field>
          <Field label="Email">
            <Input type="email" required value={form.email} onChange={(e) => setForm({ ...form, email: e.target.value })} />
          </Field>
        </div>
        <Field label="Role">
          <Select value={form.role} onChange={(e) => setForm({ ...form, role: e.target.value as Role })}>
            {roles.map((r) => (
              <option key={r} value={r}>
                {roleLabel[r]}
              </option>
            ))}
          </Select>
        </Field>
        <Field label="Tags" hint="Such as team: web. Separate several with commas.">
          <Input value={form.tags} onChange={(e) => setForm({ ...form, tags: e.target.value })} placeholder="team: web" />
        </Field>
        <FormError error={invite.error} />
        <div className="flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={invite.isPending}>
            Send invite
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

function Edit({ person, onClose }: { person: Person; onClose: () => void }) {
  const [role, setRole] = useState<Role>(person.role);
  const [tags, setTags] = useState(Object.entries(person.tags).map(([k, v]) => `${k}: ${v}`).join(", "));
  const [filters, setFilters] = useState<Filter[]>(person.filters);
  const [draft, setDraft] = useState<Filter>({});
  const save = useMutation({
    mutationFn: () => api.updateMember(person.id, { role, tags: tagsFromText(tags), filters: role === "people_manager" ? filters : [] }),
    onSuccess: async () => {
      await refreshAll();
      notify("Saved", "Only future token entries follow; last month's reports do not change.");
      onClose();
    },
  });
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()} title={person.name} description={person.email} wide>
      <form className="space-y-5" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
        <div className="grid gap-3 sm:grid-cols-2">
          <Field label="Role">
            <Select value={role} onChange={(e) => setRole(e.target.value as Role)}>
              {roles.map((r) => (
                <option key={r} value={r}>
                  {roleLabel[r]}
                </option>
              ))}
            </Select>
          </Field>
          <Field label="Tags" hint="One value per tag name. Someone in two teams picks a main one.">
            <Input value={tags} onChange={(e) => setTags(e.target.value)} />
          </Field>
        </div>
        {role === "people_manager" ? (
          <div>
            <h3 className="text-sm font-semibold">Assigned filters</h3>
            <p className="mb-3 text-[13px] text-muted">The filters this people manager reads, approves and budgets for, such as team: web or client: acme.</p>
            <ul className="mb-3 flex flex-wrap gap-2">
              {filters.map((f) => (
                <li key={describeFilter(f)} className="flex items-center gap-1 rounded-md border border-line px-2 py-1">
                  {Object.entries(f).map(([k, v]) => (
                    <Tag key={k} name={k} value={v} />
                  ))}
                  <button type="button" aria-label={`Remove ${describeFilter(f)}`} className="ml-1 text-muted hover:text-ink" onClick={() => setFilters(filters.filter((x) => x !== f))}>
                    ×
                  </button>
                </li>
              ))}
              {!filters.length ? <li className="text-sm text-muted">None yet.</li> : null}
            </ul>
            <div className="rounded-lg border border-line p-3">
              <FilterBuilder value={draft} onChange={setDraft} />
              <Button className="mt-3" disabled={!Object.keys(draft).length} onClick={() => { setFilters([...filters, draft]); setDraft({}); }}>
                Assign this filter
              </Button>
            </div>
          </div>
        ) : null}
        <FormError error={save.error} />
        <div className="flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={save.isPending}>
            Save
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

export function PeoplePage(): ReactNode {
  const me = useMe();
  const people = usePeople();
  const [inviting, setInviting] = useState(false);
  const [editing, setEditing] = useState<Person | null>(null);
  const remove = useMutation({ mutationFn: api.removeMember, onSuccess: async () => { await refreshAll(); notify("Removed", "Their past token entries keep their agent manager."); } });
  const controller = hasRole(me.person, "controller");
  const org = me.organization;
  const columns: Column<Person>[] = [
    { key: "name", header: "Name", cell: (p) => <span className="font-medium">{p.name}</span>, csv: (p) => p.name },
    { key: "email", header: "Email", wide: true, cell: (p) => <span className="text-ink-soft">{p.email}</span>, csv: (p) => p.email },
    { key: "role", header: "Role", cell: (p) => roleLabel[p.role], csv: (p) => roleLabel[p.role] },
    { key: "tags", header: "Tags", wide: true, cell: (p) => <TagRow tags={p.tags} />, csv: (p) => Object.entries(p.tags).map(([k, v]) => `${k}: ${v}`).join(" · ") },
    { key: "filters", header: "Assigned filters", wide: true, cell: (p) => (p.filters.length ? <span className="text-[13px]">{p.filters.map(describeFilter).join("; ")}</span> : ""), csv: (p) => p.filters.map(describeFilter).join("; ") },
    { key: "status", header: "", cell: (p) => (p.status === "invited" ? <Badge tone="accent">invited</Badge> : null), csv: (p) => p.status },
    ...(controller
      ? [{ key: "menu", header: "", cell: (p: Person) => (p.id === me.person.id ? null : <span onClick={(e) => e.stopPropagation()}><Menu label={`Actions for ${p.name}`} items={[{ label: "Edit", onClick: () => setEditing(p) }, { label: "Remove from organization", danger: true, onClick: () => remove.mutate(p.id) }]} /></span>) }]
      : []),
  ];
  return (
    <>
      <PageHeader
        title="People"
        lead="Members, their role and their tags. Teams and departments are tags on people; there is no org chart."
        actions={controller ? <Button variant="primary" onClick={() => setInviting(true)} disabled={org.plan === "solo"}>Invite a person</Button> : undefined}
      />
      {org.plan === "solo" ? <Note>Solo is for one person who holds every role. Choose the Team plan under Settings to invite people.</Note> : null}
      {org.member_limit ? <p className="mb-4 text-[13px] text-muted">{org.members} of {org.member_limit} members on the {org.plan === "team" ? "Team plan" : org.plan === "solo" ? "Solo plan" : "plan"}.</p> : null}
      {people.isPending ? <Loading /> : people.isError ? <ErrorBox error={people.error} /> : (
        <DataTable stack columns={columns} rows={people.data} rowKey={(p) => p.id} csvName="people.csv" caption="People" onRowClick={controller ? setEditing : undefined} />
      )}
      <Invite open={inviting} onClose={() => setInviting(false)} />
      {editing ? <Edit person={editing} onClose={() => setEditing(null)} /> : null}
    </>
  );
}
