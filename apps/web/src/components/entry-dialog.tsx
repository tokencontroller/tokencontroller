import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import type { Tags, TokenEntry } from "@/api/contract";
import { api } from "@/api/endpoints";
import { formatDateTime, formatTokens, tagList } from "@/lib/format";
import { refreshAll, useMe, usePersonName } from "@/lib/queries";
import { hasRole } from "@/lib/roles";
import { Badge, CostKindBadge, StatusBadge, valueLabelText } from "./ui/badge";
import { Button } from "./ui/button";
import { Dialog } from "./ui/dialog";
import { MoneyText, Note } from "./ui/display";
import { Field, FormError, Input, Select, Textarea } from "./ui/form";
import { Tag } from "./ui/tag";
import { notify } from "./ui/toast";

const builtIn = ["person", "ticket", "project"];

/** Picks a ticket, a project and any other tag on the organization's list. */
export function TagEditor({ tags, onChange }: { tags: Tags; onChange: (tags: Tags) => void }) {
  const names = useQuery({ queryKey: ["tag-names"], queryFn: api.tagNames });
  const projects = useQuery({ queryKey: ["projects"], queryFn: api.projects });
  const tickets = useQuery({ queryKey: ["tickets"], queryFn: () => api.tickets() });
  const set = (name: string, value: string) => {
    const next = Object.fromEntries(Object.entries(tags).filter(([k]) => k !== name && !(name === "ticket" && value && k === "project")));
    if (value) next[name] = value;
    onChange(next);
  };
  const others = (names.data ?? []).filter((n) => !builtIn.includes(n.name));
  return (
    <div className="grid gap-3 sm:grid-cols-2">
      <Field label="Ticket" hint="Project and client follow from the ticket.">
        <Select aria-label="Ticket" value={tags.ticket ?? ""} onChange={(e) => set("ticket", e.target.value)}>
          <option value="">No ticket</option>
          {(tickets.data ?? []).map((t) => (
            <option key={t.key} value={t.key}>
              {t.key} {t.title ? `· ${t.title}` : ""}
            </option>
          ))}
        </Select>
      </Field>
      <Field label="Project">
        <Select aria-label="Project" value={tags.project ?? ""} onChange={(e) => set("project", e.target.value)} disabled={Boolean(tags.ticket)}>
          <option value="">No project</option>
          {(projects.data ?? []).map((p) => (
            <option key={p.key} value={p.key}>
              {p.name}
            </option>
          ))}
        </Select>
      </Field>
      {others.map((name) => (
        <Field key={name.name} label={name.name}>
          <Input aria-label={name.name} list={`values-${name.name}`} value={tags[name.name] ?? ""} onChange={(e) => set(name.name, e.target.value.trim())} placeholder="none" />
          <datalist id={`values-${name.name}`}>
            {name.values.map((v) => (
              <option key={v.value} value={v.value} />
            ))}
          </datalist>
        </Field>
      ))}
    </div>
  );
}

