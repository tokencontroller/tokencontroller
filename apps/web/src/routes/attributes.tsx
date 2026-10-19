import { useMutation } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { Member } from "@/api/contract";
import { api } from "@/api/endpoints";
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
  const [setting, setSetting] = useState<Member | null>(null);
  const [adding, setAdding] = useState<string | null>(null);
  const [newName, setNewName] = useState("");
  const addName = useMutation({ mutationFn: () => api.addAttributeName(newName.trim()), onSuccess: async () => { await refreshAll(); setNewName(""); } });
  return (
    <>
      <PageHeader title="Attributes" lead="Tags say what work was for; attributes say who the person is." />
      <Section title="People">
        {people.error ? <ErrorBox error={people.error} /> : null}
        {!people.data ? (
          <Loading />
        ) : (
          <DataTable
            caption="People and their attributes"
            stack
            rows={people.data}
            rowKey={(p) => p.id}
            columns={[
              { key: "name", header: "Person", cell: (p) => <span>{displayName(p)}<span className="block text-xs text-muted">{p.email}</span></span> },
              { key: "attributes", header: "Attributes", cell: (p) => <TagRow tags={p.attributes} /> },
              { key: "set", header: "", cell: (p) => <Button size="sm" onClick={() => setSetting(p)} aria-label={`Set an attribute for ${displayName(p)}`}>Set an attribute</Button> },
            ]}
          />
        )}
      </Section>
      {attributes.data?.map((name) => (
        <Section key={name.name} title={name.name} lead={name.built_in ? "Built in" : "Your own attribute name"} actions={<Button size="sm" onClick={() => setAdding(name.name)} aria-label={`Add a ${name.name} value`}>Add a value</Button>}>
          <DataTable
            caption={`Values of ${name.name}`}
            rows={name.values}
            rowKey={(v) => v.id}
            empty="No values yet."
            columns={[
              { key: "key", header: "Key", cell: (v) => <span className="font-mono text-[13px]">{v.key}</span> },
              { key: "title", header: "Title", cell: (v) => v.title ?? "" },
              { key: "defaults", header: "Default attributes", cell: (v) => <TagRow tags={v.defaults} /> },
            ]}
          />
        </Section>
      ))}
      {hasRole(me.person, "org_manager") ? (
        <Section title="Add an attribute name" lead="Such as guild.">
          <form className="flex max-w-md items-end gap-2" onSubmit={(e) => { e.preventDefault(); addName.mutate(); }}>
            <Field label="New attribute name" className="flex-1"><Input value={newName} onChange={(e) => setNewName(e.target.value)} /></Field>
            <Button type="submit" disabled={!newName.trim()}>Add</Button>
          </form>
          <FormError error={addName.error} />
        </Section>
      ) : null}
      {setting ? <PersonAttributeDialog person={setting} onClose={() => setSetting(null)} /> : null}
      {adding ? <AttributeValueDialog name={adding} onClose={() => setAdding(null)} /> : null}
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
        <Field label="From" hint="Empty: for every day."><Input type="date" value={from} onChange={(e) => setFrom(e.target.value)} /></Field>
        <FormError error={save.error} />
        <div className="flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
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
        <Field label="Default attributes" hint="Such as department:engineering for a team."><Input value={defaults} onChange={(e) => setDefaults(e.target.value)} /></Field>
        <FormError error={save.error} />
        <div className="flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={!key.trim() || save.isPending}>Save</Button>
        </div>
      </form>
    </Dialog>
  );
}
