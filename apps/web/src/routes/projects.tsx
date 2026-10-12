import { useMutation, useQuery } from "@tanstack/react-query";
import { getRouteApi, Link } from "@tanstack/react-router";
import { useState, type ReactNode } from "react";
import type { CadenceView, Money, Project, Tags, Ticket } from "@/api/contract";
import { api } from "@/api/endpoints";
import { CadenceSelect, describeCadence, type CadenceChoice } from "@/components/cadence-select";
import { PeriodSelect } from "@/components/period-select";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Loading, MoneyText, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select, Textarea } from "@/components/ui/form";
import { DataTable, type Column } from "@/components/ui/table";
import { TagRow } from "@/components/ui/tag";
import { notify } from "@/components/ui/toast";
import { fromDecimal, money, nanosOf, toDecimal } from "@/lib/money";
import { refreshAll, useMe, usePeople, usePersonName, useProjects, useTickets } from "@/lib/queries";
import { thisPeriod } from "@/lib/today";

const projectRoute = getRouteApi("/app/projects/$name");

export function tagsToText(tags: Tags): string {
  return Object.entries(tags).map(([k, v]) => `${k}: ${v}`).join(", ");
}

export function tagsFromText(text: string): Tags {
  const tags: Tags = {};
  for (const part of text.split(",")) {
    const [k, ...rest] = part.split(":");
    const value = rest.join(":").trim();
    if (k?.trim() && value) tags[k.trim()] = value;
  }
  return tags;
}

export function useCadences() {
  return useQuery({ queryKey: ["cadences"], queryFn: api.cadences });
}

/** Sets, changes or removes a cadence: the organization's, or one project's. */
export async function saveCadence(cadences: CadenceView[], project: string | null, choice: CadenceChoice | null): Promise<void> {
  const current = cadences.find((c) => (c.project ?? null) === project);
  if (!choice) {
    if (current) await api.removeCadence(current.id);
    return;
  }
  if (current?.kind === choice.kind && (current.every_days ?? null) === (choice.every_days ?? null)) return;
  await api.setCadence({ kind: choice.kind, every_days: choice.kind === "custom" ? (choice.every_days ?? null) : null, project });
}

