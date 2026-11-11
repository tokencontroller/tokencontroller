import { useMutation } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { Member } from "@/api/contract";
import { api } from "@/api/endpoints";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Loading, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select } from "@/components/ui/form";
import { DataTable } from "@/components/ui/table";
import { TagRow, TagValueList } from "@/components/ui/tag";
import { notify } from "@/components/ui/toast";
import { displayName, filterFromText } from "@/lib/format";
import { refreshAll, useAttributes, useMe, usePeople } from "@/lib/queries";
import { hasRole } from "@/lib/roles";

// Docs: Attributes. Attributes say who a person is (team, department,
// location); their defaults do the rollup, such as team web bringing
// department engineering. A message carries its person's attributes as of its time.
export function AttributesPage(): ReactNode {
  const me = useMe();
  const people = usePeople();
  const attributes = useAttributes();
  const orgManager = hasRole(me.person, "org_manager");
  const [setting, setSetting] = useState<Member | null>(null);
  const [adding, setAdding] = useState<string | null>(null);
  const [naming, setNaming] = useState(false);
  const [newName, setNewName] = useState("");
  const addName = useMutation({ mutationFn: () => api.addAttributeName(newName.trim()), onSuccess: async () => { await refreshAll(); setNewName(""); setNaming(false); } });
  return (
    <>
      <PageHeader title="Attributes" actions={orgManager ? <Button variant="primary" onClick={() => setNaming(true)}>Add an attribute name</Button> : null} />
      <Section title="People" flush hint="Tags say what work was for; attributes say who the person is. A message carries its person's attributes as of its time.">
        {people.error ? <div className="px-5 pb-5"><ErrorBox error={people.error} /></div> : null}
        {!people.data ? (
          <div className="px-5 pb-5"><Loading /></div>
        ) : (
          <DataTable
            caption="People and their attributes"
            stack
            rows={people.data}
            rowKey={(p) => p.id}
            columns={[
              { key: "name", header: "Person", cell: (p) => <span className="inline-flex flex-col items-end sm:items-start"><span className="font-medium">{displayName(p)}</span><span className="text-caption text-muted">{p.email}</span></span> },
              { key: "attributes", header: "Attributes", cell: (p) => <TagRow tags={p.attributes} /> },
              { key: "set", header: "", align: "right", cell: (p) => <Button size="sm" onClick={() => setSetting(p)} aria-label={`Set an attribute for ${displayName(p)}`}>Set</Button> },
            ]}
          />
        )}
      </Section>
      {attributes.data?.map((name) => (
        <Section
          key={name.name}
          title={name.name}
          flush
          badge={name.built_in ? <Badge>built in</Badge> : null}
          hint={name.name === "team" ? "A team brings its department." : undefined}
          actions={<Button size="sm" onClick={() => setAdding(name.name)} aria-label={`Add a ${name.name} value`}>Add a value</Button>}
        >
          <DataTable
            caption={`Values of ${name.name}`}
            stack
            rows={name.values}
            rowKey={(v) => v.id}
            empty="No values yet."
            columns={[
              { key: "key", header: "Key", className: "sm:w-40", cell: (v) => <span className="font-mono text-small">{v.key}</span> },
              { key: "title", header: "Title", cell: (v) => v.title ?? "" },
              { key: "defaults", header: "Default attributes", className: "sm:w-56", cell: (v) => (Object.keys(v.defaults).length ? <TagRow tags={v.defaults} /> : null) },
            ]}
          />
        </Section>
      ))}
      {setting ? <PersonAttributeDialog person={setting} onClose={() => setSetting(null)} /> : null}
      {adding ? <AttributeValueDialog name={adding} onClose={() => setAdding(null)} /> : null}
      <Dialog open={naming} onOpenChange={setNaming} title="Add an attribute name" description="Such as guild.">
        <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); addName.mutate(); }}>
          <Field label="New attribute name"><Input value={newName} onChange={(e) => setNewName(e.target.value)} /></Field>
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

function PersonAttributeDialog({ person, onClose }: { person: Member; onClose: () => void }) {
  const attributes = useAttributes();
  const [name, setName] = useState("team");
  const [key, setKey] = useState("");
  const [from, setFrom] = useState("");
  const save = useMutation({
    mutationFn: () => api.setPersonAttribute(person.id, { name, key: key.trim(), ...(from ? { from } : {}) }),
    onSuccess: async () => {
      await refreshAll();
      notify("Attribute set", `${displayName(person)}: ${name} ${key}`);
      onClose();
    },
  });
  return (
    <Dialog open onOpenChange={(o) => { if (!o) onClose(); }} title={`Set an attribute for ${displayName(person)}`} description="From a date: earlier days keep the earlier value.">
      <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
        <Field label="Attribute name">
          <Select value={name} onChange={(e) => setName(e.target.value)}>
            {(attributes.data ?? []).map((n) => <option key={n.name} value={n.name}>{n.name}</option>)}
          </Select>
        </Field>
        <Field label="Value">
          <Input list="attribute-keys" value={key} onChange={(e) => setKey(e.target.value)} />
          <TagValueList id="attribute-keys" name={name} />
        </Field>
        <Field label="From" hint="Empty: every day"><Input type="date" value={from} onChange={(e) => setFrom(e.target.value)} /></Field>
        <FormError error={save.error} />
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={!key.trim() || save.isPending}>Save</Button>
        </div>
      </form>
    </Dialog>
  );
}

function AttributeValueDialog({ name, onClose }: { name: string; onClose: () => void }) {
  const [key, setKey] = useState("");
  const [title, setTitle] = useState("");
  const [defaults, setDefaults] = useState(name === "team" ? "department:" : "");
  const save = useMutation({
    mutationFn: () => api.addAttributeValue(name, { key: key.trim(), ...(title ? { title } : {}), defaults: Object.fromEntries(Object.entries(filterFromText(defaults.replace(/\s+/g, ""))).filter(([, v]) => v)) }),
    onSuccess: async () => {
      await refreshAll();
      notify("Attribute value saved", `${name}: ${key}`);
      onClose();
    },
  });
  return (
    <Dialog open onOpenChange={(o) => { if (!o) onClose(); }} title={`Add a ${name} value`}>
      <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
        <Field label="Key"><Input value={key} onChange={(e) => setKey(e.target.value)} /></Field>
        <Field label="Title"><Input value={title} onChange={(e) => setTitle(e.target.value)} /></Field>
        <Field label="Default attributes" hint="Such as department:engineering"><Input value={defaults} onChange={(e) => setDefaults(e.target.value)} /></Field>
        <FormError error={save.error} />
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={!key.trim() || save.isPending}>Save</Button>
        </div>
      </form>
    </Dialog>
  );
}
