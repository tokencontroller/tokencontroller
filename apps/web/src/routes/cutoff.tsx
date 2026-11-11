import { useMutation, useQuery } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import { api } from "@/api/endpoints";
import { Amount } from "@/components/controls";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { ErrorBox, Kpi, KpiRow, Loading, PageHeader, Section } from "@/components/ui/display";
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
  const orgOverhead = overhead.data?.rows.find((r) => r.key === "organization overhead")?.amount ?? "0.00";
  const checks: { key: string; ok: boolean; text: ReactNode }[] = [
    ...inRange.map((r) => ({
      key: r.bill_id,
      ok: !r.a_message_is_wrong,
      text:
        r.contract_kind === "flat" ? (
          <>{r.contract} bill {formatRange(r.from, r.to)}: covered <Amount value={r.covered ?? null} />{r.a_message_is_wrong ? ", negative: a message is wrong." : "."}</>
        ) : (
          <>{r.contract} bill {formatRange(r.from, r.to)}: what no message explains <Amount value={r.unexplained} />{r.a_message_is_wrong ? ", negative: a message is wrong." : "."}</>
        ),
    })),
    ...(inRange.length ? [] : [{ key: "no-bill", ok: true, text: "No bill ends between the cutoff and this day." }]),
    { key: "overhead", ok: Number(orgOverhead) === 0, text: <>Organization overhead since the cutoff: <Amount value={orgOverhead} />. Some of it may belong on a project.</> },
    ...(me.organization.approval
      ? [{ key: "submissions", ok: !pending.length, text: pending.length ? `Submissions not approved yet: ${pending.map((s) => `${personName(s.person)} (${s.status})`).join(", ")}.` : "Every submission in the range is approved." }]
      : []),
  ];
  const open = checks.filter((c) => !c.ok).length;
  return (
    <>
      <PageHeader title="Cutoff" lead="Figures up to the cutoff are frozen; after it they are live." />
      {cutoff.error ? <ErrorBox error={cutoff.error} /> : null}
      {!cutoff.data ? (
        <Loading />
      ) : (
        <KpiRow>
          <Kpi
            hero
            label="Cutoff"
            value={current ? formatDate(current) : "none"}
            note={cutoff.data.moved_at ? `moved ${formatDateTime(cutoff.data.moved_at)} by ${personName(cutoff.data.moved_by)}` : "every figure is live"}
          />
          <Kpi label="To look at before moving it" value={open} tone={open ? "exception" : "posted"} note={`of ${String(checks.length)} checks`} />
        </KpiRow>
      )}
      <form
        onSubmit={(e) => {
          e.preventDefault();
          move.mutate();
        }}
      >
        <Section
          title={`Before you move it to ${formatDate(target)}`}
          flush
          hint="Nothing is edited. A wrong split before the cutoff gets a reversal and a new split; an org manager with adjust may date them before the cutoff, and the log says who did it. Moving the cutoff back is allowed."
          footer={
            <>
              <FormError error={move.error} />
              <Field label="New cutoff" className="w-44">
                <Input type="date" value={target} max={today()} onChange={(e) => setDay(e.target.value)} />
              </Field>
              <Button type="submit" variant="primary" className="self-end" disabled={move.isPending}>
                Move the cutoff
              </Button>
            </>
          }
        >
          <ul className="border-t border-line">
            {checks.map((c) => (
              <li key={c.key} className="flex items-start gap-3 border-b border-line px-5 py-3 text-body last:border-b-0">
                <CheckIcon ok={c.ok} />
                <span>{c.text}</span>
                <span className="ml-auto shrink-0">{c.ok ? <Badge tone="posted">fine</Badge> : <Badge tone="exception">look</Badge>}</span>
              </li>
            ))}
          </ul>
        </Section>
      </form>
    </>
  );
}

function CheckIcon({ ok }: { ok: boolean }) {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true" className={`mt-0.5 shrink-0 ${ok ? "text-posted" : "text-exception"}`}>
      <circle cx="8" cy="8" r="6.25" fill="none" stroke="currentColor" strokeWidth="1.4" />
      {ok ? <path d="M5.3 8.2 7.2 10l3.5-4" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" /> : <path d="M8 4.8v3.8M8 10.9v.1" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />}
    </svg>
  );
}
