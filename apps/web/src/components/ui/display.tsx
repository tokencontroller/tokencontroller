import type { Money, ValueLabel } from "@/api/contract";
import { formatMoney, formatShare } from "@/lib/money";
import { useEffect, type ReactNode } from "react";
import { additions } from "@/lib/additions";
import { valueLabelText } from "./badge";

export function MoneyText({ value, label, signed = false, share }: { value: Money | null; label?: ValueLabel; signed?: boolean; share?: number | undefined }) {
  if (share !== undefined) return <span className="num">{formatShare(share)}</span>;
  if (value === null || label === "unknown_model") {
    return <span className="num text-exception" title="The model is missing from the price table, so its cost is not known yet.">unknown model</span>;
  }
  return (
    <span className="num" title={label ? valueLabelText[label] : undefined}>
      {formatMoney(value, { signed })}
    </span>
  );
}

export function PageHeader({ title, lead, actions }: { title: string; lead?: ReactNode; actions?: ReactNode }) {
  return (
    <header className="mb-6 flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
      <div className="min-w-0">
        <h1 className="text-[22px] font-semibold tracking-[-0.01em]">{title}</h1>
        {lead ? <p className="mt-1 max-w-2xl text-sm text-muted">{lead}</p> : null}
      </div>
      {actions ? <div className="flex flex-wrap items-center gap-2">{actions}</div> : null}
    </header>
  );
}

export function Section({ id, title, actions, children, lead }: { id?: string; title: string; actions?: ReactNode; children: ReactNode; lead?: ReactNode }) {
  return (
    <section id={id} className="mb-8 scroll-mt-20">
      <div className="mb-3 flex flex-wrap items-end justify-between gap-2">
        <div>
          <h2 className="text-[15px] font-semibold">{title}</h2>
          {lead ? <p className="mt-0.5 text-[13px] text-muted">{lead}</p> : null}
        </div>
        {actions ? <div className="flex flex-wrap items-center gap-2">{actions}</div> : null}
      </div>
      {children}
    </section>
  );
}

export function Kpi({ label, value, note, tone }: { label: string; value: ReactNode; note?: ReactNode; tone?: "posted" | "exception" | undefined }) {
  return (
    <div className="rounded-lg border border-line bg-page p-4">
      <div className="text-[13px] text-muted">{label}</div>
      <div className={`mt-1 text-[22px] font-light tracking-[-0.01em] num !text-left ${tone === "posted" ? "text-posted" : tone === "exception" ? "text-exception" : ""}`}>{value}</div>
      {note ? <div className="mt-1 text-xs text-muted">{note}</div> : null}
    </div>
  );
}

export function KpiRow({ children }: { children: ReactNode }) {
  return <div className="mb-8 grid grid-cols-2 gap-3 lg:grid-cols-4">{children}</div>;
}

/** A share bar: one segment per value, in spectrum order. */
export function ShareBar({ parts, label }: { parts: { label: string; share: number; color: string }[]; label: string }) {
  return (
    <div>
      <div role="img" aria-label={label} className="flex h-2.5 overflow-hidden rounded-full bg-sand">
        {parts.map((part) => (
          <span key={part.label} style={{ width: `${part.share / 100}%`, background: part.color }} />
        ))}
      </div>
      <ul className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted">
        {parts.map((part) => (
          <li key={part.label} className="flex items-center gap-1.5">
            <span aria-hidden="true" className="size-2 rounded-full" style={{ background: part.color }} />
            {part.label} <span className="num text-ink">{formatShare(part.share)}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

/** A horizontal bar for one row, scaled to the largest row. */
export function RowBar({ share, color = "var(--color-spectrum-6)" }: { share: number; color?: string }) {
  return (
    <span className="block h-1.5 w-full min-w-16 overflow-hidden rounded-full bg-sand" aria-hidden="true">
      <span className="block h-full rounded-full" style={{ width: `${Math.max(0, Math.min(100, share / 100))}%`, background: color }} />
    </span>
  );
}

export function Empty({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <div className="rounded-lg border border-dashed border-line-strong px-6 py-10 text-center">
      <p className="text-sm font-medium">{title}</p>
      {children ? <div className="mt-1 text-sm text-muted">{children}</div> : null}
    </div>
  );
}

export function Loading() {
  return (
    <div className="space-y-2" aria-busy="true" aria-label="Loading">
      {[0, 1, 2].map((i) => (
        <div key={i} className="h-9 animate-pulse rounded-md bg-sand" />
      ))}
    </div>
  );
}

export function ErrorBox({ error }: { error: unknown }) {
  const message = error instanceof Error ? error.message : "Something went wrong.";
  useEffect(() => {
    additions.onErrorShown(message);
  }, [message]);
  return (
    <div role="alert" className="rounded-lg border border-exception/30 bg-exception-tint px-4 py-3 text-sm text-exception">
      {message}
    </div>
  );
}

export function Note({ children, tone = "accent" }: { children: ReactNode; tone?: "accent" | "exception" | "posted" }) {
  const styles = { accent: "bg-accent-tint text-ink", exception: "bg-exception-tint text-ink", posted: "bg-posted-tint text-ink" };
  const bar = { accent: "bg-accent", exception: "bg-exception", posted: "bg-posted" };
  return (
    <div className={`relative mb-6 overflow-hidden rounded-md py-2.5 pl-4 pr-3 text-sm ${styles[tone]}`}>
      <span aria-hidden="true" className={`absolute inset-y-0 left-0 w-1 ${bar[tone]}`} />
      {children}
    </div>
  );
}
