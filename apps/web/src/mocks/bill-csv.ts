// The mock's reading of a CSV bill file, mirroring the service
// (apps/server/src/integrations/bills/csv): our own format, one bill per row
// in the bill schema's columns, or a cloud's cost export summed into one
// month's bill. Errors name the row and column in the service's words.

import { parseCsv } from "@/lib/csv";
import { parseDec } from "./fold";

export interface CsvBill {
  provider: string;
  from: string;
  to: string;
  /** Nanos of the bill's currency. */
  amount: number;
  currency: string;
  exchange_rate: string | null;
  /** How many lines a cloud's export was summed from; 0 for our own format. */
  lines: number;
}

export interface CsvProblem {
  row: number | null;
  column: string | null;
  message: string;
}

export type Read = { bills: CsvBill[]; problem: null } | { bills: []; problem: CsvProblem };

const REQUIRED = ["provider", "from", "to", "amount", "currency"];
const OPTIONAL = ["exchange_rate"];
const SCHEMA = "provider, from, to, amount and currency, and optionally exchange_rate";

const problem = (message: string, row: number | null = null, column: string | null = null): Read => ({ bills: [], problem: { row, column, message } });

export function describeProblem(p: CsvProblem): string {
  if (p.row !== null && p.column !== null) return `Row ${String(p.row)}, column ${p.column}: ${p.message}`;
  if (p.row !== null) return `Row ${String(p.row)}: ${p.message}`;
  if (p.column !== null) return `Column ${p.column}: ${p.message}`;
  return p.message;
}

/** A header name as a key: lower case, runs of other characters as one underscore, a unit in brackets dropped. */
export function columnKey(name: string): string {
  const base = name.replace(/^\uFEFF/, "").split("(")[0] ?? "";
  return base
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");
}

/** The separator that appears more often outside quotes in the first lines: a semicolon, or else a comma. */
function separator(text: string): "," | ";" {
  let commas = 0;
  let semicolons = 0;
  let quoted = false;
  let lines = 0;
  for (const c of text) {
    if (c === '"') quoted = !quoted;
    else if (c === "," && !quoted) commas += 1;
    else if (c === ";" && !quoted) semicolons += 1;
    else if (c === "\n" && !quoted) {
      lines += 1;
      if (lines >= 10) break;
    }
  }
  return semicolons > commas ? ";" : ",";
}

function sheet(text: string): { rows: string[][]; decimalComma: boolean } {
  const plain = text.replace(/^\uFEFF/, "");
  const sep = separator(plain);
  return { rows: parseCsv(plain, sep).map((r) => r.map((c) => c.trim())), decimalComma: sep === ";" };
}

function plainNumber(text: string, decimalComma: boolean): string {
  const trimmed = text.replace(/\s/g, "");
  return decimalComma ? trimmed.replaceAll(".", "").replace(",", ".") : trimmed.replaceAll(",", "");
}

function readDecimal(text: string, decimalComma: boolean): string | null {
  const plain = plainNumber(text, decimalComma);
  return /^-?\d+(\.\d+)?$/.test(plain) ? plain : null;
}

/** A day as `YYYY-MM-DD`, from ISO text (a time after it is dropped) or a US date. */
export function readDay(text: string): string | null {
  const t = text.trim();
  const iso = /^(\d{4})-(\d{2})-(\d{2})/.exec(t);
  if (iso) return valid(Number(iso[1]), Number(iso[2]), Number(iso[3]));
  const us = /^(\d{1,2})\/(\d{1,2})\/(\d{4})/.exec(t);
  if (us) return valid(Number(us[3]), Number(us[1]), Number(us[2]));
  return null;
}

function valid(year: number, month: number, day: number): string | null {
  const date = new Date(Date.UTC(year, month - 1, day));
  if (date.getUTCFullYear() !== year || date.getUTCMonth() !== month - 1 || date.getUTCDate() !== day) return null;
  return date.toISOString().slice(0, 10);
}

