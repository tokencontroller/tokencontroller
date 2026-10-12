import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { TagMap as TagMapValue, TagName } from "@/api/contract";
import { api } from "@/api/endpoints";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Loading, Note, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select } from "@/components/ui/form";
import { tagColor } from "@/components/ui/tag";
import { notify } from "@/components/ui/toast";
import { refreshAll, usePersonName } from "@/lib/queries";

function Merge({ tag, onClose }: { tag: TagName | null; onClose: () => void }) {
  const [from, setFrom] = useState<string[]>([]);
  const [into, setInto] = useState("");
  const merge = useMutation({
    mutationFn: () => api.mergeTagValues(tag?.name ?? "", from.filter((v) => v !== into), into),
    onSuccess: async () => {
      await refreshAll();
      notify("Values merged", "In every open period. Closed periods change only by adjustment.");
      setFrom([]);
      onClose();
    },
  });
  return (
    <Dialog open={Boolean(tag)} onOpenChange={(o) => !o && onClose()} title={`Merge ${tag?.name ?? ""} values`} description="acme, Acme and acme-corp are three values. Merge them into one in the open periods.">
      <ul className="mb-4 max-h-64 space-y-1.5 overflow-y-auto">
        {tag?.values.map((v) => (
          <li key={v.value}>
            <label className="flex items-center gap-2 text-sm">
              <Checkbox label={v.value} checked={from.includes(v.value)} onCheckedChange={(on) => setFrom(on ? [...from, v.value] : from.filter((x) => x !== v.value))} />
              <span className="font-medium">{v.value}</span>
              <span className="text-muted">{v.entries} token entries</span>
            </label>
          </li>
        ))}
      </ul>
      <Field label="Into">
        <Input list="merge-into" value={into} onChange={(e) => setInto(e.target.value)} placeholder="acme" />
        <datalist id="merge-into">
          {from.map((v) => (
            <option key={v} value={v} />
          ))}
        </datalist>
      </Field>
      <FormError error={merge.error} />
      <div className="mt-5 flex justify-end gap-2">
        <Button onClick={onClose}>Cancel</Button>
        <Button variant="primary" disabled={from.length < 1 || !into.trim() || merge.isPending} onClick={() => merge.mutate()}>
          Merge
        </Button>
      </div>
    </Dialog>
  );
}

function TagMap({ names }: { names: TagName[] }) {
  const map = useQuery({ queryKey: ["tag-map"], queryFn: api.tagMap });
  if (map.isPending) return <Loading />;
  if (map.isError) return <ErrorBox error={map.error} />;
  return <TagMapEditor names={names} saved={map.data} />;
}

function TagMapEditor({ names, saved }: { names: TagName[]; saved: TagMapValue }) {
  const [draft, setDraft] = useState<TagMapValue>(saved);
  const save = useMutation({
    mutationFn: () => api.setTagMap(draft),
    onSuccess: async () => {
      await refreshAll();
      notify("Tag map saved", "Reports mean the new map from now on. Closed periods keep the map they closed with.");
    },
  });
  const move = (i: number, delta: number) => {
    const ladder = [...draft.ladder];
    const [rung] = ladder.splice(i, 1);
    if (rung) ladder.splice(i + delta, 0, rung);
    setDraft({ ...draft, ladder });
  };
  const unused = names.filter((n) => !draft.ladder.includes(n.name) && n.name !== "person");
  const changed = JSON.stringify(draft) !== JSON.stringify(saved);
  return (
    <div className="grid gap-6 lg:grid-cols-2">
      <div className="rounded-lg border border-line p-4">
        <h3 className="text-sm font-semibold">Cost center</h3>
        <p className="mb-3 mt-0.5 text-[13px] text-muted">Who answers for a cost. Each token entry still has one agent manager who submits it.</p>
        <Select aria-label="Cost center" value={draft.cost_center} onChange={(e) => setDraft({ ...draft, cost_center: e.target.value })}>
          {names.filter((n) => !["ticket", "project"].includes(n.name)).map((n) => (
            <option key={n.name}>{n.name}</option>
          ))}
        </Select>
      </div>
      <div className="rounded-lg border border-line p-4">
        <h3 className="text-sm font-semibold">Ladder for the cost object</h3>
        <p className="mb-3 mt-0.5 text-[13px] text-muted">Read from the top: the cost object is the first rung with a value.</p>
        <ol className="space-y-1.5">
          {draft.ladder.map((rung, i) => (
            <li key={rung} className="flex items-center gap-2 rounded-md border border-line bg-page px-2.5 py-1.5 text-sm shadow-chip">
              <span className="num w-5 text-muted">{i + 1}</span>
              <span aria-hidden="true" className="size-2 rounded-full" style={{ background: tagColor(rung) }} />
              <span className="flex-1 font-medium">{rung}</span>
              <button type="button" aria-label={`Move ${rung} up`} disabled={i === 0} onClick={() => move(i, -1)} className="rounded px-1.5 text-muted hover:bg-sand disabled:opacity-30">↑</button>
              <button type="button" aria-label={`Move ${rung} down`} disabled={i === draft.ladder.length - 1} onClick={() => move(i, 1)} className="rounded px-1.5 text-muted hover:bg-sand disabled:opacity-30">↓</button>
              <button type="button" aria-label={`Remove ${rung}`} onClick={() => setDraft({ ...draft, ladder: draft.ladder.filter((r) => r !== rung) })} className="rounded px-1.5 text-muted hover:bg-sand">×</button>
            </li>
          ))}
          <li className="flex items-center gap-2 rounded-md border border-dashed border-line-strong px-2.5 py-1.5 text-sm text-muted">
            <span className="num w-5">{draft.ladder.length + 1}</span> organization <span className="text-xs">(always last)</span>
          </li>
        </ol>
        {unused.length ? (
          <Select aria-label="Add a rung" className="mt-3" value="" onChange={(e) => e.target.value && setDraft({ ...draft, ladder: [...draft.ladder, e.target.value] })}>
            <option value="">Add a rung</option>
            {unused.map((n) => (
              <option key={n.name}>{n.name}</option>
            ))}
          </Select>
        ) : null}
      </div>
      <div className="lg:col-span-2">
        {changed ? <Note tone="exception">Changing the tag map changes what reports mean from now on. Compare periods across a change with care.</Note> : null}
        <FormError error={save.error} />
        <Button variant="primary" disabled={!changed || save.isPending} onClick={() => save.mutate()}>
          Save tag map
        </Button>
      </div>
    </div>
  );
}

