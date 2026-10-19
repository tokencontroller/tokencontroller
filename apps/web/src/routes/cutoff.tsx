import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import { api } from "@/api/endpoints";
import { Amount } from "@/components/controls";
import { Button } from "@/components/ui/button";
import { ErrorBox, Kpi, KpiRow, Loading, Note, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Input } from "@/components/ui/form";
import { notify } from "@/components/ui/toast";
import { formatDate, formatDateTime, formatRange } from "@/lib/format";
import { refreshAll, useMe, usePersonName } from "@/lib/queries";
import { today } from "@/lib/today";

// Docs: Cutoff. One date per organization: figures as of it are frozen,
// everything after is live. Only an org manager with adjust adds events
// dated before it. Before moving it: reconciliation, organization overhead,
// open or rejected submissions.
export function CutoffPage(): ReactNode {
  const me = useMe();
  const cutoff = useQuery({ queryKey: ["cutoff"], queryFn: api.cutoff });
  const reconciliation = useQuery({ queryKey: ["reconciliation"], queryFn: api.reconciliation });
  const submissions = useQuery({ queryKey: ["submissions"], queryFn: api.submissions });
  const personName = usePersonName();
  const latest = reconciliation.data?.toSorted((a, b) => b.to.localeCompare(a.to))[0];
  const [day, setDay] = useState("");
  const target = day || latest?.to || today();
  const current = cutoff.data?.cutoff ?? null;
  const since = current ? new Date(Date.parse(current) + 86_400_000).toISOString().slice(0, 10) : "2000-01-01";
  const overhead = useQuery({ queryKey: ["cutoff-overhead", since, target], queryFn: () => api.cost({ from: since, to: target, by: "label" }), enabled: target >= since });
  const move = useMutation({
    mutationFn: () => api.moveCutoff(target),
    onSuccess: async () => {
      await refreshAll();
      notify("Cutoff moved", formatDate(target));
    },
  });
  const inRange = reconciliation.data?.filter((r) => r.to <= target && r.to > (current ?? "")) ?? [];
  const pending = submissions.data?.filter((s) => s.status !== "approved" && s.to <= target) ?? [];
  const orgOverhead = overhead.data?.rows.find((r) => r.key === "organization_overhead")?.amount ?? "0.00";
  return (
    <>
      <PageHeader title="Cutoff" lead="Every figure as of the cutoff is frozen; everything after it is live. There are no months to close." />
      {cutoff.error ? <ErrorBox error={cutoff.error} /> : null}
      {!cutoff.data ? (
        <Loading />
      ) : (
        <KpiRow>
          <Kpi
            label="Cutoff"
            value={current ? formatDate(current) : "none"}
            note={cutoff.data.moved_at ? `moved ${formatDateTime(cutoff.data.moved_at)} by ${personName(cutoff.data.moved_by)}` : "Every figure is live."}
          />
        </KpiRow>
      )}
      <Section title="Move it" lead={`Checks before moving it to ${formatDate(target)}. Moving it back is allowed and is an event like any other.`}>
        <ul className="mb-4 flex max-w-2xl flex-col gap-2 text-sm">
          {inRange.map((r) => (
            <li key={r.bill_id} className={r.a_message_is_wrong ? "text-exception" : ""}>
              {r.provider} bill {formatRange(r.from, r.to)}: what no message explains <Amount value={r.unexplained} />
              {r.a_message_is_wrong ? ", negative: a message is wrong." : ", zero or positive."}
            </li>
          ))}
          {!inRange.length ? <li className="text-muted">No bill ends between the cutoff and this day.</li> : null}
          <li>
            Organization overhead since the cutoff: <Amount value={orgOverhead} />. Check whether some of it belongs on a project.
          </li>
          {me.organization.approval ? (
            <li className={pending.length ? "text-exception" : ""}>
              {pending.length ? `Submissions not approved yet: ${pending.map((s) => `${personName(s.person)} (${s.status})`).join(", ")}.` : "Every submission in the range is approved."}
            </li>
          ) : null}
        </ul>
        <form
          className="flex max-w-md items-end gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            move.mutate();
          }}
        >
          <Field label="New cutoff" className="flex-1">
            <Input type="date" value={target} max={today()} onChange={(e) => setDay(e.target.value)} />
          </Field>
          <Button type="submit" variant="primary" disabled={move.isPending}>
            Move the cutoff
          </Button>
        </form>
        <FormError error={move.error} />
      </Section>
      <Note>
        Nothing is edited. A wrong split before the cutoff gets a reversal and a new split. An org manager with adjust can date them before the cutoff, so the frozen figures change on purpose and the
        log says who did it and when.
      </Note>
    </>
  );
}
