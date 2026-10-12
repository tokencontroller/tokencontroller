import { useMutation } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useState } from "react";
import type { EntryView, Money, Person, Project, SplitView, Tags, ValueLabel } from "@/api/contract";
import { formatDateTime, formatTime, tagList } from "@/lib/format";
import { formatShare, fromDecimal, nanosOf, shareOf, sum } from "@/lib/money";
import { api } from "@/api/endpoints";
import { refreshAll, useMe, useProjects, useTagNames, useTickets } from "@/lib/queries";
import { hasRole } from "@/lib/roles";
import { Badge, StatusBadge, valueLabelText } from "./ui/badge";
import { Button } from "./ui/button";
import { Dialog } from "./ui/dialog";
import { MoneyText, Note } from "./ui/display";
import { Field, FormError, Input, Select, Textarea } from "./ui/form";
import { Tag, TagValueList } from "./ui/tag";
import { notify } from "./ui/toast";

const builtIn = ["person", "ticket", "project"];

/** The most splits a token entry can be split into by hand. */
const MAX_SPLITS = 20;

/** A token entry's list value, or null for an unknown model. */
export function entryValue(entry: EntryView): Money | null {
  return entry.list_value ? fromDecimal(entry.list_value, entry.currency) : null;
}

/** The total list value of token entries in one currency; unknown models are left out, never counted as zero. */
export function listTotal(entries: EntryView[], fallback: string): Money {
  const currency = entries.find((e) => e.list_value)?.currency ?? fallback;
  return sum(
    entries.filter((e) => e.currency === currency).flatMap((e) => {
      const value = entryValue(e);
      return value ? [value] : [];
    }),
    currency,
  );
}

/** How a token entry's figure is labelled: a list-price estimate, or an unknown model. */
export function valueLabel(entry: EntryView): ValueLabel {
  return entry.list_value ? "list_estimate" : "unknown_model";
}

/** A split's share of its token entry in basis points: as set by hand, else from the list values when both are known. */
export function splitShare(entry: EntryView, split: SplitView): number | null {
  if (split.share_percent != null) return split.share_percent * 100;
  const whole = entryValue(entry);
  if (!whole || !split.list_value || nanosOf(whole) === 0n) return null;
  return shareOf(fromDecimal(split.list_value, entry.currency), whole);
}

/** Whether a token entry is the member's own: they are its agent manager. */
export function isOwn(entry: EntryView, person: Person): boolean {
  return entry.agent_manager_id === person.id;
}

/** The unknown tickets of a token entry: keys that matched a pattern but that the ticket sync did not find. */
export function unknownTickets(entry: EntryView): string[] {
  return [...new Set(entry.splits.flatMap((split) => (split.unknown_ticket ? [split.unknown_ticket] : [])))];
}

/**
 * A split's tags as a person names them when placing it by hand: without the
 * person, which comes from who did the work, and without what the project
 * passes on, which follows from the ticket or project again.
 */
export function namedTags(tags: Tags, projects: Project[] | undefined): Tags {
  const passed = projects?.find((p) => p.name === tags.project)?.tags ?? {};
  return Object.fromEntries(Object.entries(tags).filter(([name, value]) => name !== "person" && passed[name] !== value));
}

/** Picks a ticket, a project and any other tag on the organization's list. */
export function TagEditor({ tags, onChange }: { tags: Tags; onChange: (tags: Tags) => void }) {
  const names = useTagNames();
  const projects = useProjects();
  const tickets = useTickets();
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
            <option key={p.id} value={p.name}>
              {p.name}
            </option>
          ))}
        </Select>
      </Field>
      {others.map((name) => (
        <Field key={name.name} label={name.name}>
          <Input aria-label={name.name} list={`values-${name.name}`} value={tags[name.name] ?? ""} onChange={(e) => set(name.name, e.target.value.trim())} placeholder="none" />
          <TagValueList id={`values-${name.name}`} name={name.name} />
        </Field>
      ))}
    </div>
  );
}

