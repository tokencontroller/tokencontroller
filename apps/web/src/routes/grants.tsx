import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import type { Grant, Role } from "@/api/contract";
import { api } from "@/api/endpoints";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { ErrorBox, Loading, Note, PageHeader, Section } from "@/components/ui/display";
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
  const changeRole = useMutation({ mutationFn: ({ id, role }: { id: string; role: Role }) => api.updateMember(id, { role }), onSuccess: async () => { await refreshAll(); notify("Role changed"); } });
  const revoke = useMutation({ mutationFn: (g: Grant) => api.reverse(g.id), onSuccess: async () => { await refreshAll(); notify("Grant taken away"); } });
  return (
    <>
      <PageHeader
        title="Grants and roles"
        lead="What each person may do, and on which part of the organization."
        actions={
          <>
            <Button onClick={() => setInviting(true)}>Invite a person</Button>
            <Button variant="primary" onClick={() => setGranting(true)}>Hand on a grant</Button>
          </>
        }
      />
      {additions.parts.PeopleNote ? <additions.parts.PeopleNote /> : null}
      <Section title="Roles" lead="Agent manager: record, view on themselves. People manager: also view, assign, approve, plan and grant on their filters. Org manager: everything, adjust, the cutoff.">
        {!people.data ? (
          <Loading />
        ) : (
          <DataTable
            caption="People and roles"
            stack
            rows={people.data}
            rowKey={(p) => p.id}
            columns={[
              { key: "name", header: "Person", cell: (p) => <span>{displayName(p)}{p.status === "invited" ? <> <Badge>invited</Badge></> : null}<span className="block text-xs text-muted">{p.email}</span></span> },
              {
                key: "role",
                header: "Role",
                cell: (p) =>
                  orgManager ? (
                    <Select aria-label={`Role of ${displayName(p)}`} className="w-44" value={p.role} onChange={(e) => changeRole.mutate({ id: p.id, role: e.target.value as Role })}>
                      {(Object.keys(roleLabel) as Role[]).map((r) => <option key={r} value={r}>{roleLabel[r]}</option>)}
                    </Select>
                  ) : (
                    roleLabel[p.role]
                  ),
              },
              { key: "filters", header: "On filters", cell: (p) => (p.role === "people_manager" ? p.filters.map((f) => describeFilter(f)).join("; ") || "none yet" : p.role === "org_manager" ? "Whole organization" : "Themselves") },
            ]}
          />
        )}
        <FormError error={changeRole.error} />
      </Section>
      <Section title="Grants in force">
        {grants.error ? <ErrorBox error={grants.error} /> : null}
        {!grants.data ? (
          <Loading />
        ) : (
          <DataTable
            caption="Grants"
            stack
            rows={grants.data.filter((g) => g.capability !== "record")}
            rowKey={(g) => g.id}
            columns={[
              { key: "person", header: "Person", cell: (g) => personName(g.person) },
              { key: "capability", header: "Capability", cell: (g) => g.capability },
              { key: "filter", header: "Filter", cell: (g) => describeFilter(g.filter) },
              { key: "from", header: "From", cell: (g) => (g.from_role ? <span className="text-muted">role</span> : <Badge tone="accent">grant</Badge>) },
              { key: "act", header: "", cell: (g) => (g.from_role ? null : <Button size="sm" variant="ghost" onClick={() => revoke.mutate(g)}>Take away</Button>) },
            ]}
          />
        )}
        <FormError error={revoke.error} />
      </Section>
      <Note>Finance needs no login: hand them a report as of the cutoff.</Note>
      {granting ? <GrantDialog onClose={() => setGranting(false)} /> : null}
      {inviting ? <InviteDialog onClose={() => setInviting(false)} /> : null}
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
          <Button onClick={onClose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={!person || save.isPending}>Grant</Button>
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
        <div className="flex flex-col gap-3 text-sm">
          <p>Invited. They got an email; this is the same link:</p>
          <code className="break-all rounded-md bg-sand px-2 py-1">{link}</code>
          <div className="flex justify-end"><Button onClick={onClose}>Done</Button></div>
        </div>
      ) : (
        <form className="flex flex-col gap-4" onSubmit={(e) => { e.preventDefault(); invite.mutate(); }}>
          <Field label="Work email"><Input type="email" value={email} onChange={(e) => setEmail(e.target.value)} /></Field>
          <Field label="Name"><Input value={name} onChange={(e) => setName(e.target.value)} /></Field>
          <FormError error={invite.error} />
          <div className="flex justify-end gap-2">
            <Button onClick={onClose}>Cancel</Button>
            <Button type="submit" variant="primary" disabled={!email.trim() || invite.isPending}>Invite</Button>
          </div>
        </form>
      )}
    </Dialog>
  );
}
