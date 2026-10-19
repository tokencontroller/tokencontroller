// The mocks show the worked month of the docs, so screenshots never change:
// their today is 10 April 2026. Everywhere else, today is today.
export function today(): string {
  return import.meta.env.VITE_API_MOCK ? "2026-04-10" : new Date().toISOString().slice(0, 10);
}
