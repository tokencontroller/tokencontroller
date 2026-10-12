import { Toast } from "@base-ui/react/toast";
import type { ReactNode } from "react";

export const toasts = Toast.createToastManager();

export function notify(title: string, description?: string): void {
  toasts.add({ title, ...(description ? { description } : {}) });
}

function Toasts() {
  const { toasts: list } = Toast.useToastManager();
  return list.map((toast) => (
    <Toast.Root key={toast.id} toast={toast} className="rounded-md bg-ink px-4 py-3 text-on-ink shadow-pop data-[ending-style]:opacity-0 data-[starting-style]:translate-y-2 data-[starting-style]:opacity-0 transition-all">
      <Toast.Content>
        <Toast.Title className="text-sm font-medium" />
        <Toast.Description className="mt-0.5 text-[13px] opacity-80" />
      </Toast.Content>
    </Toast.Root>
  ));
}

export function ToastProvider({ children }: { children: ReactNode }) {
  return (
    <Toast.Provider toastManager={toasts} timeout={4000}>
      {children}
      <Toast.Portal>
        <Toast.Viewport className="fixed bottom-4 left-1/2 z-[60] flex w-[calc(100%-2rem)] max-w-sm -translate-x-1/2 flex-col gap-2 sm:left-auto sm:right-4 sm:translate-x-0">
          <Toasts />
        </Toast.Viewport>
      </Toast.Portal>
    </Toast.Provider>
  );
}
