import { Tabs as BaseTabs } from "@base-ui/react/tabs";
import type { ReactNode } from "react";

export function Tabs({ value, onValueChange, tabs, children }: { value: string; onValueChange: (value: string) => void; tabs: { value: string; label: string }[]; children: ReactNode }) {
  return (
    <BaseTabs.Root value={value} onValueChange={(v) => onValueChange(String(v))}>
      <div className="-mx-4 overflow-x-auto px-4 sm:mx-0 sm:px-0">
        <BaseTabs.List className="relative flex gap-1 border-b border-line">
          {tabs.map((tab) => (
            <BaseTabs.Tab
              key={tab.value}
              value={tab.value}
              className="relative -mb-px whitespace-nowrap border-b-2 border-transparent px-3 py-2 text-body font-medium text-muted hover:text-ink data-[active]:border-ink data-[active]:text-ink"
            >
              {tab.label}
            </BaseTabs.Tab>
          ))}
        </BaseTabs.List>
      </div>
      <div className="pt-5">{children}</div>
    </BaseTabs.Root>
  );
}

export const TabPanel = BaseTabs.Panel;

/**
 * A small switch between two or three views of one list, such as open and
 * submitted sessions. It sits in a card header.
 */
export function Segmented<V extends string>({ label, value, onValueChange, options }: { label: string; value: V; onValueChange: (value: V) => void; options: { value: V; label: string; count?: number }[] }) {
  return (
    <div role="group" aria-label={label} className="inline-flex rounded-md bg-hover p-0.5 ring-1 ring-inset ring-line">
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          aria-pressed={value === o.value}
          onClick={() => onValueChange(o.value)}
          className={`inline-flex h-7 items-center gap-1.5 rounded-[5px] px-2.5 text-small font-medium ${value === o.value ? "bg-surface text-ink shadow-chip" : "text-muted hover:text-ink"}`}
        >
          {o.label}
          {o.count !== undefined ? <span className="num text-caption text-muted">{o.count}</span> : null}
        </button>
      ))}
    </div>
  );
}
