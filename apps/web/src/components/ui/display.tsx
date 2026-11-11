import type { Money } from "@/api/contract";
import { formatMoney, formatShare } from "@/lib/money";
import { Popover } from "@base-ui/react/popover";
import { Children, useEffect, type ReactNode } from "react";
import { additions } from "@/lib/additions";

/** An amount, or a share of a total in basis points. `null` shows why there is none. */
export function MoneyText({ value, signed = false, share, missing = "not priced yet" }: { value: Money | null; signed?: boolean; share?: number | undefined; missing?: string }) {
  if (share !== undefined) return <span className="num">{formatShare(share)}</span>;
  if (value === null) return <span className="num text-exception">{missing}</span>;
  return <span className="num">{formatMoney(value, { signed })}</span>;
}

/**
 * The top of every screen: the title, at most one short line under it, and the
 * screen's one primary action on the right (docs: design system).
 */
export function PageHeader({ title, lead, actions, meta }: { title: string; lead?: ReactNode; actions?: ReactNode; meta?: ReactNode }) {
  return (
    <header className="mb-8 flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
      <div className="min-w-0">
        <h1 className="text-title font-semibold text-ink">{title}</h1>
        {lead || meta ? (
          <p className="mt-1 flex max-w-2xl flex-wrap items-baseline gap-x-2 text-small text-muted">
            {lead ? <span>{lead}</span> : null}
            {meta ? <span>{meta}</span> : null}
          </p>
        ) : null}
      </div>
      {actions ? <div className="flex flex-wrap items-center gap-2">{actions}</div> : null}
    </header>
  );
}

/** A quiet link in a card header, such as to the screen that holds the details. Use it with TanStack's Link: className={linkClass}. */
export const linkClass = "text-small font-medium text-accent hover:underline";

/** Help text moved out of the way: a small (i) that opens it on click or tap. */
export function Hint({ label, children }: { label: string; children: ReactNode }) {
  return (
    <Popover.Root>
      <Popover.Trigger
        aria-label={label}
        className="inline-flex size-5 shrink-0 items-center justify-center rounded-full text-muted hover:bg-hover hover:text-ink"
      >
        <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="6.25" fill="none" stroke="currentColor" strokeWidth="1.3" /><path d="M8 7.2v3.6" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" /><circle cx="8" cy="5.1" r="0.85" fill="currentColor" /></svg>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Positioner sideOffset={6} className="z-50">
          <Popover.Popup className="max-w-72 rounded-md bg-surface px-3 py-2 text-small text-ink-soft shadow-pop">{children}</Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  );
}

/** A plain card, for a toolbar or anything without a title. */
export function Card({ children, className = "" }: { children: ReactNode; className?: string }) {
  return <div className={`mb-6 rounded-lg border border-line bg-surface p-4 shadow-card max-sm:-mx-4 max-sm:rounded-none max-sm:border-x-0 ${className}`}>{children}</div>;
}

/** A card runs to the screen's edges on a phone, so tables get the width. */
const cardClass = "rounded-lg border border-line bg-surface shadow-card max-sm:-mx-4 max-sm:rounded-none max-sm:border-x-0";

/**
 * A card: one thing on the screen, with its title, an optional hint and its
 * actions in one row. `flush` lets a table run to the card's edges. An
 * explanation goes in `hint`, never in a line under the title.
 */
export function Section({
  id,
  title,
  actions,
  children,
  hint,
  badge,
  flush = false,
  footer,
}: {
  id?: string;
  title: string;
  /** A short status next to the title, such as "built in" or "not connected". */
  badge?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  hint?: ReactNode;
  flush?: boolean;
  footer?: ReactNode;
}) {
  const pad = "px-5";
  return (
    <section id={id} className={`relative mb-6 scroll-mt-20 ${cardClass} ${flush && !actions ? "[&_.tc-csv]:absolute [&_.tc-csv]:right-3 [&_.tc-csv]:top-3 [&_.tc-csv]:mb-0" : "[&_.tc-csv]:px-3"}`}>
      <div className={`flex flex-wrap items-center justify-between gap-x-3 gap-y-2 ${pad} pt-4 ${flush ? "pb-3" : "pb-1"}`}>
        <div className="min-w-0">
          <div className="flex items-center gap-1.5">
            <h2 className="text-heading font-semibold text-ink">{title}</h2>
            {hint ? <Hint label={`About ${title}`}>{hint}</Hint> : null}
            {badge ? <span className="ml-1">{badge}</span> : null}
          </div>
        </div>
        {actions ? <div className="flex flex-wrap items-center gap-2">{actions}</div> : null}
      </div>
      <div
        className={
          flush
            ? "[&_.tc-table-wrap]:mx-0 sm:[&_td:first-child]:pl-5 sm:[&_td:last-child]:pr-5 sm:[&_th:first-child]:pl-5 sm:[&_th:last-child]:pr-5"
            : `${pad} pb-5 pt-2`
        }
      >
        {children}
      </div>
      {footer ? <div className={`flex flex-wrap items-center justify-end gap-2 border-t border-line ${pad} py-3 rounded-b-lg bg-hover/60`}>{footer}</div> : null}
    </section>
  );
}

