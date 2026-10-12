import { QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { ToastProvider } from "@/components/ui/toast";
import { queryClient } from "@/lib/queries";
import { router } from "@/router";
import "./styles.css";

async function start() {
  // Mock mode answers the API in the browser (development, journeys, screenshots).
  if (import.meta.env.VITE_API_MOCK) {
    const { startMocks } = await import("@/mocks/browser");
    await startMocks();
  }
  const root = document.getElementById("root");
  if (!root) throw new Error("index.html has no #root");
  createRoot(root).render(
    <StrictMode>
      <QueryClientProvider client={queryClient}>
        <ToastProvider>
          <RouterProvider router={router} />
        </ToastProvider>
      </QueryClientProvider>
    </StrictMode>,
  );
}

void start();
