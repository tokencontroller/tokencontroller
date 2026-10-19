import { Checkbox as BaseCheckbox } from "@base-ui/react/checkbox";

export function Checkbox({ checked, onCheckedChange, label, indeterminate = false }: { checked: boolean; onCheckedChange: (checked: boolean) => void; label: string; indeterminate?: boolean }) {
  return (
    <BaseCheckbox.Root
      checked={checked}
      indeterminate={indeterminate}
      onCheckedChange={onCheckedChange}
      aria-label={label}
      className="flex size-4 shrink-0 items-center justify-center rounded-sm border border-line-strong bg-page data-[checked]:border-ink data-[checked]:bg-ink data-[indeterminate]:border-ink data-[indeterminate]:bg-ink"
    >
      <BaseCheckbox.Indicator className="text-on-ink">
        {indeterminate ? (
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M2 5h6" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" /></svg>
        ) : (
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true"><path d="M1.5 5.2l2.2 2.2L8.5 2.6" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" /></svg>
        )}
      </BaseCheckbox.Indicator>
    </BaseCheckbox.Root>
  );
}

/** A checkbox with its label shown next to it. */
export function CheckField({ checked, onCheckedChange, label }: { checked: boolean; onCheckedChange: (checked: boolean) => void; label: string }) {
  return (
    <span className="inline-flex items-center gap-2 text-sm">
      <Checkbox checked={checked} onCheckedChange={onCheckedChange} label={label} />
      <span aria-hidden="true">{label}</span>
    </span>
  );
}
