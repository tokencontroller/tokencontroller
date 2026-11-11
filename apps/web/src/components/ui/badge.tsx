import type { ReactNode } from "react";
import { labelText } from "@/lib/format";

type Tone = "neutral" | "posted" | "exception" | "accent" | "ink";

const tones: Record<Tone, string> = {
  neutral: "bg-sand text-ink-soft ring-line-strong/60",
  posted: "bg-posted-tint text-posted ring-posted/25",
  exception: "bg-exception-tint text-exception ring-exception/25",
  accent: "bg-accent-tint text-accent ring-accent/25",
  ink: "bg-ink text-on-ink ring-ink",
};

export function Badge({ tone = "neutral", children }: { tone?: Tone; children: ReactNode }) {
  return <span className={`inline-flex items-center rounded-md px-1.5 py-px text-caption font-medium leading-5 whitespace-nowrap ring-1 ring-inset ${tones[tone]}`}>{children}</span>;
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
