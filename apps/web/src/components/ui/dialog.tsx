import { Dialog as BaseDialog } from "@base-ui/react/dialog";
import type { ReactNode } from "react";

export function Dialog({
  open,
  onOpenChange,
  title,
  description,
  children,
  wide = false,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description?: ReactNode;
  children: ReactNode;
  wide?: boolean;
}) {
  return (
    <BaseDialog.Root open={open} onOpenChange={onOpenChange}>
      <BaseDialog.Portal>
        <BaseDialog.Backdrop className="fixed inset-0 z-40 bg-ink/25 transition-opacity data-[ending-style]:opacity-0 data-[starting-style]:opacity-0" />
        <BaseDialog.Viewport className="fixed inset-0 z-50 flex items-end justify-center sm:items-center sm:p-6">
          <BaseDialog.Popup
            className={`max-h-[92dvh] w-full overflow-y-auto rounded-t-lg bg-page p-5 shadow-pop transition-all data-[ending-style]:translate-y-2 data-[ending-style]:opacity-0 data-[starting-style]:translate-y-2 data-[starting-style]:opacity-0 sm:rounded-lg sm:p-6 ${wide ? "sm:max-w-3xl" : "sm:max-w-lg"}`}
          >
            <div className="mb-4 flex items-start justify-between gap-4">
              <div>
                <BaseDialog.Title className="text-base font-semibold">{title}</BaseDialog.Title>
                {description ? <BaseDialog.Description className="mt-1 text-sm text-muted">{description}</BaseDialog.Description> : null}
              </div>
              <BaseDialog.Close aria-label="Close" className="-mr-1 -mt-1 rounded-md p-1.5 text-muted hover:bg-sand hover:text-ink">
                <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true"><path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" /></svg>
              </BaseDialog.Close>
            </div>
            {children}
          </BaseDialog.Popup>
        </BaseDialog.Viewport>
      </BaseDialog.Portal>
    </BaseDialog.Root>
  );
}
