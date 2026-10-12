import type { CadenceKind } from "@/api/contract";
import { Input, Select } from "./ui/form";

/** A cadence as a form holds it: how often, and the days for a custom one. */
export interface CadenceChoice {
  kind: CadenceKind;
  every_days?: number | null;
}

export const cadenceKindText: Record<CadenceKind, string> = { weekly: "Weekly", every_two_weeks: "Every two weeks", monthly: "Monthly", custom: "Custom" };

export function describeCadence(cadence: CadenceChoice | null | undefined): string {
  if (!cadence) return "No cadence";
  return cadence.kind === "custom" ? `Every ${String(cadence.every_days ?? "?")} days` : cadenceKindText[cadence.kind];
}

export function CadenceSelect({ value, onChange }: { value: CadenceChoice | null; onChange: (cadence: CadenceChoice | null) => void }) {
  return (
    <div className="flex gap-2">
      <Select
        aria-label="Cadence"
        value={value?.kind ?? ""}
        onChange={(e) => {
          const kind = e.target.value as CadenceKind | "";
          onChange(kind ? { kind, every_days: kind === "custom" ? (value?.every_days ?? 10) : null } : null);
        }}
      >
        <option value="">No cadence</option>
        {(Object.keys(cadenceKindText) as CadenceKind[]).map((k) => (
          <option key={k} value={k}>
            {cadenceKindText[k]}
          </option>
        ))}
      </Select>
      {value?.kind === "custom" ? (
        <Input aria-label="Days" type="number" min={1} className="w-24" value={value.every_days ?? 10} onChange={(e) => onChange({ kind: "custom", every_days: Number(e.target.value) || 1 })} />
      ) : null}
    </div>
  );
}
