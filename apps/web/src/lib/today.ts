// The mocks show one fixed month, so screenshots never change. Everywhere
// else, today is today.
export function today(): string {
  return import.meta.env.VITE_API_MOCK ? "2026-10-12" : new Date().toISOString().slice(0, 10);
}

export function thisPeriod(): string {
  return today().slice(0, 7);
}
