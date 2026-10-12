import { usePeriods } from "@/lib/queries";
import { formatPeriod } from "@/lib/format";
import { Select } from "./ui/form";

export function PeriodSelect({ value, onChange }: { value: string; onChange: (period: string) => void }) {
  const { data } = usePeriods();
  const periods = data ?? [{ id: value, status: "open" as const }];
  return (
    <Select aria-label="Period" value={value} onChange={(e) => onChange(e.target.value)} className="w-auto min-w-44 max-w-full">
      {periods.map((p) => (
        <option key={p.id} value={p.id}>
          {formatPeriod(p.id)}
          {p.status === "closed" ? " · closed" : ""}
        </option>
      ))}
    </Select>
  );
}