/** Our own format: a header naming the bill schema's columns, then one row per bill. */
export function parseBills(text: string): Read {
  const { rows, decimalComma } = sheet(text);
  const header = rows[0];
  if (!header) return problem(`The file is empty. It needs a header row naming ${SCHEMA}, then one row per bill.`);
  const keys = header.map(columnKey);
  const seenKeys = new Set<string>();
  for (const [i, key] of keys.entries()) {
    if (!REQUIRED.includes(key) && !OPTIONAL.includes(key)) return problem(`The header has a column ${JSON.stringify(header[i] ?? "")} that is not part of the bill schema. Use ${SCHEMA}.`);
    if (seenKeys.has(key)) return problem(`The header names ${key} twice. Give each column once.`);
    seenKeys.add(key);
  }
  for (const key of REQUIRED) if (!seenKeys.has(key)) return problem(`The header has no ${key} column. A bill needs the columns ${SCHEMA}.`);
  const at = (row: string[], key: string) => row[keys.indexOf(key)] ?? "";
  const name = (key: string) => header[keys.indexOf(key)] ?? key;

  const bills: CsvBill[] = [];
  const seen = new Map<string, number>();
  for (const [index, row] of rows.slice(1).entries()) {
    const number = index + 2;
    if (row.slice(header.length).some((v) => v !== "")) return problem(`This row has more values than the header has columns (${String(header.length)}).`, number);
    const provider = at(row, "provider").toLowerCase();
    if (!provider) return problem("The provider is empty. Write who charged, such as anthropic.", number, name("provider"));
    const from = readDay(at(row, "from"));
    if (!from) return problem(`${JSON.stringify(at(row, "from"))} is not a day. Write it as YYYY-MM-DD, such as 2026-03-31.`, number, name("from"));
    const to = readDay(at(row, "to"));
    if (!to) return problem(`${JSON.stringify(at(row, "to"))} is not a day. Write it as YYYY-MM-DD, such as 2026-03-31.`, number, name("to"));
    if (to < from) return problem(`The bill ends on ${to}, before it starts on ${from}.`, number, name("to"));
    const currency = at(row, "currency").toUpperCase();
    if (!/^[A-Z]{3}$/.test(currency)) return problem(`${JSON.stringify(at(row, "currency"))} is not a currency. Use a three-letter code such as USD.`, number, name("currency"));
    const amountText = at(row, "amount");
    if (!amountText) return problem("The amount is empty. Write the amount, or 0.", number, name("amount"));
    const amount = readDecimal(amountText, decimalComma);
    if (amount === null) return problem(`${JSON.stringify(amountText)} is not an amount. Write a plain number such as 1234.56.`, number, name("amount"));
    if (amount.startsWith("-")) return problem(`${JSON.stringify(amountText)} is below zero. A bill's amount is zero or more.`, number, name("amount"));
    let exchange_rate: string | null = null;
    if (seenKeys.has("exchange_rate") && at(row, "exchange_rate") !== "") {
      const rate = readDecimal(at(row, "exchange_rate"), decimalComma);
      if (rate === null || Number(rate) <= 0) return problem(`${JSON.stringify(at(row, "exchange_rate"))} is not an exchange rate. Write a number above zero, such as 0.9214.`, number, name("exchange_rate"));
      exchange_rate = rate;
    }
    const key = `${provider} ${from} ${to}`;
    const first = seen.get(key);
    if (first !== undefined) return problem(`This is a second bill for ${provider} from ${from} to ${to}; row ${String(first)} already is one. Give each bill once per provider and range.`, number);
    seen.set(key, number);
    bills.push({ provider, from, to, amount: parseDec(amount), currency, exchange_rate, lines: 0 });
  }
  if (!bills.length) return problem("The file has a header but no bills. Add one row per bill below the header.");
  return { bills, problem: null };
}

export type Cloud = "bedrock" | "vertex" | "foundry";

const clouds: Record<Cloud, { provider: string; service: string; export: string; date: string[]; cost: string[]; currency: string[]; kind: string[]; skip: string[] }> = {
  bedrock: {
    provider: "aws_bedrock",
    service: "Amazon Bedrock",
    export: "an AWS Cost and Usage Report",
    date: ["line_item_usage_start_date"],
    cost: ["line_item_unblended_cost"],
    currency: ["line_item_currency_code"],
    kind: ["line_item_line_item_type"],
    skip: ["tax", "fee", "rifee", "savingsplanrecurringfee", "savingsplanupfrontfee"],
  },
  vertex: {
    provider: "gcp_vertex",
    service: "Vertex AI",
    export: "a Google Cloud billing export",
    date: ["usage_start_date", "usage_start_time", "usage_date"],
    cost: ["unrounded_cost", "cost"],
    currency: ["currency"],
    kind: ["cost_type"],
    skip: ["tax", "total", "roundingerror"],
  },
  foundry: {
    provider: "azure_foundry",
    service: "Foundry",
    export: "an Azure cost export",
    date: ["date", "usage_date", "charge_period_start"],
    cost: ["cost_in_billing_currency", "pre_tax_cost", "billed_cost", "cost"],
    currency: ["billing_currency", "billing_currency_code", "currency"],
    kind: ["charge_type", "charge_category"],
    skip: ["tax", "purchase", "roundingadjustment"],
  },
};