function SplitEditor({ entry, onDone }: { entry: EntryView; onDone: () => void }) {
  const tickets = useTickets();
  const projects = useProjects();
  const first = entry.splits[0]?.tags ?? {};
  const [rows, setRows] = useState<{ ticket: string; percent: string }[]>(() => {
    if (entry.splits.length < 2) {
      return [
        { ticket: first.ticket ?? "", percent: "60" },
        { ticket: "", percent: "40" },
      ];
    }
    return entry.splits.map((split) => {
      const share = splitShare(entry, split);
      return { ticket: split.tags.ticket ?? "", percent: share === null ? "" : String(Math.round(share / 100)) };
    });
  });
  const percents = rows.map((r) => (/^\d+$/.test(r.percent.trim()) ? Number(r.percent) : NaN));
  const total = percents.reduce((sum, p) => sum + (Number.isNaN(p) ? 0 : p), 0);
  const valid = percents.every((p) => Number.isInteger(p) && p > 0) && total === 100;
  const save = useMutation({
    mutationFn: () => {
      // The other tags stay; each split gets its own ticket, and its project follows from it.
      const rest = Object.fromEntries(Object.entries(namedTags(first, projects.data)).filter(([k]) => k !== "ticket" && k !== "project"));
      return api.setSplits(
        entry.id,
        rows.map((r) => ({ share_percent: Number(r.percent), tags: r.ticket ? { ...rest, ticket: r.ticket } : rest })),
      );
    },
    onSuccess: async () => {
      await refreshAll();
      notify("Split saved");
      onDone();
    },
  });
  const change = (i: number, row: Partial<{ ticket: string; percent: string }>) => setRows(rows.map((r, j) => (j === i ? { ...r, ...row } : r)));
  return (
    <div className="space-y-3">
      <p className="text-sm text-muted">One session, several tickets. The shares are your judgment, not a measurement: whole percents that add up to 100.</p>
      {rows.map((row, i) => (
        <div key={i} className="flex items-end gap-2">
          <Field label={`Split ${String(i + 1)}`} className="min-w-0 flex-1">
            <Select aria-label={`Split ${String(i + 1)} ticket`} value={row.ticket} onChange={(e) => change(i, { ticket: e.target.value })}>
              <option value="">No ticket</option>
              {(tickets.data ?? []).map((t) => (
                <option key={t.key} value={t.key}>
                  {t.key} {t.title ? `· ${t.title}` : ""}
                </option>
              ))}
            </Select>
          </Field>
          <Field label="Share %" className="w-20 shrink-0">
            <Input aria-label={`Split ${String(i + 1)} share`} inputMode="numeric" value={row.percent} onChange={(e) => change(i, { percent: e.target.value })} />
          </Field>
          {rows.length > 2 ? (
            <Button size="sm" variant="ghost" aria-label={`Remove split ${String(i + 1)}`} onClick={() => setRows(rows.filter((_, j) => j !== i))}>
              ×
            </Button>
          ) : null}
        </div>
      ))}
      <div className="flex flex-wrap items-center justify-between gap-2">
        <Button size="sm" variant="ghost" disabled={rows.length >= MAX_SPLITS} onClick={() => setRows([...rows, { ticket: "", percent: "" }])}>
          Add a split
        </Button>
        <span className={`text-sm ${valid ? "text-muted" : "text-exception"}`}>Total {total}%</span>
      </div>
      <FormError error={save.error} />
      <div className="flex justify-end gap-2">
        <Button onClick={onDone}>Cancel</Button>
        <Button variant="primary" disabled={!valid || save.isPending} onClick={() => save.mutate()}>
          Save split
        </Button>
      </div>
    </div>
  );
}

/** A token entry's tokens: input, output and the cache, in the provider's words. */
function TokenCounts({ tokens }: { tokens: EntryView["tokens"] }) {
  const n = (count: number) => count.toLocaleString("en-US");
  const parts = [`${n(tokens.input)} input`, `${n(tokens.output)} output`, ...(tokens.cache_write ? [`${n(tokens.cache_write)} cache write`] : []), ...(tokens.cache_read ? [`${n(tokens.cache_read)} cache read`] : [])];
  return <span className="num !text-left">{parts.join(" · ")}</span>;
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[110px_1fr] gap-3 border-b border-line py-2 text-sm last:border-0 sm:grid-cols-[120px_1fr]">
      <dt className="font-mono text-[11px] uppercase tracking-wide text-muted">{label}</dt>
      <dd className="min-w-0">{children}</dd>
    </div>
  );
}

/** One split: its share and list value when there are several, its tags, what placed it and an unknown ticket. */
function SplitLine({ entry, split }: { entry: EntryView; split: SplitView }) {
  const several = entry.splits.length > 1;
  const share = several ? splitShare(entry, split) : null;
  return (
    <li className="space-y-1">
      <span className="flex flex-wrap items-center gap-1.5">
        {share !== null ? <span className="num mr-1 text-xs text-muted">{formatShare(share)}</span> : null}
        {tagList(split.tags).map(([n, v]) => (
          <Tag key={n} name={n} value={v} />
        ))}
        {several ? <MoneyText value={split.list_value ? fromDecimal(split.list_value, entry.currency) : null} label={valueLabel(entry)} /> : null}
      </span>
      <span className="flex flex-wrap items-center gap-1.5 text-xs text-muted">
        {split.placed_by}
        {split.unknown_ticket ? <Badge tone="exception">unknown ticket {split.unknown_ticket}</Badge> : null}
      </span>
    </li>
  );
}

