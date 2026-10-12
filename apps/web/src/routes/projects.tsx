import { useMutation, useQuery } from "@tanstack/react-query";
import { getRouteApi, Link } from "@tanstack/react-router";
import { useState, type ReactNode } from "react";
import type { Cadence, CadenceEvery, Project, Tags, Ticket } from "@/api/contract";
import { api } from "@/api/endpoints";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Loading, MoneyText, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select, Textarea } from "@/components/ui/form";
import { DataTable, type Column } from "@/components/ui/table";
import { TagRow } from "@/components/ui/tag";
import { notify } from "@/components/ui/toast";
import { refreshAll, useMe, usePeople, usePersonName } from "@/lib/queries";
import { hasRole } from "@/lib/roles";
import { toDecimal } from "@/lib/money";
const projectRoute = getRouteApi("/app/projects/$key");

const cadenceText: Record<CadenceEvery, string> = { weekly: "Weekly", biweekly: "Every two weeks", monthly: "Monthly", custom: "Custom" };

export function describeCadence(cadence: Cadence | null): string {
  if (!cadence) return "No cadence";
  return cadence.every === "custom" ? `Every ${cadence.days ?? "?"} days` : cadenceText[cadence.every];
}

export function CadenceSelect({ value, onChange }: { value: Cadence | null; onChange: (cadence: Cadence | null) => void }) {
  return (
    <div className="flex gap-2">
      <Select aria-label="Cadence" value={value?.every ?? ""} onChange={(e) => onChange(e.target.value ? { every: e.target.value as CadenceEvery, days: e.target.value === "custom" ? (value?.days ?? 10) : null } : null)}>
        <option value="">No cadence</option>
        {(Object.keys(cadenceText) as CadenceEvery[]).map((k) => (
          <option key={k} value={k}>
            {cadenceText[k]}
          </option>
        ))}
      </Select>
      {value?.every === "custom" ? (
        <Input aria-label="Days" type="number" min={1} className="w-24" value={value.days ?? 10} onChange={(e) => onChange({ every: "custom", days: Number(e.target.value) || 1 })} />
      ) : null}
    </div>
  );
}

function tagsToText(tags: Tags): string {
  return Object.entries(tags).map(([k, v]) => `${k}: ${v}`).join(", ");
}

function tagsFromText(text: string): Tags {
  const tags: Tags = {};
  for (const part of text.split(",")) {
    const [k, ...rest] = part.split(":");
    const value = rest.join(":").trim();
    if (k?.trim() && value) tags[k.trim()] = value;
  }
  return tags;
}

