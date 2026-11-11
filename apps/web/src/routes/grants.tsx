import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { Grant, Member, Role } from "@/api/contract";
import { api } from "@/api/endpoints";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Loading, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input, Select } from "@/components/ui/form";
import { DataTable } from "@/components/ui/table";
import { notify } from "@/components/ui/toast";
import { additions } from "@/lib/additions";
import { describeFilter, displayName, filterFromText } from "@/lib/format";
import { refreshAll, useMe, usePeople, usePersonName } from "@/lib/queries";
import { hasRole, roleLabel } from "@/lib/roles";

const capabilities = ["view", "assign", "approve", "plan", "grant"] as const;

// Docs: Grants and roles. A grant is (person, capability, filter); a role is
// a named bundle of grants. A new person gets record and view on themselves.
export function GrantsPage(): ReactNode {
  const me = useMe();
  const people = usePeople();
  const grants = useQuery({ queryKey: ["grants"], queryFn: api.grants });
  const personName = usePersonName();
  const orgManager = hasRole(me.person, "org_manager");
  const [granting, setGranting] = useState(false);
  const [inviting, setInviting] = useState(false);
  // A people manager is granted on filters, so picking that role asks for one.
  const [promoting, setPromoting] = useState<Member | null>(null);
  const changeRole = useMutation({ mutationFn: ({ id, role }: { id: string; role: Role }) => api.updateMember(id, { role }), onSuccess: async () => { await refreshAll(); notify("Role changed"); } });
  const revoke = useMutation({ mutationFn: (g: Grant) => api.reverse(g.id), onSuccess: async () => { await refreshAll(); notify("Grant taken away"); } });
  // Grants that come with a role are shown by the role; the card lists the extra ones unless asked.
  const [roleGrants, setRoleGrants] = useState(false);
  const shown = grants.data?.filter((g) => g.capability !== "record" && (roleGrants || !g.from_role));
  return (
    <>
      <PageHeader
        title="Grants and roles"
        actions={
          <>
            <Button onClick={() => setInviting(true)}>Invite a person</Button>
            <Button variant="primary" onClick={() => setGranting(true)}>Hand on a grant</Button>
          </>
        }
      />
      {additions.parts.PeopleNote ? <additions.parts.PeopleNote /> : null}
      <Section
        title="Roles"
        flush
        hint="Agent manager: record and view on themselves. People manager: also view, assign, approve, plan and grant on their filters. Org manager: everything, adjust and the cutoff."
      >
        {!people.data ? (
          <div className="px-5 pb-5"><Loading /></div>
        ) : (
          <DataTable
            caption="People and roles"
            stack
            rows={people.data}
            rowKey={(p) => p.id}
            columns={[
              { key: "name", header: "Person", cell: (p) => <span className="inline-flex flex-col items-end sm:items-start"><span className="flex items-center gap-1.5 font-medium">{displayName(p)}{p.status === "invited" ? <Badge>invited</Badge> : null}</span><span className="text-caption text-muted">{p.email}</span></span> },
              {
                key: "role",
                header: "Role",
                cell: (p) =>
                  orgManager ? (
                    <Select aria-label={`Role of ${displayName(p)}`} className="w-44" value={p.role} onChange={(e) => { const role = e.target.value as Role; if (role === "people_manager") setPromoting(p); else changeRole.mutate({ id: p.id, role }); }}>
                      {(Object.keys(roleLabel) as Role[]).map((r) => <option key={r} value={r}>{roleLabel[r]}</option>)}
                    </Select>
                  ) : (
                    roleLabel[p.role]
                  ),
              },
              { key: "filters", header: "On", cell: (p) => <span className="text-ink-soft">{p.role === "people_manager" ? p.filters.map((f) => describeFilter(f)).join("; ") || "no filter yet" : p.role === "org_manager" ? "Whole organization" : "Themselves"}</span> },
            ]}
          />
        )}
        {changeRole.error ? <div className="px-5 py-3"><FormError error={changeRole.error} /></div> : null}
      </Section>
      <Section
        title="Grants in force"
        flush
        hint="Grants handed on one by one, beside the ones a role brings."
        actions={<Button size="sm" variant="ghost" onClick={() => setRoleGrants(!roleGrants)} aria-pressed={roleGrants}>{roleGrants ? "Hide role grants" : "Show role grants"}</Button>}
      >
        {grants.error ? <div className="px-5 pb-5"><ErrorBox error={grants.error} /></div> : null}
        {!shown ? (
          <div className="px-5 pb-5"><Loading /></div>
        ) : (
          <DataTable
            caption="Grants"
            stack
            rows={shown}
            rowKey={(g) => g.id}
            empty="No grants beyond the roles."
            columns={[
              { key: "person", header: "Person", cell: (g) => <span className="font-medium">{personName(g.person)}</span> },
              { key: "capability", header: "Capability", cell: (g) => g.capability },
              { key: "filter", header: "On", cell: (g) => describeFilter(g.filter) },
              { key: "from", header: "From", cell: (g) => (g.from_role ? <Badge>role</Badge> : <Badge tone="accent">grant</Badge>) },
              { key: "act", header: "", align: "right", cell: (g) => (g.from_role ? null : <Button size="sm" variant="danger" onClick={() => revoke.mutate(g)}>Take away</Button>) },
            ]}
          />
        )}
        {revoke.error ? <div className="px-5 py-3"><FormError error={revoke.error} /></div> : null}
      </Section>
      {granting ? <GrantDialog onClose={() => setGranting(false)} /> : null}
      {inviting ? <InviteDialog onClose={() => setInviting(false)} /> : null}
      {promoting ? <PeopleManagerDialog person={promoting} onClose={() => setPromoting(null)} /> : null}
    </>
  );
}

