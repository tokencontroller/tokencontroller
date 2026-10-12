import type { PeriodId, Tags, TokenCounts } from "@/api/contract";

export function formatPeriod(period: PeriodId): string {
  const [year, month] = period.split("-").map(Number);
  return new Date(Date.UTC(year ?? 1970, (month ?? 1) - 1, 1)).toLocaleDateString("en-US", {
    month: "long",
    year: "numeric",
    timeZone: "UTC",
  });
}

export function formatDate(iso: string): string {
  return new Date(iso).toLocaleDateString("en-GB", { day: "numeric", month: "short", year: "numeric", timeZone: "UTC" });
}

export function formatDay(iso: string): string {
  return new Date(iso).toLocaleDateString("en-GB", { day: "numeric", month: "short", timeZone: "UTC" });
}

export function formatTime(iso: string): string {
  return new Date(iso).toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit", timeZone: "UTC" });
}

export function formatDateTime(iso: string): string {
  return `${formatDate(iso)} ${formatTime(iso)} UTC`;
}

/** Token counts in millions or thousands, such as `6.2M`. */
export function formatTokens(count: number): string {
  if (count >= 1_000_000) return `${(count / 1_000_000).toFixed(1)}M`;
  if (count >= 1_000) return `${Math.round(count / 1_000)}k`;
  return String(count);
}

export function totalTokens(tokens: TokenCounts): number {
  return tokens.input + tokens.output + tokens.cache_write + tokens.cache_read;
}

/** Tags as `name: value` pairs, built-in ones first. */
export function tagList(tags: Tags): [string, string][] {
  const order = ["ticket", "project", "person"];
  return Object.entries(tags).sort(([a], [b]) => {
    const ia = order.indexOf(a);
    const ib = order.indexOf(b);
    return (ia === -1 ? 99 : ia) - (ib === -1 ? 99 : ib) || a.localeCompare(b);
  });
}

/** A filter as text for URLs and the reporting API: `client:acme,team:web`. */
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

export function initials(name: string): string {
  return name
    .split(/\s+/)
    .map((part) => part[0] ?? "")
    .join("")
    .slice(0, 2)
    .toUpperCase();
}

export function addMonths(period: PeriodId, delta: number): PeriodId {
  const [year = 1970, month = 1] = period.split("-").map(Number);
  const index = year * 12 + (month - 1) + delta;
  return `${Math.floor(index / 12)}-${String((index % 12) + 1).padStart(2, "0")}`;
}