/** The token entry slip: every field the service gives, its splits, and what the viewer may do with it. */
export function EntryDialog({ entry, onClose }: { entry: EntryView | null; onClose: () => void }) {
  const me = useMe();
  const projects = useProjects();
  const [mode, setMode] = useState<"view" | "tag" | "split" | "reject">("view");
  const [tags, setTags] = useState<Tags>({});
  const [reason, setReason] = useState("");
  const close = () => {
    setMode("view");
    setReason("");
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
    mutationFn: () => api.approve({ token_entry_ids: [entry?.id ?? ""] }),
    onSuccess: async (r) => {
      await refreshAll();
      if (r.approved) notify("Approved");
      else notify("Not approved", "It is no longer submitted, or not within your filters.");
      close();
    },
  });
  if (!entry) return null;
  const mine = isOwn(entry, me.person);
  // The service lets you fix your own token entries until they are approved or locked.
  const editable = mine && ["draft", "rejected", "submitted"].includes(entry.status);
  // People managers decide within their filters, never on their own; controllers on any.
  const decides = hasRole(me.person, "people_manager") && (!mine || hasRole(me.person, "controller"));
  const canApprove = decides && me.organization.approval && entry.status === "submitted";
  const canReject = decides && (entry.status === "submitted" || entry.status === "approved");
  const nobody = (entry.agent_manager ?? null) === null;
  const label = valueLabel(entry);
  return (
    <Dialog open={Boolean(entry)} onOpenChange={(open) => !open && close()} title="Token entry" description={entry.summary} wide>
      {entry.status === "rejected" && entry.rejected_reason ? (
        <Note tone="exception">
          <b>Sent back:</b> {entry.rejected_reason}
        </Note>
      ) : null}
      {nobody ? (
        <Note tone="exception">
          Nobody answers for this token entry: its service token belongs to nobody. Move the service token to someone under Settings; only future token entries follow.
        </Note>
      ) : null}
      {mode === "view" ? (
        <>
          <dl>
            <Row label="Status">
              <span className="flex flex-wrap items-center gap-2">
                <StatusBadge status={entry.status} />
                {entry.automatic ? <Badge>automatic</Badge> : null}
              </span>
            </Row>
            <Row label="Agent">
              {entry.agent_harness}
              {entry.agent_name ? ` · ${entry.agent_name}` : ""} · <span className="break-all font-mono text-[13px]">{entry.model}</span>
            </Row>
            <Row label="Provider">{entry.provider}</Row>
            <Row label="Agent manager">{nobody ? <Badge tone="exception">nobody</Badge> : entry.agent_manager}</Row>
            <Row label="Time">
              {formatDateTime(entry.started_at)} to {entry.ended_at.slice(0, 10) === entry.started_at.slice(0, 10) ? `${formatTime(entry.ended_at)} UTC` : formatDateTime(entry.ended_at)}
            </Row>
            <Row label="Tokens">
              <TokenCounts tokens={entry.tokens} />
            </Row>
            <Row label="List value">
              <MoneyText value={entryValue(entry)} label={label} /> {label === "unknown_model" ? null : <span className="text-xs text-muted">{valueLabelText[label]}</span>}
            </Row>
            <Row label={entry.splits.length > 1 ? `${String(entry.splits.length)} splits` : "Tags"}>
              <ul className="space-y-3">
                {entry.splits.map((split) => (
                  <SplitLine key={split.id} entry={entry} split={split} />
                ))}
              </ul>
            </Row>
          </dl>
          <div className="mt-5 flex flex-wrap justify-end gap-2">
            {nobody && hasRole(me.person, "controller") ? (
              // Its service token belongs to nobody: hand the token over (docs: Roles).
              <Link to="/settings" hash="api-tokens" className="inline-flex h-9 items-center rounded-md px-3 text-sm text-accent hover:underline">
                Hand over its service token
              </Link>
            ) : null}
            {canReject ? (
              <Button variant="danger" onClick={() => setMode("reject")}>
                Reject
              </Button>
            ) : null}
            {canApprove ? (
              <Button onClick={() => approve.mutate()} disabled={approve.isPending}>
                Approve
              </Button>
            ) : null}
            {editable ? (
              <>
                <Button onClick={() => setMode("split")}>Split</Button>
                <Button
                  variant="primary"
                  onClick={() => {
                    setTags(namedTags(entry.splits[0]?.tags ?? {}, projects.data));
                    setMode("tag");
                  }}
                >
                  Change tags
                </Button>
              </>
            ) : null}
          </div>
          <FormError error={approve.error} />
        </>
      ) : null}
      {mode === "tag" ? (
        <div className="space-y-4">
          {entry.splits.length > 1 ? <p className="text-sm text-muted">Saving tags makes it one split again.</p> : null}
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
          <Field label="Reason" hint="The agent manager sees it in their review list. It does not count until it is fixed and submitted again.">
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
