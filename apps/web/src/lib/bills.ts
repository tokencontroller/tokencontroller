import type { BillApi, BillCloud, BillCostSource, BillSource } from "@/api/contract";

// The words the bills, close and cost sheet screens use for what the service
// sends as keys (docs: Account > Bills; Reference > Bill schema, Token entry).

/** Providers as token entries name them. Any other name shows as the service stores it. */
export const providerNames: Record<string, string> = {
  anthropic: "Anthropic",
  openai: "OpenAI",
  google: "Google",
  aws_bedrock: "Bedrock",
  gcp_vertex: "Vertex",
  azure_foundry: "Foundry",
  github: "GitHub",
  cursor: "Cursor",
  other: "Other",
};

export function providerName(provider: string): string {
  return providerNames[provider] ?? provider;
}

export const billCostSources: BillCostSource[] = ["usage_at_api_rates", "api_key", "cloud_marketplace", "reseller"];

export const costSourceNames: Record<BillCostSource, string> = {
  usage_at_api_rates: "Usage at API rates",
  api_key: "API key",
  cloud_marketplace: "Cloud marketplace",
  reseller: "Reseller",
};

export function costSourceName(source: string): string {
  return (costSourceNames as Record<string, string | undefined>)[source] ?? source;
}

/** Where a bill revision came from. */
export const billSourceNames: Record<BillSource, string> = { pulled: "pulled", csv: "CSV", manual: "typed in", api: "API" };

/** The providers whose admin API a bill can be pulled from. */
export const billApis: { value: BillApi; name: string; key: string }[] = [
  { value: "anthropic_enterprise", name: "Anthropic Enterprise", key: "An Enterprise Analytics API key." },
  { value: "anthropic_console", name: "Anthropic Console", key: "A Console admin key." },
  { value: "openai", name: "OpenAI", key: "An OpenAI admin key." },
];

export function billApiName(api: BillApi): string {
  return billApis.find((a) => a.value === api)?.name ?? api;
}

/** The clouds whose own cost export uploads as a bill. */
export const billClouds: { value: BillCloud; name: string }[] = [
  { value: "bedrock", name: "Bedrock: the AWS Cost and Usage Report" },
  { value: "vertex", name: "Vertex: the Google Cloud billing export" },
  { value: "foundry", name: "Foundry: the Azure cost export" },
];