/** The first and last day of `YYYY-MM`. */
export function monthRange(month: string): { from: string; to: string } | null {
  const m = /^(\d{4})-(0[1-9]|1[0-2])$/.exec(month);
  if (!m) return null;
  const year = Number(m[1]);
  const number = Number(m[2]);
  const last = new Date(Date.UTC(year, number, 0)).getUTCDate();
  return { from: `${m[1] ?? ""}-${m[2] ?? ""}-01`, to: `${m[1] ?? ""}-${m[2] ?? ""}-${String(last).padStart(2, "0")}` };
}

/** A cloud's own cost export as the bill for one month: the usage rows of its AI service inside the month, added up. */
export function parseCloudExport(cloud: Cloud, text: string, month: string): Read {
  const spec = clouds[cloud];
  const range = monthRange(month);
  if (!range) return problem(`${JSON.stringify(month)} is not a month such as 2026-03.`);
  const { rows, decimalComma } = sheet(text);
  const headerIndex = rows.slice(0, 50).findIndex((row) => {
    const keys = row.map(columnKey);
    return spec.date.some((k) => keys.includes(k)) && spec.cost.some((k) => keys.includes(k));
  });
  if (headerIndex < 0) return problem(`This is not ${spec.export} for ${spec.service}: no row names the columns it needs (${spec.date[0] ?? ""}, ${spec.cost[0] ?? ""}).`);
  const header = (rows[headerIndex] ?? []).map(columnKey);
  const find = (names: string[]) => names.map((n) => header.indexOf(n)).find((i) => i >= 0) ?? -1;
  const dateAt = find(spec.date);
  const costAt = find(spec.cost);
  const currencyAt = find(spec.currency);
  const kindAt = find(spec.kind);

  let total = 0;
  let lines = 0;
  let currency: string | null = null;
  for (const [index, row] of rows.slice(headerIndex + 1).entries()) {
    const number = headerIndex + index + 2;
    const day = readDay(row[dateAt] ?? "");
    if (!day) return problem(`${JSON.stringify(row[dateAt] ?? "")} is not a date. Exports write dates such as 2026-09-01.`, number, spec.date[0] ?? null);
    if (day < range.from || day > range.to) continue;
    const kind = (row[kindAt] ?? "").toLowerCase().replace(/[^a-z]/g, "");
    if (kindAt >= 0 && spec.skip.includes(kind)) continue;
    const costText = row[costAt] ?? "";
    const cost = readDecimal(costText, decimalComma);
    if (cost === null) return problem(`${JSON.stringify(costText)} is not an amount. Write a plain number such as 12.34.`, number, spec.cost[0] ?? null);
    const code = currencyAt >= 0 ? (row[currencyAt] ?? "").toUpperCase() : "USD";
    if (!/^[A-Z]{3}$/.test(code)) return problem(`${JSON.stringify(row[currencyAt] ?? "")} is not a currency. Exports use codes such as USD.`, number, spec.currency[0] ?? null);
    if (currency && code !== currency) return problem(`This row is in ${code}; the rows before it are in ${currency}. A bill is in one currency.`, number, spec.currency[0] ?? null);
    currency = code;
    total += parseDec(cost);
    lines += 1;
  }
  if (!currency) return problem(`No row of this export is ${spec.service} cost in ${month}. Check that the export covers ${month} and includes ${spec.service}.`);
  return { bills: [{ provider: spec.provider, from: range.from, to: range.to, amount: Math.max(0, total), currency, exchange_rate: null, lines }], problem: null };
}

/** Reads a file the way the two CSV bill endpoints do, from their query. */
export function readBillCsv(text: string, cloud: string | null, month: string | null): Read {
  if (cloud === null || cloud === "") return parseBills(text);
  if (!(cloud in clouds)) return problem(`${JSON.stringify(cloud)} is not a cloud whose export can be uploaded. Use bedrock, vertex or foundry.`);
  if (!month) return problem("Give the month the export is the bill for, such as 2026-03.");
  return parseCloudExport(cloud as Cloud, text, month);
}