function SplitEditor({ entry, onDone }: { entry: TokenEntry; onDone: () => void }) {
  const tickets = useQuery({ queryKey: ["tickets"], queryFn: () => api.tickets() });
  const first = entry.splits[0]?.tags.ticket ?? "";
  const [rows, setRows] = useState<{ ticket: string; percent: string }[]>(
    entry.splits.length > 1
      ? entry.splits.map((s) => ({ ticket: s.tags.ticket ?? "", percent: String(s.share / 100) }))
      : [
          { ticket: first, percent: "60" },
          { ticket: "", percent: "40" },
        ],
  );
  const total = rows.reduce((s, r) => s + (Number(r.percent) || 0), 0);
  const save = useMutation({
    mutationFn: () =>
      api.setSplits(
        entry.id,
        rows.map((r) => {
          const rest = Object.fromEntries(Object.entries(entry.splits[0]?.tags ?? {}).filter(([k]) => !["ticket", "project", "client"].includes(k)));
          return { share: Math.round((Number(r.percent) || 0) * 100), tags: r.ticket ? { ...rest, ticket: r.ticket } : rest };
        }),
      ),
    onSuccess: async () => {
      await refreshAll();
      notify("Split saved");
      onDone();
    },
  });
  return (
    <div className="space-y-3">
      <p className="text-sm text-muted">One session, several tickets. The shares are your judgment, not a measurement.</p>
      {rows.map((row, i) => (
        <div key={i} className="flex items-end gap-2">
          <Field label={`Split ${i + 1}`} className="flex-1">
            <Select aria-label={`Split ${i + 1} ticket`} value={row.ticket} onChange={(e) => setRows(rows.map((r, j) => (j === i ? { ...r, ticket: e.target.value } : r)))}>
              <option value="">No ticket</option>
              {(tickets.data ?? []).map((t) => (
                <option key={t.key} value={t.key}>
                  {t.key} {t.title ? `· ${t.title}` : ""}
                </option>
              ))}
            </Select>
          </Field>
          <Field label="Share %" className="w-24">
            <Input aria-label={`Split ${i + 1} share`} inputMode="decimal" value={row.percent} onChange={(e) => setRows(rows.map((r, j) => (j === i ? { ...r, percent: e.target.value } : r)))} />
          </Field>
        </div>
      ))}
      <div className="flex flex-wrap items-center justify-between gap-2">
        <Button size="sm" variant="ghost" onClick={() => setRows([...rows, { ticket: "", percent: "0" }])}>
          Add a split
        </Button>
        <span className={`text-sm ${total === 100 ? "text-muted" : "text-exception"}`}>Total {total}%</span>
      </div>
      <FormError error={save.error} />
      <div className="flex justify-end gap-2">
        <Button onClick={onDone}>Cancel</Button>
        <Button variant="primary" disabled={total !== 100 || save.isPending} onClick={() => save.mutate()}>
          Save split
        </Button>
      </div>
    </div>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[120px_1fr] gap-3 border-b border-line py-2 text-sm last:border-0">
      <dt className="font-mono text-[11px] uppercase tracking-wide text-muted">{label}</dt>
      <dd className="min-w-0">{children}</dd>
    </div>
  );
}

