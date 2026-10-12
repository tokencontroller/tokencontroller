import type { ComponentProps, ReactNode } from "react";

const control =
  "h-9 rounded-md border border-line-strong bg-page px-2.5 text-sm text-ink placeholder:text-muted focus:border-accent focus:outline-none disabled:opacity-60";

/** Full width unless the caller sets a width of its own. */
const width = (className: string) => (/(^|\s)w-/.test(className) ? "" : "w-full");

export function Field({ label, hint, children, className = "" }: { label: string; hint?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <label className={`block ${className}`}>
      <span className="mb-1 block text-[13px] font-medium text-ink-soft">{label}</span>
      {children}
      {hint ? <span className="mt-1 block text-xs text-muted">{hint}</span> : null}
    </label>
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
  if (!error) return null;
  const message = error instanceof Error ? error.message : "Something went wrong.";
  return (
    <p role="alert" className="rounded-md bg-exception-tint px-3 py-2 text-sm text-exception">
      {message}
    </p>
  );
}