function ProjectForm({ project, onDone }: { project: Project | null; onDone: () => void }) {
  const me = useMe();
  const controller = hasRole(me.person, "controller");
  const [form, setForm] = useState({
    name: project?.name ?? "",
    key: project?.key ?? "",
    tags: tagsToText(project?.tags ?? {}),
    ticket_key_pattern: project?.ticket_key_pattern ?? "",
    repositories: project?.repositories.join(", ") ?? "",
  });
  const [cadence, setCadence] = useState<Cadence | null>(project?.cadence ?? null);
  const set = (k: keyof typeof form, v: string) => setForm((f) => ({ ...f, [k]: v }));
  const save = useMutation({
    mutationFn: () => {
      try {
        if (form.ticket_key_pattern) new RegExp(form.ticket_key_pattern);
      } catch {
        throw new Error("The ticket key pattern is not a valid regular expression.");
      }
      const body = { name: form.name, tags: tagsFromText(form.tags), ticket_key_pattern: form.ticket_key_pattern, repositories: form.repositories.split(/[\s,]+/).filter(Boolean), cadence };
      return project ? api.updateProject(project.key, controller ? body : { cadence }) : api.createProject({ ...body, key: form.key });
    },
    onSuccess: async () => {
      await refreshAll();
      notify(project ? "Project saved" : "Project created");
      onDone();
    },
  });
  return (
    <form className="space-y-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
      <div className="grid gap-3 sm:grid-cols-2">
        <Field label="Name">
          <Input required disabled={!controller} value={form.name} onChange={(e) => set("name", e.target.value)} placeholder="Storefront relaunch" />
        </Field>
        <Field label="Key" hint="The value of the project tag. It cannot change later.">
          <Input required disabled={Boolean(project)} pattern="[a-z0-9][a-z0-9-]*" value={form.key} onChange={(e) => set("key", e.target.value)} placeholder="storefront" />
        </Field>
      </div>
      <Field label="Tags its token entries inherit" hint="Such as client: acme. Separate several with commas.">
        <Input disabled={!controller} value={form.tags} onChange={(e) => set("tags", e.target.value)} placeholder="client: acme" />
      </Field>
      <Field label="Ticket key pattern" hint={<>A regular expression, such as <code className="font-mono">^KD-\d+</code>. For GitHub Issues use the repository and the number, such as <code className="font-mono">storefront#\d+</code>.</>}>
        <Input disabled={!controller} className="font-mono" value={form.ticket_key_pattern} onChange={(e) => set("ticket_key_pattern", e.target.value)} />
      </Field>
      <Field label="Repositories" hint="Sessions here with no ticket key land on the project as project overhead. One repository belongs to one project.">
        <Input disabled={!controller} className="font-mono" value={form.repositories} onChange={(e) => set("repositories", e.target.value)} placeholder="kestrel/storefront" />
      </Field>
      <Field label="Cadence for token sheets">
        <CadenceSelect value={cadence} onChange={setCadence} />
      </Field>
      <FormError error={save.error} />
      <div className="flex justify-end gap-2">
        <Button onClick={onDone}>Cancel</Button>
        <Button type="submit" variant="primary" disabled={save.isPending}>
          {project ? "Save project" : "Create project"}
        </Button>
      </div>
    </form>
  );
}

export function ProjectsPage(): ReactNode {
  const me = useMe();
  const projects = useQuery({ queryKey: ["projects"], queryFn: api.projects });
  const tickets = useQuery({ queryKey: ["tickets"], queryFn: () => api.tickets() });
  const [creating, setCreating] = useState(false);
  const columns: Column<Project>[] = [
    { key: "name", header: "Project", cell: (p) => <Link to="/projects/$key" params={{ key: p.key }} className="font-medium text-ink hover:text-accent">{p.name}</Link>, csv: (p) => p.name },
    { key: "key", header: "Key", cell: (p) => <span className="font-mono text-[13px]">{p.key}</span>, csv: (p) => p.key },
    { key: "tags", header: "Tags", wide: true, cell: (p) => <TagRow tags={p.tags} />, csv: (p) => tagsToText(p.tags) },
    { key: "pattern", header: "Ticket keys", wide: true, cell: (p) => <span className="font-mono text-[13px] text-ink-soft">{p.ticket_key_pattern}</span>, csv: (p) => p.ticket_key_pattern },
    { key: "repos", header: "Repositories", wide: true, cell: (p) => <span className="font-mono text-[13px] text-ink-soft">{p.repositories.join(", ")}</span>, csv: (p) => p.repositories.join(" ") },
    { key: "tickets", header: "Tickets", align: "right", cell: (p) => tickets.data?.filter((t) => t.project === p.key).length ?? "", csv: (p) => tickets.data?.filter((t) => t.project === p.key).length ?? 0 },
    { key: "cadence", header: "Cadence", wide: true, cell: (p) => describeCadence(p.cadence), csv: (p) => describeCadence(p.cadence) },
  ];
  return (
    <>
      <PageHeader
        title="Projects and tickets"
        lead="A project carries its ticket key pattern, its repositories and tags such as client: acme. A client is a tag on projects, not a record of its own."
        actions={hasRole(me.person, "controller") ? <Button variant="primary" onClick={() => setCreating(true)}>New project</Button> : undefined}
      />
      {projects.isPending ? <Loading /> : projects.isError ? <ErrorBox error={projects.error} /> : (
        <DataTable stack columns={columns} rows={projects.data} rowKey={(p) => p.key} csvName="projects.csv" caption="Projects" empty="No projects yet." />
      )}
      <Dialog open={creating} onOpenChange={setCreating} title="New project" wide>
        <ProjectForm project={null} onDone={() => setCreating(false)} />
      </Dialog>
    </>
  );
}

function NewTicket({ open, onClose, project }: { open: boolean; onClose: () => void; project: string }) {
  const people = usePeople();
  const tickets = useQuery({ queryKey: ["tickets", project], queryFn: () => api.tickets(project) });
  const [form, setForm] = useState({ key: "", title: "", description: "", parent: "", assignee: "" });
  const set = (k: keyof typeof form, v: string) => setForm((f) => ({ ...f, [k]: v }));
  const add = useMutation({
    mutationFn: () => api.createTicket({ key: form.key.trim(), title: form.title || null, description: form.description || null, parent: form.parent || null, assignee: form.assignee || null, project }),
    onSuccess: async () => {
      await refreshAll();
      notify("Ticket created");
      onClose();
    },
  });
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()} title="New ticket" description="For work no ticket system knows. It works the same as a synced one. Only the key is required.">
      <form className="space-y-4" onSubmit={(e) => { e.preventDefault(); add.mutate(); }}>
        <div className="grid gap-3 sm:grid-cols-2">
          <Field label="Key">
            <Input required className="font-mono" value={form.key} onChange={(e) => set("key", e.target.value)} placeholder="KD-300" />
          </Field>
          <Field label="Parent ticket">
            <Select value={form.parent} onChange={(e) => set("parent", e.target.value)}>
              <option value="">None</option>
              {(tickets.data ?? []).map((t) => (
                <option key={t.key} value={t.key}>
                  {t.key} {t.title ? `· ${t.title}` : ""}
                </option>
              ))}
            </Select>
          </Field>
        </div>
        <Field label="Title">
          <Input value={form.title} onChange={(e) => set("title", e.target.value)} />
        </Field>
        <Field label="Description" hint="Token forecasts compare the title and description with past work.">
          <Textarea value={form.description} onChange={(e) => set("description", e.target.value)} />
        </Field>
        <Field label="Assignee" hint="Being assignee grants nothing and costs nothing.">
          <Select value={form.assignee} onChange={(e) => set("assignee", e.target.value)}>
            <option value="">Nobody</option>
            {(people.data ?? []).map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </Select>
        </Field>
        <FormError error={add.error} />
        <div className="flex justify-end gap-2">
          <Button onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={add.isPending}>
            Create ticket
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

/** Tickets with their children under them; cost rolls up from the children. */
function ticketTree(tickets: Ticket[]): { ticket: Ticket; depth: number }[] {
  const keys = new Set(tickets.map((t) => t.key));
  const out: { ticket: Ticket; depth: number }[] = [];
  const walk = (parent: string | null, depth: number) => {
    for (const t of tickets.filter((x) => (parent === null ? !x.parent || !keys.has(x.parent) : x.parent === parent))) {
      out.push({ ticket: t, depth });
      walk(t.key, depth + 1);
    }
  };
  walk(null, 0);
  return out;
}

const sourceLabel = { jira: "Jira Cloud", azure_devops: "Azure DevOps", github_issues: "GitHub Issues", manual: "by hand" };

export function ProjectPage(): ReactNode {
  const { key } = projectRoute.useParams();
  const name = usePersonName();
  const projects = useQuery({ queryKey: ["projects"], queryFn: api.projects });
  const tickets = useQuery({ queryKey: ["tickets", key], queryFn: () => api.tickets(key) });
  const [editing, setEditing] = useState(false);
  const [adding, setAdding] = useState(false);
  const project = projects.data?.find((p) => p.key === key);
  if (projects.isPending) return <Loading />;
  if (!project) return <ErrorBox error={new Error("No such project.")} />;
  const rows = ticketTree(tickets.data ?? []);
  const columns: Column<{ ticket: Ticket; depth: number }>[] = [
    { key: "key", header: "Ticket", cell: (r) => <span className="font-mono text-[13px]" style={{ paddingLeft: r.depth * 16 }}>{r.depth ? "└ " : ""}{r.ticket.key}</span>, csv: (r) => r.ticket.key },
    { key: "title", header: "Title", cell: (r) => r.ticket.title ?? <span className="text-muted">no title</span>, csv: (r) => r.ticket.title ?? "" },
    { key: "parent", header: "Parent", csvOnly: true, cell: () => null, csv: (r) => r.ticket.parent ?? "" },
    { key: "assignee", header: "Assignee", wide: true, cell: (r) => (r.ticket.assignee ? name(r.ticket.assignee) : ""), csv: (r) => r.ticket.assignee ?? "" },
    { key: "source", header: "From", wide: true, cell: (r) => <Badge>{sourceLabel[r.ticket.source]}</Badge>, csv: (r) => r.ticket.source },
    { key: "cost", header: "Cost", align: "right", cell: (r) => <MoneyText value={r.ticket.cost} />, csv: (r) => toDecimal(r.ticket.cost) },
  ];
  return (
    <>
      <PageHeader
        title={project.name}
        lead={
          <span className="flex flex-wrap items-center gap-2">
            <span className="font-mono">{project.key}</span> <TagRow tags={project.tags} /> · {describeCadence(project.cadence)}
          </span>
        }
        actions={
          <>
            <Link to="/projects" className="text-sm text-accent hover:underline">All projects</Link>
            <Button onClick={() => setEditing(true)}>Edit</Button>
            <Button variant="primary" onClick={() => setAdding(true)}>New ticket</Button>
          </>
        }
      />
      <dl className="mb-8 grid gap-3 text-sm sm:grid-cols-2">
        <div>
          <dt className="text-[13px] text-muted">Ticket key pattern</dt>
          <dd className="font-mono">{project.ticket_key_pattern || "none"}</dd>
        </div>
        <div>
          <dt className="text-[13px] text-muted">Repositories</dt>
          <dd className="font-mono">{project.repositories.join(", ") || "none"}</dd>
        </div>
      </dl>
      <Section title="Tickets" lead="Cost rolls up: an epic's cost is the sum of its stories. Cost on the project but on no ticket is project overhead.">
        {tickets.isPending ? <Loading /> : <DataTable stack columns={columns} rows={rows} rowKey={(r) => r.ticket.key} csvName={`tickets-${project.key}.csv`} caption="Tickets" empty="No tickets yet. Connect a ticket system under Settings, or create one by hand." />}
      </Section>
      <Dialog open={editing} onOpenChange={setEditing} title={`Edit ${project.name}`} wide>
        <ProjectForm project={project} onDone={() => setEditing(false)} />
      </Dialog>
      <NewTicket open={adding} onClose={() => setAdding(false)} project={project.key} />
    </>
  );
}
