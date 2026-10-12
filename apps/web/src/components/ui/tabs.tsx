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
              className="relative -mb-px whitespace-nowrap border-b-2 border-transparent px-3 py-2 text-sm font-medium text-muted hover:text-ink data-[selected]:border-ink data-[selected]:text-ink"
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