function GrantDialog({ onClose }: { onClose: () => void }) {
  const people = usePeople();
  const [person, setPerson] = useState("");
  const [capability, setCapability] = useState<string>("view");
  const [filter, setFilter] = useState("");
  const save = useMutation({
    mutationFn: () => api.addGrant({ person, capability, filter: filterFromText(filter.replace(/\s+/g, "")) }),
    onSuccess: async () => {
      await refreshAll();
      notify("Grant handed on");
      onClose();
    },
  });
  return (
    <Dialog open onOpenChange={(o) => { if (!o) onClose(); }} title="Hand on a grant" description="For a filter inside your own. A new grant for the same person and capability supersedes the old one.">
      <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
        <Field label="Person">
          <Select value={person} onChange={(e) => setPerson(e.target.value)}>
            <option value="">Pick a person</option>
            {(people.data ?? []).map((p) => <option key={p.id} value={p.email}>{displayName(p)}</option>)}
          </Select>
        </Field>
        <Field label="Capability">
          <Select value={capability} onChange={(e) => setCapability(e.target.value)}>
            {capabilities.map((c) => <option key={c} value={c}>{c}</option>)}
          </Select>
        </Field>
        <Field label="On filter" hint="Such as team:web or client:acme. Empty: the whole organization."><Input value={filter} onChange={(e) => setFilter(e.target.value)} /></Field>
        <FormError error={save.error} />
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={!person || save.isPending}>Grant</Button>
        </div>
      </form>
    </Dialog>
  );
}

function PeopleManagerDialog({ person, onClose }: { person: Member; onClose: () => void }) {
  const [filter, setFilter] = useState(person.filters.map((f) => Object.entries(f).map(([name, value]) => `${name}:${value}`).join(",")).join("; "));
  const save = useMutation({
    mutationFn: () => api.updateMember(person.id, { role: "people_manager", filters: filter.split(";").map((part) => filterFromText(part.replace(/\s+/g, ""))).filter((f) => Object.keys(f).length > 0) }),
    onSuccess: async () => {
      await refreshAll();
      notify("Role changed");
      onClose();
    },
  });
  return (
    <Dialog open onOpenChange={(o) => { if (!o) onClose(); }} title="Make a people manager" description={`${displayName(person)} gets view, assign, approve, plan and grant on these filters.`}>
      <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); save.mutate(); }}>
        <Field label="On filters" hint="Such as team:web or client:acme. Separate several with a semicolon."><Input value={filter} onChange={(e) => setFilter(e.target.value)} /></Field>
        <FormError error={save.error} />
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={!filter.trim() || save.isPending}>Make people manager</Button>
        </div>
      </form>
    </Dialog>
  );
}

function InviteDialog({ onClose }: { onClose: () => void }) {
  const [email, setEmail] = useState("");
  const [name, setName] = useState("");
  const [link, setLink] = useState<string | null>(null);
  const invite = useMutation({
    mutationFn: () => api.addMember({ email: email.trim(), ...(name.trim() ? { name: name.trim() } : {}) }),
    onSuccess: async (answer) => {
      await refreshAll();
      setLink(answer.invite_url);
    },
  });
  return (
    <Dialog open onOpenChange={(o) => { if (!o) onClose(); }} title="Invite a person" description="They get record and view on themselves.">
      {link ? (
        <div className="flex flex-col gap-3 text-body">
          <p>Invited. They got an email; this is the same link:</p>
          <code className="break-all rounded-md bg-hover px-2 py-1 font-mono text-small ring-1 ring-inset ring-line">{link}</code>
          <div className="flex justify-end"><Button variant="primary" onClick={onClose}>Done</Button></div>
        </div>
      ) : (
        <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); invite.mutate(); }}>
          <Field label="Work email"><Input type="email" value={email} onChange={(e) => setEmail(e.target.value)} /></Field>
          <Field label="Name"><Input value={name} onChange={(e) => setName(e.target.value)} /></Field>
          <FormError error={invite.error} />
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={onClose}>Cancel</Button>
            <Button type="submit" variant="primary" disabled={!email.trim() || invite.isPending}>Invite</Button>
          </div>
        </form>
      )}
    </Dialog>
  );
}
