import type { CostSource } from "@/api/contract";

// Small things the browser keeps between the sign-in page and the page the
// magic link opens. The link opens the service's own page, which sends the
// browser to the start page, so anything else the person asked for waits here.
// Storage can be missing or blocked; then the app simply forgets.

const NEXT = "tc.next";
const SIGN_UP = "tc.sign-up";
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

/** What someone picked at sign-up, applied once they are signed in. */
export interface SignUpChoices {
  cost_source: CostSource | null;
  reporting_currency: string;
  manages: "agents" | "people";
  /** Set once the cost source and currency went to the service. */
  applied?: boolean;
}

export function rememberSignUp(choices: SignUpChoices): void {
  write(SIGN_UP, choices);
}

export function readSignUp(): SignUpChoices | null {
  return read(SIGN_UP) as SignUpChoices | null;
}

export function forgetSignUp(): void {
  write(SIGN_UP, null);
}