export function TagsPage(): ReactNode {
  const personName = usePersonName();
  const names = useQuery({ queryKey: ["tag-names"], queryFn: api.tagNames });
  const [newName, setNewName] = useState("");
  const [merging, setMerging] = useState<TagName | null>(null);
  const add = useMutation({
    mutationFn: () => api.addTagName(newName),
    onSuccess: async () => {
      await refreshAll();
      notify(`Tag name ${newName} added`);
      setNewName("");
    },
  });
  const remove = useMutation({ mutationFn: api.removeTagName, onSuccess: refreshAll });
  if (names.isPending) return <Loading />;
  if (names.isError) return <ErrorBox error={names.error} />;
  return (
    <>
      <PageHeader title="Tags and the tag map" lead="You keep the list of tag names. Values are free: a new client or feature needs no setting. One value per tag name, so cost by one tag name always adds up." />
      <Section title="Tag names">
        <ul className="mb-4 divide-y divide-line rounded-lg border border-line">
          {names.data.map((n) => (
            <li key={n.name} className="flex flex-col gap-2 px-4 py-3 sm:flex-row sm:items-center">
              <div className="flex w-40 items-center gap-2">
                <span aria-hidden="true" className="size-2.5 rounded-full" style={{ background: tagColor(n.name) }} />
                <span className="font-medium">{n.name}</span>
                {n.built_in ? <Badge>built in</Badge> : null}
              </div>
              <div className="min-w-0 flex-1 truncate text-[13px] text-muted">
                {n.values.length ? `${n.values.slice(0, 6).map((v) => (n.name === "person" ? personName(v.value) : v.value)).join(", ")}${n.values.length > 6 ? ` and ${n.values.length - 6} more` : ""}` : "no values yet"}
              </div>
              <div className="flex gap-1">
                {n.values.length > 1 && n.name !== "person" ? <Button size="sm" variant="ghost" onClick={() => setMerging(n)}>Merge values</Button> : null}
                {!n.built_in ? <Button size="sm" variant="ghost" onClick={() => remove.mutate(n.name)}>Remove</Button> : null}
              </div>
            </li>
          ))}
        </ul>
        <form className="flex max-w-md gap-2" onSubmit={(e) => { e.preventDefault(); add.mutate(); }}>
          <Input aria-label="New tag name" value={newName} onChange={(e) => setNewName(e.target.value)} placeholder="feature" />
          <Button type="submit" disabled={!newName.trim() || add.isPending}>
            Add tag name
          </Button>
        </form>
        <div className="mt-2 max-w-md">
          <FormError error={add.error} />
        </div>
      </Section>
      <Section title="Tag map" lead="Which tags fill the cost center and the cost object. Each closed period keeps the map it was closed with.">
        <TagMap names={names.data} />
      </Section>
      <Merge tag={merging} onClose={() => setMerging(null)} />
    </>
  );
}
