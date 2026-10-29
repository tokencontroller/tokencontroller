import { useState, type ReactNode } from "react";
import type { Filter } from "@/api/contract";
import { MoneyText } from "@/components/ui/display";
import { Field, Input } from "@/components/ui/form";
import { filterFromText, filterToText, formatDateTime, lastMonth } from "@/lib/format";
import { fromDecimal } from "@/lib/money";
import { today } from "@/lib/today";

/** An amount the service sent as a decimal string, in its currency. `null` shows why there is none. */
export function Amount({ value, currency = "USD", signed = false, missing }: { value: string | null | undefined; currency?: string | undefined; signed?: boolean; missing?: string }) {
  return <MoneyText value={value === null || value === undefined ? null : fromDecimal(value, currency)} signed={signed} {...(missing ? { missing } : {})} />;
}

/** A date range and an as-of date, as every report takes (docs: Reading reports). */
export interface RangeState {
  from: string;
  to: string;
  /** Empty: as of now. A day reads the log up to the start of the next day. */
  asOf: string;
}

export function useRange(initial?: Partial<RangeState>): [RangeState, (next: RangeState) => void] {
  const month = lastMonth(today());
  return useState<RangeState>({ from: initial?.from ?? month.from, to: initial?.to ?? month.to, asOf: initial?.asOf ?? "" });
}

/** The as-of moment for the API: the end of the day given, or now. */
export function asOfParam(asOf: string): string | undefined {
  return asOf ? `${asOf}T23:59:59Z` : undefined;
}

export function RangeControls({ value, onChange, children }: { value: RangeState; onChange: (next: RangeState) => void; children?: ReactNode }) {
  return (
    <div className="mb-6 grid grid-cols-2 gap-3 sm:flex sm:flex-wrap sm:items-end">
      <Field label="From" className="sm:w-40">
        <Input type="date" value={value.from} onChange={(e) => onChange({ ...value, from: e.target.value })} />
      </Field>
      <Field label="To" className="sm:w-40">
        <Input type="date" value={value.to} onChange={(e) => onChange({ ...value, to: e.target.value })} />
      </Field>
      <Field label="As of" className="sm:w-40" hint="Empty: as of now">
        <Input type="date" value={value.asOf} onChange={(e) => onChange({ ...value, asOf: e.target.value })} />
      </Field>
      {children}
    </div>
  );
}

/** A filter typed as `client:acme, team:web`. Empty is the whole organization. */
export function FilterField({ value, onChange, label = "Filter" }: { value: Filter; onChange: (filter: Filter) => void; label?: string }) {
  const [text, setText] = useState(filterToText(value));
  return (
    <Field label={label} className="col-span-2 sm:w-64" hint="Tags and attributes, such as client:acme, team:web">
      <Input
        value={text}
        placeholder="Whole organization"
        onChange={(e) => setText(e.target.value)}
        onBlur={() => onChange(filterFromText(text.replace(/\s+/g, "")))}
        onKeyDown={(e) => {
          if (e.key === "Enter") onChange(filterFromText(text.replace(/\s+/g, "")));
        }}
      />
    </Field>
  );
}

/** "As of …" under a figure, as every report shows it. */
export function AsOf({ at }: { at: string }) {
  return <span className="text-xs text-muted">as of {formatDateTime(at)}</span>;
}