/** The token entry slip: every field, its splits, and what the viewer may do with it. */
export function EntryDialog({ entry, onClose }: { entry: TokenEntry | null; onClose: () => void }) {
  const me = useMe();
  const name = usePersonName();
  const [mode, setMode] = useState<"view" | "tag" | "split" | "reject">("view");
  const [tags, setTags] = useState<Tags>({});
  const [reason, setReason] = useState("");
  const close = () => {
    setMode("view");
    onClose();
  };
  const saveTags = useMutation({
    mutationFn: () => api.setTags(entry?.id ?? "", tags),
    onSuccess: async () => {
      await refreshAll();
      notify("Tags saved");
      close();
    },
  });
  const reject = useMutation({
    mutationFn: () => api.reject(entry?.id ?? "", reason),
    onSuccess: async () => {
      await refreshAll();
      notify("Sent back", "It is in the agent manager's review list with your reason.");
      close();
    },
  });
  const approve = useMutation({
    mutationFn: () => api.approve([entry?.id ?? ""]),
    onSuccess: async () => {
      await refreshAll();
      notify("Approved");
      close();
    },
  });
  const takeOver = useMutation({
    mutationFn: () => api.takeOver(entry?.id ?? ""),
    onSuccess: async () => {
      await refreshAll();
      notify("You are its agent manager now");
      close();
    },
  });
  if (!entry) return null;
  const mine = entry.agent_manager === me.person.id;
  const editable = entry.status !== "locked" && (mine || hasRole(me.person, "people_manager"));
  const canReview = hasRole(me.person, "people_manager") && entry.status === "submitted";
  const tokens = entry.tokens;
  const value = entry.billed_value ?? entry.list_value;
  return (
    <Dialog open={Boolean(entry)} onOpenChange={(open) => !open && close()} title="Token entry" description={entry.summary} wide>
      {entry.reject_reason ? (
        <Note tone="exception">
          <b>Sent back:</b> {entry.reject_reason}
        </Note>
      ) : null}
      {entry.agent_manager === null ? (
        <Note tone="exception">
          Nobody answers for this token entry: its service token belongs to nobody. Take it over, or move the service token to someone under Settings.
        </Note>
      ) : null}
      {mode === "view" ? (
        <>
          <dl>
            <Row label="Status">
              <span className="flex flex-wrap items-center gap-2">
                <StatusBadge status={entry.status} />
                {entry.automatic ? <Badge>automatic</Badge> : null}
                {entry.late ? <Badge tone="exception">late</Badge> : null}
              </span>
            </Row>
            <Row label="Agent">
              {entry.agent_harness}
              {entry.agent_name ? ` · ${entry.agent_name}` : ""} · <span className="font-mono text-[13px]">{entry.model}</span>
            </Row>
            <Row label="Agent manager">{name(entry.agent_manager)}</Row>
            <Row label="Start · end">
              {formatDateTime(entry.started_at)} · {formatDateTime(entry.ended_at).split(" ").slice(-2).join(" ")}
            </Row>
            <Row label="Tokens">
              <span className="num !text-left">
                in {tokens.input.toLocaleString("en-US")} · out {tokens.output.toLocaleString("en-US")} · cache write {formatTokens(tokens.cache_write)} · cache read {formatTokens(tokens.cache_read)}
              </span>
            </Row>
            <Row label="Cost">
              <MoneyText value={value} label={entry.value_label} /> <span className="text-xs text-muted">{valueLabelText[entry.value_label]}</span>
            </Row>
            <Row label="Placed by">{entry.placed_by}</Row>
            <Row label={entry.splits.length > 1 ? "Splits" : "Tags"}>
              <ul className="space-y-2">
                {entry.splits.map((split, i) => (
                  <li key={i} className="flex flex-wrap items-center gap-1.5">
                    {entry.splits.length > 1 ? <span className="num mr-1 text-xs text-muted">{split.share / 100}%</span> : null}
                    {tagList(split.tags).map(([n, v]) => (
                      <Tag key={n} name={n} value={v} />
                    ))}
                    <CostKindBadge kind={split.cost_kind} />
                    {entry.splits.length > 1 ? <MoneyText value={split.value} /> : null}
                  </li>
                ))}
              </ul>
              {entry.candidates.length ? <p className="mt-2 text-xs text-muted">Candidates: {entry.candidates.join(", ")}</p> : null}
            </Row>
          </dl>
          <div className="mt-5 flex flex-wrap justify-end gap-2">
            {entry.agent_manager === null && hasRole(me.person, "people_manager") ? (
              <Button onClick={() => takeOver.mutate()}>Take it over</Button>
            ) : null}
            {canReview ? (
              <>
                <Button variant="danger" onClick={() => setMode("reject")}>
                  Reject
                </Button>
                {me.organization.approval ? (
                  <Button onClick={() => approve.mutate()} disabled={approve.isPending}>
                    Approve
                  </Button>
                ) : null}
              </>
            ) : null}
            {editable ? (
              <>
                <Button onClick={() => setMode("split")}>Split</Button>
                <Button
                  variant="primary"
                  onClick={() => {
                    setTags(Object.fromEntries(Object.entries(entry.splits[0]?.tags ?? {}).filter(([k]) => k !== "person" && k !== "client")));
                    setMode("tag");
                  }}
                >
                  Change tags
                </Button>
              </>
            ) : null}
          </div>
        </>
      ) : null}
      {mode === "tag" ? (
        <div className="space-y-4">
          <TagEditor tags={tags} onChange={setTags} />
          <FormError error={saveTags.error} />
          <div className="flex justify-end gap-2">
            <Button onClick={() => setMode("view")}>Cancel</Button>
            <Button variant="primary" onClick={() => saveTags.mutate()} disabled={saveTags.isPending}>
              Save tags
            </Button>
          </div>
        </div>
      ) : null}
      {mode === "split" ? <SplitEditor entry={entry} onDone={() => setMode("view")} /> : null}
      {mode === "reject" ? (
        <div className="space-y-4">
          <Field label="Reason" hint="The agent manager sees it in their review list.">
            <Textarea autoFocus value={reason} onChange={(e) => setReason(e.target.value)} placeholder="This was for KD-231, not KD-214." />
          </Field>
          <FormError error={reject.error} />
          <div className="flex justify-end gap-2">
            <Button onClick={() => setMode("view")}>Cancel</Button>
            <Button variant="primary" disabled={!reason.trim() || reject.isPending} onClick={() => reject.mutate()}>
              Send back
            </Button>
          </div>
        </div>
      ) : null}
    </Dialog>
  );
}

/** The value and its label, as one table cell. */
export function entryValue(entry: TokenEntry) {
  return entry.billed_value ?? entry.list_value;
}
