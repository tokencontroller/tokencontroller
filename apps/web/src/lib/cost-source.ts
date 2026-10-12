import type { CostSource } from "@/api/contract";

/** What the close reconciles against, per cost source (docs: Account > Bills). */
export const costSourceText: Record<CostSource, { name: string; against: string; list: string }> = {
  usage_at_api_rates: { name: "Enterprise plan, usage at API rates", against: "the Enterprise Analytics cost report at list price; the discount is its own line", list: "money, before discount" },
  api_key: { name: "Console or API key", against: "the Usage and Cost report, per workspace and day", list: "money" },
  seat: { name: "Seats: Claude Team", against: "nothing: the seat is a flat fee and there is no bill to reconcile", list: "tokens at API rates, labelled as an estimate on every figure" },
  subscription: { name: "Subscription: Claude Max or Pro", against: "nothing: the subscription is a flat fee and there is no bill to reconcile", list: "tokens at API rates, labelled as an estimate on every figure" },
  cloud_marketplace: { name: "Cloud marketplace: Bedrock, Vertex, Foundry", against: "the cloud's export, uploaded as CSV", list: "approximate" },
  reseller: { name: "Through a reseller", against: "the reseller's bill, uploaded as CSV or typed in", list: "money" },
};

/** No cost source picked yet. */
export const noCostSource = { name: "None yet", against: "nothing yet; pick a cost source under Settings", list: "a list-price estimate" };

export function describeCostSource(source: CostSource | null | undefined): { name: string; against: string; list: string } {
  return source ? costSourceText[source] : noCostSource;
}

/** Seats and subscriptions are flat fees: every figure is an estimate at API prices. */
export function isFlatFee(source: CostSource | null | undefined): boolean {
  return source === "seat" || source === "subscription";
}

/** The choices, in the order the docs' quick start names them. */
export const costSourceOrder: CostSource[] = ["usage_at_api_rates", "seat", "subscription", "api_key", "cloud_marketplace", "reseller"];
