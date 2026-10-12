import { afterEach, beforeAll, describe, expect, test, vi } from "vitest";
import type { Additions } from "@/lib/additions";

// A build's additions see every answer from the service, whichever helper
// made the call, also the ones that fail.
describe("the API client", () => {
  const answers: Response[] = [];
  let client: typeof import("./client");
  let additions: Additions;

  beforeAll(async () => {
    // `typed` keeps the Request and fetch it finds when the module loads. A
    // page resolves `/api/v1/...` against its own address; Node needs one given.
    vi.stubGlobal(
      "Request",
      class extends Request {
        constructor(input: RequestInfo | URL, init?: RequestInit) {
          super(typeof input === "string" ? new URL(input, "http://localhost") : input, init);
        }
      },
    );
    vi.stubGlobal("fetch", (input: RequestInfo | URL) => {
      const path = input instanceof Request ? new URL(input.url).pathname : String(input);
      const status = path.endsWith("/missing") ? 404 : 200;
      const response = new Response(JSON.stringify(status === 404 ? { error: "not_found", message: "not found" } : { status: "ok" }), {
        status,
        headers: { "content-type": "application/json", "x-request-id": `req-${path}` },
      });
      return Promise.resolve(response);
    });
    // Both load only now, after the stubs: a build's slot may load the client
    // itself, through the endpoints it calls.
    additions = (await import("@/lib/additions")).additions;
    client = await import("./client");
  });

  afterEach(() => {
    vi.restoreAllMocks();
    answers.length = 0;
  });

  test("passes every answer of typed, call and download to the additions", async () => {
    vi.spyOn(additions, "onResponse").mockImplementation((response) => answers.push(response));
    await client.ok(client.typed.GET("/api/v1/health"));
    await client.call("GET", "/status");
    await client.download("/exports/raw");
    await expect(client.call("GET", "/missing")).rejects.toThrow("not found");
    expect(answers.map((r) => r.headers.get("x-request-id"))).toEqual(["req-/api/v1/health", "req-/api/v1/status", "req-/api/v1/exports/raw", "req-/api/v1/missing"]);
  });
});
