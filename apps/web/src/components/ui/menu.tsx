import { Menu as BaseMenu } from "@base-ui/react/menu";
import type { ReactNode } from "react";

export function Menu({ label, items, trigger }: { label: string; trigger?: ReactNode; items: { label: string; onClick: () => void; danger?: boolean }[] }) {
  return (
    <BaseMenu.Root>
      <BaseMenu.Trigger aria-label={label} className="inline-flex size-8 items-center justify-center rounded-md text-muted hover:bg-sand hover:text-ink">
        {trigger ?? (
          <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true"><circle cx="3.5" cy="8" r="1.3" fill="currentColor" /><circle cx="8" cy="8" r="1.3" fill="currentColor" /><circle cx="12.5" cy="8" r="1.3" fill="currentColor" /></svg>
        )}
      </BaseMenu.Trigger>
      <BaseMenu.Portal>
        <BaseMenu.Positioner sideOffset={4} align="end" className="z-50">
          <BaseMenu.Popup className="min-w-44 rounded-md bg-page p-1 shadow-pop">
            {items.map((item) => (
              <BaseMenu.Item
                key={item.label}
                onClick={item.onClick}
                className={`flex cursor-default rounded-sm px-2.5 py-1.5 text-sm outline-none data-[highlighted]:bg-sand ${item.danger ? "text-exception" : "text-ink"}`}
              >
                {item.label}
              </BaseMenu.Item>
            ))}
          </BaseMenu.Popup>
        </BaseMenu.Positioner>
      </BaseMenu.Portal>
    </BaseMenu.Root>
  );
}
