import { useEffect, type ComponentProps, type ReactNode } from "react";
import { additions } from "@/lib/additions";

const control =
  "h-9 rounded-md border border-line-strong bg-surface px-3 text-body text-ink shadow-[inset_0_1px_1px_rgb(28_32_48/0.04)] placeholder:text-muted focus:border-accent focus:outline-none focus:ring-3 focus:ring-accent-tint disabled:opacity-60";

/** Full width unless the caller sets a width of its own. */
const width = (className: string) => (/(^|\s)w-/.test(className) ? "" : "w-full");

export function Field({ label, hint, children, className = "" }: { label: string; hint?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <label className={`block ${className}`}>
      <span className="mb-1.5 block text-small font-medium text-ink-soft">{label}</span>
      {children}
      {hint ? <span className="mt-1 block text-caption text-muted">{hint}</span> : null}
    </label>
  );
}

/**
 * A settings row: what it is on the left, with one line under it, and the
 * control on the right. Stacks on a phone.
 */
export function SettingRow({ label, description, children, htmlFor }: { label: string; description?: ReactNode; children: ReactNode; htmlFor?: string }) {
  return (
    <div className="grid gap-2 border-b border-line py-4 last:border-b-0 sm:grid-cols-[minmax(0,1fr)_minmax(0,2fr)] sm:gap-8">
      <div className="min-w-0">
        {htmlFor ? <label htmlFor={htmlFor} className="text-body font-medium text-ink">{label}</label> : <div className="text-body font-medium text-ink">{label}</div>}
        {description ? <p className="mt-0.5 text-small text-muted">{description}</p> : null}
      </div>
      <div className="min-w-0 max-w-md sm:pt-0.5">{children}</div>
    </div>
  );
}

export function Input({ className = "", ...props }: ComponentProps<"input">) {
  return <input className={`${control} ${width(className)} ${className}`} {...props} />;
}

export function Textarea({ className = "", ...props }: ComponentProps<"textarea">) {
  return <textarea className={`${control} ${width(className)} h-auto min-h-20 py-2 ${className}`} {...props} />;
}

export function Select({ className = "", children, ...props }: ComponentProps<"select">) {
  return (
    <select className={`${control} ${width(className)} appearance-none bg-[length:16px] bg-[right_8px_center] bg-no-repeat pr-8 ${className}`} style={{ backgroundImage: "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 16 16'%3E%3Cpath d='M4 6l4 4 4-4' fill='none' stroke='%23646a7d' stroke-width='1.5'/%3E%3C/svg%3E\")" }} {...props}>
      {children}
    </select>
  );
}

export function FormError({ error }: { error: unknown }) {
  const message = !error ? null : error instanceof Error ? error.message : "Something went wrong.";
  useEffect(() => {
    if (!message) return;
    additions.onErrorShown(message);
  }, [message]);
  if (!message) return null;
  return (
    <p role="alert" className="rounded-md bg-exception-tint px-3 py-2 text-sm text-exception">
      {message}
    </p>
  );
}
