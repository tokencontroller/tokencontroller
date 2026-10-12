import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import type { Plugin } from "vite";
import { defineConfig } from "vitest/config";

const mock = Boolean(process.env.VITE_API_MOCK);
const workerFile = fileURLToPath(new URL("./mock-public/mockServiceWorker.js", import.meta.url));

// The mock service worker lives outside public/ so it never ships in the
// real build. Only mock builds and the mock dev server serve it.
function mockWorker(): Plugin {
  return {
    name: "tc-mock-worker",
    configureServer(server) {
      if (!mock) return;
      server.middlewares.use("/mockServiceWorker.js", (_req, res) => {
        res.setHeader("Content-Type", "text/javascript");
        res.end(readFileSync(workerFile));
      });
    },
    generateBundle() {
      if (!mock) return;
      this.emitFile({ type: "asset", fileName: "mockServiceWorker.js", source: readFileSync(workerFile, "utf8") });
    },
  };
}

// The service serves the built files from the same image (ADR 0002), so the
// app always talks to /api/v1 on its own origin. In development, the API is
// either proxied to a local service or answered by the mocks (VITE_API_MOCK=1).
export default defineConfig({
  plugins: [react(), tailwindcss(), mockWorker()],
  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
  },
  server: {
    proxy: mock ? {} : { "/api": process.env.TC_API_URL ?? "http://localhost:8080" },
  },
  build: { sourcemap: true },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