/** One figure: a short label, the number large, an optional note under it. */
export function Kpi({ label, value, note, tone, hint, hero = false }: { label: string; value: ReactNode; note?: ReactNode; tone?: "posted" | "exception" | undefined; hint?: ReactNode; hero?: boolean }) {
  return (
    <div className={`min-w-0 p-5 ${hero ? "max-lg:col-span-2" : ""}`}>
      <div className="flex items-center gap-1 text-small font-medium text-muted">
        {label}
        {hint ? <Hint label={`About ${label}`}>{hint}</Hint> : null}
      </div>
      <div className={`mt-1 ${hero ? "text-[36px] leading-[44px] tracking-[-0.025em] font-semibold" : "text-[22px] leading-8 tracking-[-0.01em] font-medium"} num !text-left ${tone === "posted" ? "text-posted" : tone === "exception" ? "text-exception" : "text-ink"}`}>{value}</div>
      {note ? <div className="mt-1 text-caption text-muted">{note}</div> : null}
    </div>
  );
}

/** Figures side by side in one card, split by hairlines. */
export function KpiRow({ children }: { children: ReactNode }) {
  // Two to four figures share the row; the first, the hero, is wider.
  const count = Children.toArray(children).filter(Boolean).length;
  // Below lg, two figures stack; three or four go hero first, then two by two.
  const columns = count <= 2 ? "grid-cols-1 lg:grid-cols-[1.5fr_1fr] [&>*]:col-span-1!" : count === 3 ? "grid-cols-2 lg:grid-cols-[1.5fr_1fr_1fr]" : "grid-cols-2 lg:grid-cols-[1.5fr_1fr_1fr_1fr]";
  return (
    <div className={`mb-6 grid gap-px overflow-hidden rounded-lg border border-line bg-line shadow-card ${columns} max-sm:-mx-4 max-sm:rounded-none max-sm:border-x-0 [&>*]:bg-surface max-lg:[&>*:nth-child(4):last-child]:col-span-2`}>
      {children}
    </div>
  );
}

/** A figure in a row of figures inside a card: label above, value under it. */
export function Figure({ label, value, tone, size = "md" }: { label: ReactNode; value: ReactNode; tone?: "posted" | "exception" | undefined; size?: "md" | "lg" }) {
  return (
    <div className="min-w-0">
      <div className="text-small text-muted">{label}</div>
      <div className={`mt-0.5 num !text-left font-medium ${size === "lg" ? "text-figure" : "text-[20px] leading-7 tracking-[-0.01em]"} ${tone === "posted" ? "text-posted" : tone === "exception" ? "text-exception" : "text-ink"}`}>{value}</div>
    </div>
  );
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
      <ul className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-caption text-muted">
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

/** Nothing to show yet: what is missing, in a line, and the one action that fills it. */
export function Empty({ title, children, action }: { title: string; children?: ReactNode; action?: ReactNode }) {
  return (
    <div className="flex flex-col items-center rounded-lg border border-dashed border-line-strong px-6 py-10 text-center">
      <p className="text-body font-medium text-ink">{title}</p>
      {children ? <div className="mt-1 max-w-sm text-small text-muted">{children}</div> : null}
      {action ? <div className="mt-4">{action}</div> : null}
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
    <div role="alert" className="rounded-lg border border-exception/30 bg-exception-tint px-4 py-3 text-body text-exception">
      {message}
    </div>
  );
}

export function Note({ children, tone = "accent" }: { children: ReactNode; tone?: "accent" | "exception" | "posted" }) {
  const styles = { accent: "bg-accent-tint text-ink", exception: "bg-exception-tint text-ink", posted: "bg-posted-tint text-ink" };
  const bar = { accent: "bg-accent", exception: "bg-exception", posted: "bg-posted" };
  return (
    <div className={`relative mb-6 overflow-hidden rounded-md py-2.5 pl-4 pr-3 text-body ${styles[tone]}`}>
      <span aria-hidden="true" className={`absolute inset-y-0 left-0 w-1 ${bar[tone]}`} />
      {children}
    </div>
  );
}
