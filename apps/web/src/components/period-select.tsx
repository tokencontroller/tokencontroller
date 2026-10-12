import { usePeriods } from "@/lib/queries";
import { addMonths, formatPeriod } from "@/lib/format";
import { thisPeriod } from "@/lib/today";
import { Select } from "./ui/form";

/** The last twelve months, newest first: what to pick from until the periods arrive. */
function lastMonths(): { period: string; closed: boolean }[] {
  return Array.from({ length: 12 }, (_, i) => ({ period: addMonths(thisPeriod(), -i), closed: false }));
}

export function PeriodSelect({ value, onChange }: { value: string; onChange: (period: string) => void }) {
  const { data } = usePeriods();
  const listed = data ?? lastMonths();
  const periods = listed.some((p) => p.period === value) ? listed : [...listed, { period: value, closed: false }].sort((a, b) => b.period.localeCompare(a.period));
  return (
    <Select aria-label="Period" value={value} onChange={(e) => onChange(e.target.value)} className="w-auto min-w-44 max-w-full">
      {periods.map((p) => (
        <option key={p.period} value={p.period}>
          {formatPeriod(p.period)}
          {p.closed ? " · closed" : ""}
        </option>
      ))}
    </Select>
  );
}
