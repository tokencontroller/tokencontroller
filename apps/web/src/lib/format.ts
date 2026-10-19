import type { Tags } from "@/api/contract";

export function formatDate(iso: string): string {
  return new Date(iso.length === 10 ? `${iso}T00:00:00Z` : iso).toLocaleDateString("en-GB", { day: "numeric", month: "short", year: "numeric", timeZone: "UTC" });
}

export function formatDay(iso: string): string {
  return new Date(iso.length === 10 ? `${iso}T00:00:00Z` : iso).toLocaleDateString("en-GB", { day: "numeric", month: "short", timeZone: "UTC" });
}

export function formatTime(iso: string): string {
  return new Date(iso).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit", timeZone: "UTC" });
}

export function formatDateTime(iso: string): string {
  return `${formatDate(iso)} ${formatTime(iso)} UTC`;
}

/** A date range as text, such as `1 Mar – 31 Mar 2026`. */
export function formatRange(from: string, to: string): string {
  return `${formatDay(from)} – ${formatDate(to)}`;
}

/** Token counts in millions or thousands, such as `6.2M`. */
export function formatTokens(count: number): string {
  if (count >= 1_000_000) return `${(count / 1_000_000).toFixed(1)}M`;
  if (count >= 1_000) return `${String(Math.round(count / 1_000))}k`;
  return String(count);
}

/** Tags as `name: value` pairs, built-in ones first. */
export function tagList(tags: Tags): [string, string][] {
  const order = ["feature", "project", "client", "person"];
  return Object.entries(tags).sort(([a], [b]) => {
    const ia = order.indexOf(a);
    const ib = order.indexOf(b);
    return (ia === -1 ? 99 : ia) - (ib === -1 ? 99 : ib) || a.localeCompare(b);
  });
}

/** A filter as text for URLs and the API: `client:acme,team:web`. */
export function filterToText(filter: Tags): string {
  return Object.entries(filter)
    .map(([name, value]) => `${name}:${value}`)
    .join(",");
}

export function filterFromText(text: string | undefined): Tags {
  const filter: Tags = {};
  for (const part of (text ?? "").split(",")) {
    const at = part.indexOf(":");
    if (at > 0) filter[part.slice(0, at).trim()] = part.slice(at + 1).trim();
  }
  return filter;
}

export function describeFilter(filter: Tags): string {
  const parts = Object.entries(filter).map(([name, value]) => `${name}: ${value}`);
  return parts.length ? parts.join(" and ") : "Whole organization";
}

/** A person's name, or their email until they have one. */
export function displayName(person: { name: string; email: string }): string {
  return person.name.trim() || person.email;
}

export function initials(name: string): string {
  return name
    .split(/\s+/)
    .map((part) => part[0] ?? "")
    .join("")
    .slice(0, 2)
    .toUpperCase();
}

/** A fraction such as `0.6` as `60 %`. */
export function formatFraction(fraction: string): string {
  const value = Number(fraction) * 100;
  return `${String(Math.round(value * 100) / 100)} %`;
}

/** The label of a split, in the docs' words. */
export const labelText: Record<string, string> = {
  direct: "Direct",
  project_overhead: "Project overhead",
  organization_overhead: "Organization overhead",
  "project overhead": "Project overhead",
  "organization overhead": "Organization overhead",
};

/** The deviation label from the classifier. */
export const deviationLabelText: Record<string, string> = { scope: "scope", trouble: "trouble", estimate_itself: "estimate itself" };

/** The first and last day of the month before the one `day` is in. */
export function lastMonth(day: string): { from: string; to: string } {
  const [y = 1970, m = 1] = day.split("-").map(Number);
  const first = new Date(Date.UTC(y, m - 2, 1));
  const last = new Date(Date.UTC(y, m - 1, 0));
  return { from: first.toISOString().slice(0, 10), to: last.toISOString().slice(0, 10) };
}