function ProjectForm({ project, onDone }: { project: Project | null; onDone: () => void }) {
  const cadences = useCadences();
  const [form, setForm] = useState({
    name: project?.name ?? "",
    tags: tagsToText(project?.tags ?? {}),
    ticket_key_pattern: project?.ticket_key_pattern ?? "",
    repositories: project?.repositories.join(", ") ?? "",
  });
  const current = project ? cadences.data?.find((c) => c.project === project.name) : undefined;
  const [cadence, setCadence] = useState<CadenceChoice | null | undefined>(undefined);
  const shownCadence = cadence === undefined ? (current ?? null) : cadence;
  const set = (k: keyof typeof form, v: string) => setForm((f) => ({ ...f, [k]: v }));
  const save = useMutation({
    mutationFn: async () => {
      try {
        if (form.ticket_key_pattern) new RegExp(form.ticket_key_pattern);
      } catch {
        throw new Error("The ticket key pattern is not a valid regular expression.");
      }
      const body = {
        tags: tagsFromText(form.tags),
        ticket_key_pattern: form.ticket_key_pattern || null,
        repositories: form.repositories.split(/[\s,]+/).filter(Boolean),
      };
      const saved = project ? await api.updateProject(project.id, body) : await api.createProject({ ...body, name: form.name.trim() });
      if (cadence !== undefined) await saveCadence(cadences.data ?? [], saved.name, cadence);
    },
    onSuccess: async () => {
      await refreshAll();
      notify(project ? "Project saved" : "Project created");
      onDone();
    },
  });
  return (
    <form className="space-y-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
      <Field label="Name" hint={project ? "The value of the project tag, so it stays as it is." : "The value of the project tag, such as storefront. It cannot change later."}>
        <Input required disabled={Boolean(project)} pattern="[a-z0-9][a-z0-9-]*" value={form.name} onChange={(e) => set("name", e.target.value)} placeholder="storefront" />
      </Field>
      <Field label="Tags its token entries inherit" hint="Such as client: acme. Separate several with commas.">
        <Input value={form.tags} onChange={(e) => set("tags", e.target.value)} placeholder="client: acme" />
      </Field>
      <Field label="Ticket key pattern" hint={<>A regular expression, such as <code className="font-mono">^KD-\d+</code>. For GitHub Issues use the repository and the number, such as <code className="font-mono">storefront#\d+</code>.</>}>
        <Input className="font-mono" value={form.ticket_key_pattern} onChange={(e) => set("ticket_key_pattern", e.target.value)} />
      </Field>
      <Field label="Repositories" hint="Sessions here with no ticket key land on the project as project overhead. One repository belongs to one project.">
        <Input className="font-mono" value={form.repositories} onChange={(e) => set("repositories", e.target.value)} placeholder="kestrel/storefront" />
      </Field>
      <Field label="Cadence for token sheets" hint="Wins over the organization's cadence for this project.">
        <CadenceSelect value={shownCadence} onChange={setCadence} />
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
  const projects = useProjects();
  const tickets = useTickets();
  const cadences = useCadences();
  const [creating, setCreating] = useState(false);
  const count = (p: Project) => tickets.data?.filter((t) => t.project_id === p.id).length ?? 0;
  const cadenceOf = (p: Project) => describeCadence(cadences.data?.find((c) => c.project === p.name));
  const columns: Column<Project>[] = [
    { key: "name", header: "Project", cell: (p) => <Link to="/projects/$name" params={{ name: p.name }} className="font-medium text-ink hover:text-accent">{p.name}</Link>, csv: (p) => p.name },
    { key: "tags", header: "Tags", wide: true, cell: (p) => <TagRow tags={p.tags} />, csv: (p) => tagsToText(p.tags) },
    { key: "pattern", header: "Ticket keys", wide: true, cell: (p) => <span className="font-mono text-[13px] text-ink-soft">{p.ticket_key_pattern ?? ""}</span>, csv: (p) => p.ticket_key_pattern ?? "" },
    { key: "repos", header: "Repositories", wide: true, cell: (p) => <span className="font-mono text-[13px] text-ink-soft">{p.repositories.join(", ")}</span>, csv: (p) => p.repositories.join(" ") },
    { key: "tickets", header: "Tickets", align: "right", cell: (p) => (tickets.data ? count(p) : ""), csv: count },
    { key: "cadence", header: "Cadence", wide: true, cell: cadenceOf, csv: cadenceOf },
  ];
  return (
    <>
      <PageHeader
        title="Projects and tickets"
        lead="A project carries its ticket key pattern, its repositories and tags such as client: acme. A client is a tag on projects, not a record of its own."
        actions={<Button variant="primary" onClick={() => setCreating(true)}>New project</Button>}
      />
      {projects.isPending ? <Loading /> : projects.isError ? <ErrorBox error={projects.error} /> : (
        <DataTable stack columns={columns} rows={projects.data} rowKey={(p) => p.id} csvName="projects.csv" caption="Projects" empty="No projects yet." />
      )}
      <Dialog open={creating} onOpenChange={setCreating} title="New project" wide>
        <ProjectForm project={null} onDone={() => setCreating(false)} />
      </Dialog>
    </>
  );
}

function NewTicket({ open, onClose, project, tickets }: { open: boolean; onClose: () => void; project: Project; tickets: Ticket[] }) {
  const people = usePeople();
  const [form, setForm] = useState({ key: "", title: "", description: "", parent_id: "", assignee_id: "" });
  const set = (k: keyof typeof form, v: string) => setForm((f) => ({ ...f, [k]: v }));
  const add = useMutation({
    mutationFn: () =>
      api.createTicket({
        key: form.key.trim(),
        title: form.title || null,
        description: form.description || null,
        parent_id: form.parent_id || null,
        assignee_id: form.assignee_id || null,
        project_id: project.id,
      }),
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
            <Select value={form.parent_id} onChange={(e) => set("parent_id", e.target.value)}>
              <option value="">None</option>
              {tickets.map((t) => (
                <option key={t.id} value={t.id}>
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
          <Select value={form.assignee_id} onChange={(e) => set("assignee_id", e.target.value)}>
            <option value="">Nobody</option>
            {(people.data ?? []).filter((p) => p.status !== "removed").map((p) => (
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

interface TicketRow {
  ticket: Ticket;
  depth: number;
  /** The period's cost on the ticket and everything under it. */
  cost: Money | null;
}

/** Tickets with their children under them; cost rolls up from the children. */
function ticketTree(tickets: Ticket[], own: Map<string, bigint>, currency: string | null): TicketRow[] {
  const ids = new Set(tickets.map((t) => t.id));
  const childrenOf = (id: string | null) => tickets.filter((t) => (id === null ? !t.parent_id || !ids.has(t.parent_id) : t.parent_id === id));
  const rolled = (t: Ticket): bigint => childrenOf(t.id).reduce((sum, child) => sum + rolled(child), own.get(t.key) ?? 0n);
  const out: TicketRow[] = [];
  const walk = (parent: string | null, depth: number) => {
    for (const t of childrenOf(parent)) {
      out.push({ ticket: t, depth, cost: currency === null ? null : money(rolled(t), currency) });
      walk(t.id, depth + 1);
    }
  };
  walk(null, 0);
  return out;
}

const sourceLabel = { jira: "Jira Cloud", azure_devops: "Azure DevOps", github_issues: "GitHub Issues", manual: "by hand" };

export function ProjectPage(): ReactNode {
  const { name } = projectRoute.useParams();
  const search = projectRoute.useSearch();
  const navigate = projectRoute.useNavigate();
  const period = search.period ?? thisPeriod();
  const me = useMe();
  const personName = usePersonName();
  const projects = useProjects();
  const cadences = useCadences();
  const project = projects.data?.find((p) => p.name === name);
  const tickets = useQuery({ queryKey: ["tickets", project?.id], queryFn: () => api.tickets({ project_id: project?.id ?? "" }), enabled: Boolean(project) });
  const cost = useQuery({ queryKey: ["report", period, `project:${name}`, "ticket"], queryFn: () => api.cost({ period, filter: { project: name }, by: "ticket" }) });
  const [editing, setEditing] = useState(false);
  const [adding, setAdding] = useState(false);
  if (projects.isPending) return <Loading />;
  if (!project) return <ErrorBox error={new Error("No such project.")} />;
  // Each ticket's own cost; an adjustment to a closed period adds to its ticket.
  const own = new Map<string, bigint>();
  for (const r of cost.data?.rows ?? []) {
    if (r.value) own.set(r.value, (own.get(r.value) ?? 0n) + nanosOf(fromDecimal(r.amount, cost.data?.currency ?? me.organization.reporting_currency)));
  }
  const rows = ticketTree(tickets.data ?? [], own, cost.data?.currency ?? (cost.isPending ? null : me.organization.reporting_currency));
  const keyOf = (id: string | null | undefined) => tickets.data?.find((t) => t.id === id)?.key ?? "";
  const columns: Column<TicketRow>[] = [
    { key: "key", header: "Ticket", cell: (r) => <span className="font-mono text-[13px]" style={{ paddingLeft: r.depth * 16 }}>{r.depth ? "└ " : ""}{r.ticket.key}</span>, csv: (r) => r.ticket.key },
    { key: "title", header: "Title", cell: (r) => r.ticket.title ?? <span className="text-muted">no title</span>, csv: (r) => r.ticket.title ?? "" },
    { key: "parent", header: "Parent", csvOnly: true, cell: () => null, csv: (r) => keyOf(r.ticket.parent_id) },
    { key: "assignee", header: "Assignee", wide: true, cell: (r) => (r.ticket.assignee_id ? personName(r.ticket.assignee_id) : ""), csv: (r) => (r.ticket.assignee_id ? personName(r.ticket.assignee_id) : "") },
    { key: "source", header: "From", wide: true, cell: (r) => <Badge>{sourceLabel[r.ticket.source]}</Badge>, csv: (r) => r.ticket.source },
    { key: "cost", header: "Cost", align: "right", cell: (r) => <MoneyText value={r.cost} />, csv: (r) => (r.cost ? toDecimal(r.cost) : "") },
  ];
  return (
    <>
      <PageHeader
        title={project.name}
        lead={
          <span className="flex flex-wrap items-center gap-2">
            <TagRow tags={project.tags} /> · {describeCadence(cadences.data?.find((c) => c.project === project.name))}
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
      <Section
        title="Tickets"
        lead="Cost rolls up: an epic's cost is the sum of its stories. Cost on the project but on no ticket is project overhead."
        actions={<PeriodSelect value={period} onChange={(p) => void navigate({ search: { period: p } })} />}
      >
        {tickets.isPending ? <Loading /> : <DataTable stack columns={columns} rows={rows} rowKey={(r) => r.ticket.id} csvName={`tickets-${project.name}-${period}.csv`} caption="Tickets" empty="No tickets yet. Connect a ticket system under Settings, or create one by hand." />}
      </Section>
      <Dialog open={editing} onOpenChange={setEditing} title={`Edit ${project.name}`} wide>
        <ProjectForm project={project} onDone={() => setEditing(false)} />
      </Dialog>
      <NewTicket open={adding} onClose={() => setAdding(false)} project={project} tickets={tickets.data ?? []} />
    </>
  );
}
