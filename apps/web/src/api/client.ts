import createClient from "openapi-fetch";
import type { components, paths } from "./generated";
import { additions } from "@/lib/additions";
import { recordRequest } from "@/lib/diagnostics";

// One client for the whole app. Endpoints in the service's OpenAPI document
// are called through `typed`, whose types are generated from it in CI, so a
// drift fails the build (ADR 0009). Endpoints the document does not list yet,
// or lists with a fault, go through `call` with the types in `contract.ts`;
// each moves to `typed` once the service publishes it.

export const API_PREFIX = "/api/v1";

export type ErrorBody = components["schemas"]["ErrorBody"];

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

export const typed = createClient<paths>({ credentials: "same-origin" });
typed.use({
  onResponse({ response }) {
    answered(response);
  },
});

/** Every answer from the service passes here, also to the build's additions. */
function answered(response: Response): void {
  recordRequest(response);
  additions.onResponse(response);
}

type Query = Record<string, string | number | boolean | undefined>;

export interface CallOptions {
  query?: Query;
  body?: unknown;
  /** Send the body as-is with this content type, such as CSV text. */
  contentType?: string;
}

function url(path: string, query?: Query): string {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(query ?? {})) {
    if (value !== undefined && value !== "") params.set(key, String(value));
  }
  const search = params.toString();
  return `${API_PREFIX}${path}${search ? `?${search}` : ""}`;
}

async function errorFrom(response: Response): Promise<ApiError> {
  try {
    const body = (await response.json()) as Partial<ErrorBody>;
    return new ApiError(response.status, body.error ?? "error", body.message ?? response.statusText);
  } catch {
    return new ApiError(response.status, "error", response.statusText || "The service did not answer.");
  }
}

export async function call<T>(method: string, path: string, options: CallOptions = {}): Promise<T> {
  const headers: Record<string, string> = { accept: "application/json" };
  let body: BodyInit | undefined;
  if (options.body !== undefined) {
    if (options.contentType) {
      headers["content-type"] = options.contentType;
      body = options.body as BodyInit;
    } else {
      headers["content-type"] = "application/json";
      body = JSON.stringify(options.body);
    }
  }
  const response = await fetch(url(path, options.query), {
    method,
    headers,
    credentials: "same-origin",
    ...(body === undefined ? {} : { body }),
  });
  answered(response);
  if (!response.ok) throw await errorFrom(response);
  if (response.status === 204) return undefined as T;
  const type = response.headers.get("content-type") ?? "";
  if (type.includes("application/json")) return (await response.json()) as T;
  return (await response.text()) as T;
}

/** Fetches a file, such as a CSV or the raw export zip, as a Blob. */
export async function download(path: string, query?: Query): Promise<Blob> {
  const response = await fetch(url(path, query), { credentials: "same-origin" });
  answered(response);
  if (!response.ok) throw await errorFrom(response);
  return response.blob();
}

/** The data of a `typed` call, or an ApiError with the service's own message. */
export async function ok<T>(request: Promise<{ data?: T; error?: unknown; response: Response }>): Promise<T> {
  const { data, error, response } = await request;
  if (!response.ok) {
    const body = (error ?? {}) as Partial<ErrorBody>;
    throw new ApiError(response.status, body.error ?? "error", body.message ?? (response.statusText || "The service did not answer."));
  }
  return data as T;
}
