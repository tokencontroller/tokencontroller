import type { Manages } from "@/api/contract";

// Small things the browser keeps between the sign-in page and the page the
// magic link opens. The link opens the service's own page, which sends the
// browser to the start page, so anything else the person asked for waits here.
// Storage can be missing or blocked; then the app simply forgets.

const NEXT = "tc.next";
const FIRST_STEPS = "tc.first-steps";
/** As long as a sign-in link works. */
const LINK_LIFETIME = 15 * 60_000;

function read(key: string): unknown {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as unknown) : null;
  } catch {
    return null;
  }
}

function write(key: string, value: unknown): void {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Nothing to keep it in.
  }
}

/** Only a path on this site, so a stored page can never send someone elsewhere. */
export function safeNext(next: string | null | undefined): string {
  return next?.startsWith("/") && !next.startsWith("//") ? next : "/";
}

/** The page to open once signed in, such as /close/2026-09. */
export function rememberNext(path: string | undefined): void {
  write(NEXT, path && safeNext(path) !== "/" ? { path, at: Date.now() } : null);
}

/** The page asked for before sign-in, once, if the link was used in time. */
export function takeNext(): string | null {
  const stored = read(NEXT) as { path: string; at: number } | null;
  if (!stored) return null;
  write(NEXT, null);
  return Date.now() - stored.at < LINK_LIFETIME ? safeNext(stored.path) : null;
}

/**
 * The first steps to show on the start page until hidden, picked by what the
 * founder manages. A sign-up link lands on /?first_steps=people, in whichever
 * browser opens it; the service already applied the other sign-up answers.
 */
export function rememberFirstSteps(manages: Manages): void {
  write(FIRST_STEPS, manages);
}

export function readFirstSteps(): Manages | null {
  const stored = read(FIRST_STEPS);
  return stored === "agents" || stored === "people" ? stored : null;
}

export function forgetFirstSteps(): void {
  write(FIRST_STEPS, null);
}
