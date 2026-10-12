import type { CostKind, TokenEntryStatus, ValueLabel } from "@/api/contract";
import type { ReactNode } from "react";

type Tone = "neutral" | "posted" | "exception" | "accent" | "ink";

const tones: Record<Tone, string> = {
  neutral: "bg-sand text-ink-soft",
  posted: "bg-posted-tint text-posted",
  exception: "bg-exception-tint text-exception",
  accent: "bg-accent-tint text-accent",
  ink: "bg-ink text-on-ink",
};

export function Badge({ tone = "neutral", children }: { tone?: Tone; children: ReactNode }) {
  return <span className={`inline-flex items-center rounded-sm px-1.5 py-px text-xs font-medium leading-5 whitespace-nowrap ${tones[tone]}`}>{children}</span>;
}

const statusTone: Record<TokenEntryStatus, Tone> = {
  draft: "neutral",
  submitted: "accent",
  approved: "posted",
  rejected: "exception",
  locked: "ink",
};

export function StatusBadge({ status }: { status: TokenEntryStatus }) {
  return <Badge tone={statusTone[status]}>{status}</Badge>;
}

export const costKindLabel: Record<CostKind, string> = {
  direct: "Direct",
  project_overhead: "Project overhead",
  organization_overhead: "Organization overhead",
};

export function CostKindBadge({ kind }: { kind: CostKind }) {
  return <Badge tone={kind === "direct" ? "posted" : kind === "organization_overhead" ? "exception" : "neutral"}>{costKindLabel[kind]}</Badge>;
}

export const valueLabelText: Record<ValueLabel, string> = {
  billed: "billed",
  list_estimate: "list-price estimate",
  unknown_model: "unknown model",
};
