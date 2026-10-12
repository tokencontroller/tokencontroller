import { Switch as BaseSwitch } from "@base-ui/react/switch";

export function Switch({ checked, onCheckedChange, label, disabled = false }: { checked: boolean; onCheckedChange: (checked: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <BaseSwitch.Root
      checked={checked}
      onCheckedChange={onCheckedChange}
      disabled={disabled}
      aria-label={label}
      className="relative inline-flex h-5 w-9 shrink-0 cursor-pointer rounded-full bg-line-strong p-0.5 transition-colors data-[checked]:bg-posted data-[disabled]:opacity-50"
    >
      <BaseSwitch.Thumb className="block size-4 rounded-full bg-page shadow-chip transition-transform data-[checked]:translate-x-4" />
    </BaseSwitch.Root>
  );
}
