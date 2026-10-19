import type { ReactNode } from "react";
import { labelText } from "@/lib/format";

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

const statusTone: Record<string, Tone> = { open: "neutral", submitted: "accent", approved: "posted", rejected: "exception" };

/** Where a submission or a session stands: open, submitted, approved, rejected. */
export function StatusBadge({ status }: { status: string }) {
  return <Badge tone={statusTone[status] ?? "neutral"}>{status}</Badge>;
}

/** Direct, project overhead or organization overhead. */
export function LabelBadge({ label }: { label: string }) {
  return <Badge tone={label === "direct" ? "posted" : label.startsWith("organization") ? "exception" : "neutral"}>{labelText[label] ?? label}</Badge>;
}
