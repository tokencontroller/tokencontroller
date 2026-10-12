// The mock service's reading of bills uploaded as CSV, as the service reads
// them (apps/server/src/integrations/bills/csv): our own format, one bill
// per row, and the cost exports of Bedrock, Vertex and Foundry, one bill per
// file. The first problem found stops the reading and names its row and
// column, in the service's words.

import type { BillCostSource } from "@/api/contract";

export type Cloud = "bedrock" | "vertex" | "foundry";

export const CLOUDS: Cloud[] = ["bedrock", "vertex", "foundry"];

/** One line of a cloud's export: usage per day, account or project and kind of usage. */
export interface CsvBillLine {
  date: string;
  workspace: string | null;
  description: string;
  /** In nano-units of the bill's currency. */
  amount: bigint;
}

/** A bill as a file holds it, amounts in nano-units of its currency. */
export interface CsvBill {
  period: string;
  /** As the service stores it, such as `anthropic` or `cursor`. */
  provider: string;
  cost_source: BillCostSource;
  currency: string;
  gross: bigint;
  discount: bigint;
  credit: bigint;
  net: bigint;
  /** As the file writes it, without trailing zeros; null when left out. */
  exchange_rate: string | null;
  lines: CsvBillLine[];
}

/** What is wrong with a file, and where. Rows count the header as row 1. */
export class CsvBillProblem extends Error {
  constructor(
    readonly row: number | null,
    readonly column: string | null,
    message: string,
  ) {
    super(message);
  }

  /** As the service writes it in an upload's error. */
  get text(): string {
    if (this.row !== null && this.column !== null) return `Row ${String(this.row)}, column ${this.column}: ${this.message}`;
    if (this.row !== null) return `Row ${String(this.row)}: ${this.message}`;
    if (this.column !== null) return `Column ${this.column}: ${this.message}`;
    return this.message;
  }
}

const fileProblem = (message: string) => new CsvBillProblem(null, null, message);

/** How Rust's `{:?}` quotes a value in a message. */
export const quoted = (text: string) => JSON.stringify(text);

// Reading the sheet: a byte order mark, comma or semicolon separators,
// quoted fields, and the line each row starts on.

interface Row {
  line: number;
  cells: string[];
}

interface Sheet {
  rows: Row[];
  decimalComma: boolean;
}

function separator(text: string): "," | ";" {
  let commas = 0;
  let semicolons = 0;
  let lines = 0;
  let inQuotes = false;
  for (const c of text) {
    if (c === '"') inQuotes = !inQuotes;
    else if (c === "," && !inQuotes) commas += 1;
    else if (c === ";" && !inQuotes) semicolons += 1;
    else if (c === "\n" && !inQuotes) {
      lines += 1;
      if (lines >= 10) break;
    }
  }
  return semicolons > commas ? ";" : ",";
}

function readSheet(raw: string): Sheet {
  const text = raw.startsWith("﻿") ? raw.slice(1) : raw;
  const sep = separator(text);
  const rows: Row[] = [];
  let cells: string[] = [];
  let field = "";
  let inQuotes = false;
  let fieldStart = true;
  let line = 1;
  let rowLine = 1;
  const endField = () => {
    cells.push(field.trim());
    field = "";
    fieldStart = true;
  };
  const endRow = () => {
    endField();
    if (cells.some(Boolean)) rows.push({ line: rowLine, cells });
    cells = [];
  };
  for (let i = 0; i < text.length; i += 1) {
    const c = text.charAt(i);
    if (inQuotes) {
      if (c === '"' && text.charAt(i + 1) === '"') {
        field += '"';
        i += 1;
      } else if (c === '"') inQuotes = false;
      else {
        if (c === "\n") line += 1;
        field += c;
      }
    } else if (c === '"' && fieldStart) {
      inQuotes = true;
      fieldStart = false;
    } else if (c === sep) endField();
    else if (c === "\n" || c === "\r") {
      if (c === "\r" && text.charAt(i + 1) === "\n") continue;
      endRow();
      if (c === "\n") line += 1;
      rowLine = line;
    } else {
      field += c;
      fieldStart = false;
    }
  }
  if (field || cells.length) endRow();
  return { rows, decimalComma: sep === ";" };
}

/** The key of a column name, and the currency in it: `Cost (EUR)` is `cost` in EUR. */
export function columnKey(name: string): { key: string; currency: string | null } {
  const trimmed = name.trim().replace(/^\uFEFF+/, "");
  const open = trimmed.indexOf("(");
  const base = open === -1 ? trimmed : trimmed.slice(0, open);
  const inside = open === -1 ? "" : trimmed.slice(open + 1).trimEnd().replace(/\)+$/, "").trim().toUpperCase();
  let key = "";
  let previous = "";
  for (const c of base) {
    if (/[A-Za-z0-9]/.test(c)) {
      if (/[A-Z]/.test(c) && /[a-z0-9]/.test(previous)) key += "_";
      key += c.toLowerCase();
    } else if (key && !key.endsWith("_")) key += "_";
    previous = c;
  }
  return { key: key.replace(/_+$/, ""), currency: /^[A-Z]{3}$/.test(inside) ? inside : null };
}

interface Column {
  index: number;
  /** As the file spells it, for messages. */
  name: string;
}

class Header {
  readonly keys: string[];
  readonly names: string[];
  readonly currency: string | null;

  constructor(cells: string[]) {
    const read = cells.map(columnKey);
    this.keys = read.map((r) => r.key);
    this.names = cells;
    this.currency = read.find((r) => r.currency)?.currency ?? null;
  }

  find(candidates: readonly string[]): Column | null {
    for (const candidate of candidates) {
      const index = this.keys.indexOf(candidate);
      if (index !== -1) return { index, name: this.names[index] ?? "" };
    }
    return null;
  }
}

const cell = (row: Row, column: Column | null) => (column ? (row.cells[column.index] ?? "").trim() : "");

/** A decimal number in nano-units, rounded half to even past the ninth digit as the service's decimals do. */
export function decimalNanos(text: string): bigint | null {
  const match = /^([+-]?)(\d*)(?:\.(\d*))?(?:[eE]([+-]?\d+))?$/.exec(text.trim());
  if (!match) return null;
  const [, sign = "", whole = "", fraction = "", exponent = "0"] = match;
  if (!whole && !fraction) return null;
  const digits = BigInt(`${whole}${fraction}` || "0");
  const shift = 9 - fraction.length + Number(exponent);
  let nanos: bigint;
  if (shift >= 0) nanos = digits * 10n ** BigInt(shift);
  else {
    const divisor = 10n ** BigInt(-shift);
    nanos = digits / divisor;
    const rest = (digits % divisor) * 2n;
    if (rest > divisor || (rest === divisor && nanos % 2n === 1n)) nanos += 1n;
  }
  return sign === "-" ? -nanos : nanos;
}

function plainNumber(text: string, decimalComma: boolean): string {
  const trimmed = text.trim();
  if (decimalComma && trimmed.includes(",")) return trimmed.replace(/[. \u00A0]/g, "").replace(/,/g, ".");
  return trimmed;
}

const readAmount = (text: string, decimalComma: boolean) => decimalNanos(plainNumber(text, decimalComma));

/** Money as the service writes it in a message, such as `12.35 USD`. */
export function moneyText(nanos: bigint, currency: string): string {
  const digits = minorDigits(currency);
  const step = 10n ** BigInt(9 - digits);
  const minor = divideRounded(nanos, step);
  const magnitude = minor < 0n ? -minor : minor;
  const scale = 10n ** BigInt(digits);
  const units = (magnitude / scale).toString();
  const fraction = (magnitude % scale).toString().padStart(digits, "0");
  return `${minor < 0n ? "-" : ""}${digits ? `${units}.${fraction}` : units} ${currency}`;
}

/** Digits after the point in a currency's minor unit, per ISO 4217, as the core counts them. */
export function minorDigits(currency: string): number {
  if (["BIF", "CLP", "DJF", "GNF", "ISK", "JPY", "KMF", "KRW", "PYG", "RWF", "UGX", "UYI", "VND", "VUV", "XAF", "XOF", "XPF"].includes(currency)) return 0;
  if (["BHD", "IQD", "JOD", "KWD", "LYD", "OMR", "TND"].includes(currency)) return 3;
  if (["CLF", "UYW"].includes(currency)) return 4;
  return 2;
}

/** Divides and rounds half away from zero, as the core does. */
export function divideRounded(numerator: bigint, denominator: bigint): bigint {
  const quotient = numerator / denominator;
  const rest = numerator % denominator;
  const twice = (rest < 0n ? -rest : rest) * 2n;
  if (twice >= denominator) return numerator < 0n ? quotient - 1n : quotient + 1n;
  return quotient;
}

const PERIOD = /^\d{4}-(0[1-9]|1[0-2])$/;
export const isPeriod = (text: string) => PERIOD.test(text);

// Our own format.

const REQUIRED = ["period", "cost_source", "gross", "discount", "credit", "net", "currency"] as const;
const OPTIONAL = ["exchange_rate", "provider"] as const;
const SCHEMA = "period, cost_source, gross, discount, credit, net and currency, and optionally exchange_rate and provider";

/** Providers the service knows by other names, such as `Bedrock`. */
const KNOWN_PROVIDERS: Record<string, string> = {
  anthropic: "anthropic",
  anthropic_console: "anthropic",
  anthropic_enterprise: "anthropic",
  claude: "anthropic",
  openai: "openai",
  open_ai: "openai",
  google: "google",
  gemini: "google",
  aws_bedrock: "aws_bedrock",
  bedrock: "aws_bedrock",
  amazon_bedrock: "aws_bedrock",
  gcp_vertex: "gcp_vertex",
  vertex: "gcp_vertex",
  vertex_ai: "gcp_vertex",
  google_vertex: "gcp_vertex",
  azure_foundry: "azure_foundry",
  foundry: "azure_foundry",
  microsoft_foundry: "azure_foundry",
  azure_openai: "azure_foundry",
  other: "other",
};

const snake = (text: string) => text.trim().replace(/[ -]/g, "_").toLowerCase();

function readProvider(text: string): string | null {
  const key = snake(text);
  const known = KNOWN_PROVIDERS[key];
  if (known) return known;
  return key && key.length <= 40 && /^[a-z0-9_]+$/.test(key) ? key : null;
}

/** Reads bills in our own format: a header naming the bill schema's fields, then one row per bill. */
export function parseBills(text: string): CsvBill[] {
  const sheet = readSheet(text);
  const [first, ...rows] = sheet.rows;
  if (!first) throw fileProblem(`The file is empty. It needs a header row naming ${SCHEMA}, then one row per bill.`);
  const header = new Header(first.cells);
  const found = new Set<string>();
  header.keys.forEach((key, index) => {
    if (key === "revision") throw fileProblem("The header has a revision column. Token Controller sets the revision when a provider restates a bill, so leave the column out.");
    if (!(REQUIRED as readonly string[]).includes(key) && !(OPTIONAL as readonly string[]).includes(key)) {
      throw fileProblem(`The header has a column ${quoted(header.names[index] ?? "")} that is not part of the bill schema. Use ${SCHEMA}.`);
    }
    if (found.has(key)) throw fileProblem(`The header names ${key} twice. Give each column once.`);
    found.add(key);
  });
  const required = (key: string): Column => {
    const column = header.find([key]);
    if (!column) throw fileProblem(`The header has no ${key} column. A bill needs the columns ${SCHEMA}.`);
    return column;
  };
  const columns = {
    period: required("period"),
    cost_source: required("cost_source"),
    gross: required("gross"),
    discount: required("discount"),
    credit: required("credit"),
    net: required("net"),
    currency: required("currency"),
    exchange_rate: header.find(["exchange_rate"]),
    provider: header.find(["provider"]),
  };

  const bills: CsvBill[] = [];
  const seen = new Map<string, number>();
  for (const row of rows) {
    if (row.cells.slice(header.keys.length).some((value) => value.trim())) {
      throw new CsvBillProblem(row.line, null, `This row has more values than the header has columns (${String(header.keys.length)}).`);
    }
    const problem = (column: Column, message: string) => new CsvBillProblem(row.line, column.name, message);
    const value = (column: Column) => cell(row, column);

    const period = value(columns.period);
    if (!isPeriod(period)) throw problem(columns.period, `${quoted(period)} is not a period. Write the calendar month as YYYY-MM, such as 2026-09.`);

    const sourceText = value(columns.cost_source);
    const source = snake(sourceText);
    if (source === "seat" || source === "subscription") {
      throw problem(columns.cost_source, "Seats and subscriptions are flat fees and have no bill. Leave this row out; finance handles them as before.");
    }
    if (!["usage_at_api_rates", "api_key", "cloud_marketplace", "reseller"].includes(source)) {
      throw problem(columns.cost_source, `${quoted(sourceText)} is not a cost source. Use usage_at_api_rates, api_key, cloud_marketplace or reseller.`);
    }
    const costSource = source as BillCostSource;

    const currencyText = value(columns.currency);
    const currency = currencyText.toUpperCase();
    if (!/^[A-Z]{3}$/.test(currency)) throw problem(columns.currency, `${quoted(currencyText)} is not a currency. Use a three-letter code such as USD.`);

    const amount = (column: Column, emptyIsZero: boolean): bigint => {
      const text = value(column);
      if (!text) {
        if (emptyIsZero) return 0n;
        throw problem(column, "The amount is empty. Write the amount, or 0.");
      }
      const nanos = readAmount(text, sheet.decimalComma);
      if (nanos === null) throw problem(column, `${quoted(text)} is not an amount. Write a plain number such as 1234.56.`);
      return nanos;
    };
    const gross = amount(columns.gross, false);
    const discount = amount(columns.discount, true);
    const credit = amount(columns.credit, true);
    const net = amount(columns.net, false);

    let provider = "other";
    if (columns.provider && value(columns.provider)) {
      const read = readProvider(value(columns.provider));
      if (!read) throw problem(columns.provider, `${quoted(value(columns.provider))} is not a provider name. Use lower-case letters, digits and _, such as anthropic, openai or cursor.`);
      provider = read;
    }

    let rate: string | null = null;
    if (columns.exchange_rate && value(columns.exchange_rate)) {
      const text = value(columns.exchange_rate);
      const nanos = decimalNanos(plainNumber(text, sheet.decimalComma));
      if (nanos === null || nanos <= 0n) throw problem(columns.exchange_rate, `${quoted(text)} is not an exchange rate. Write a number above zero, such as 0.92, or leave it empty.`);
      rate = normalized(plainNumber(text, sheet.decimalComma));
    }

    const expected = gross - discount - credit;
    if (expected !== net) {
      const m = (nanos: bigint) => moneyText(nanos, currency);
      throw problem(columns.net, `Gross ${m(gross)} minus discount ${m(discount)} minus credit ${m(credit)} is ${m(expected)}, but net is ${m(net)}. Net must be gross minus discount minus credit.`);
    }

    const id = `${provider}/${costSource}:${period}`;
    const firstRow = seen.get(id);
    if (firstRow !== undefined) {
      throw new CsvBillProblem(row.line, null, `This is a second bill for ${provider}, ${costSource} in ${period}; row ${String(firstRow)} already is one. Give each period's bill once per provider and cost source.`);
    }
    seen.set(id, row.line);
    bills.push({ period, provider, cost_source: costSource, currency, gross, discount, credit, net, exchange_rate: rate, lines: [] });
  }
  if (!bills.length) throw fileProblem("The file has a header but no bills. Add one row per bill below the header.");
  return bills;
}

/** A decimal without trailing zeros, as the service normalizes a rate: `0.920` is `0.92`. */
function normalized(text: string): string {
  const nanos = decimalNanos(text) ?? 0n;
  const whole = nanos / 1_000_000_000n;
  const fraction = (nanos % 1_000_000_000n).toString().padStart(9, "0").replace(/0+$/, "");
  return fraction ? `${whole.toString()}.${fraction}` : whole.toString();
}

// The clouds' own cost exports.

type Role =
  | "date"
  | "invoice_month"
  | "cost"
  | "currency"
  | "kind"
  | "credit_type"
  | "credits"
  | "product_code"
  | "service_code"
  | "product_name"
  | "item_description"
  | "billing_entity"
  | "service_description"
  | "seller"
  | "meter_category"
  | "service_name"
  | "consumed_service"
  | "publisher"
  | "description"
  | "workspace";

/** The column names each role goes by in each cloud's exports, first match wins. */
const NAMES: Record<Cloud, Partial<Record<Role, string[]>>> = {
  bedrock: {
    date: ["line_item_usage_start_date"],
    cost: ["line_item_unblended_cost"],
    currency: ["line_item_currency_code"],
    kind: ["line_item_line_item_type"],
    product_code: ["line_item_product_code"],
    service_code: ["product_servicecode"],
    product_name: ["product_product_name"],
    item_description: ["line_item_line_item_description"],
    billing_entity: ["bill_billing_entity"],
    description: ["line_item_usage_type", "line_item_line_item_description"],
    workspace: ["line_item_usage_account_id"],
  },
  vertex: {
    date: ["usage_start_date", "usage_start_time", "usage_date"],
    invoice_month: ["invoice_month"],
    cost: ["unrounded_cost", "cost"],
    currency: ["currency"],
    kind: ["cost_type"],
    credit_type: ["credit_type"],
    credits: ["credits"],
    service_description: ["service_description"],
    seller: ["seller_name"],
    description: ["sku_description", "service_description"],
    workspace: ["project_id"],
  },
  foundry: {
    date: ["date", "usage_date", "charge_period_start"],
    cost: ["cost_in_billing_currency", "pre_tax_cost", "billed_cost", "cost"],
    currency: ["billing_currency", "billing_currency_code", "currency"],
    kind: ["charge_type", "charge_category"],
    meter_category: ["meter_category", "x_sku_meter_category"],
    service_name: ["service_name"],
    consumed_service: ["consumed_service"],
    publisher: ["publisher_name"],
    product_name: ["product_name"],
    description: ["meter_name", "x_sku_meter_name", "product_name"],
    workspace: ["resource_name", "resource_id", "instance_name"],
  },
};

const NEEDED: Record<Cloud, Role[]> = { bedrock: ["date", "cost"], vertex: ["cost", "service_description"], foundry: ["date", "cost"] };
const ONE_OF: Record<Cloud, Role[]> = {
  bedrock: ["product_code", "service_code", "product_name"],
  vertex: ["date", "invoice_month"],
  foundry: ["meter_category", "service_name", "consumed_service", "publisher", "product_name"],
};
const SERVICE: Record<Cloud, string> = { bedrock: "Amazon Bedrock", vertex: "Vertex AI", foundry: "Foundry" };
const EXPORT: Record<Cloud, string> = { bedrock: "an AWS Cost and Usage Report", vertex: "a Google Cloud billing export", foundry: "an Azure cost export" };
const PROVIDER: Record<Cloud, string> = { bedrock: "aws_bedrock", vertex: "gcp_vertex", foundry: "azure_foundry" };

type Charge = "usage" | "discount" | "credit" | "skip";

/** A Google Cloud credit type: a discount, such as a committed use discount, or a credit, such as a promotion. */
function creditCharge(type: string): Charge {
  const key = type.trim().replace(/[ -]/g, "_").toUpperCase();
  return key.includes("DISCOUNT") || ["RESELLER_MARGIN", "SUBSCRIPTION_BENEFIT", "FEE_UTILIZATION_OFFSET"].includes(key) ? "discount" : "credit";
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

function validDay(year: number, month: number, day: number): string | null {
  const date = new Date(Date.UTC(year, month - 1, day));
  if (date.getUTCFullYear() !== year || date.getUTCMonth() !== month - 1 || date.getUTCDate() !== day) return null;
  return date.toISOString().slice(0, 10);
}

/** The day a timestamp or date falls on: `2026-09-01`, `2026-09-01T00:00:00Z`, `09/01/2026` or `Sep 1, 2026`. */
export function readDay(text: string): string | null {
  const trimmed = text.trim();
  const iso = /^(\d{4})-(\d{2})-(\d{2})/.exec(trimmed);
  if (iso) return validDay(Number(iso[1]), Number(iso[2]), Number(iso[3]));
  const firstWord = trimmed.split(/[ T]/)[0] ?? trimmed;
  const us = /^(\d{1,2})\/(\d{1,2})\/(\d{4})$/.exec(firstWord);
  if (us) return validDay(Number(us[3]), Number(us[1]), Number(us[2]));
  const written = /^([A-Z][a-z]{2}) (\d{1,2}), (\d{4})$/.exec(trimmed);
  const month = written ? MONTHS.indexOf(written[1] ?? "") : -1;
  if (written && month !== -1) return validDay(Number(written[3]), month + 1, Number(written[2]));
  return null;
}

/** Reads one cloud's cost export as the bill for `period`: only its AI service's rows in the period count. */
export function parseCloudExport(cloud: Cloud, text: string, period: string): CsvBill {
  const sheet = readSheet(text);
  const preamble = new Map<string, string>();
  let columns: Partial<Record<Role, Column>> | null = null;
  let headerCurrency: string | null = null;
  let start = 0;
  for (const [index, row] of sheet.rows.slice(0, 50).entries()) {
    const header = new Header(row.cells);
    const found: Partial<Record<Role, Column>> = {};
    for (const [role, names] of Object.entries(NAMES[cloud]) as [Role, string[]][]) {
      const column = header.find(names);
      if (column) found[role] = column;
    }
    if (NEEDED[cloud].every((role) => found[role]) && ONE_OF[cloud].some((role) => found[role])) {
      columns = found;
      headerCurrency = header.currency;
      start = index + 1;
      break;
    }
    const filled = row.cells.filter(Boolean);
    const [key, value] = filled;
    if (filled.length === 2 && key && value) {
      const name = columnKey(key).key;
      if (!preamble.has(name)) preamble.set(name, value);
    }
  }
  if (!columns) {
    const names = NEEDED[cloud].map((role) => NAMES[cloud][role]?.[0] ?? "");
    const oneOf = ONE_OF[cloud].map((role) => NAMES[cloud][role]?.[0] ?? "");
    throw fileProblem(`This is not ${EXPORT[cloud]} for ${SERVICE[cloud]}: no row names the columns it needs (${[...names, "and one of"].join(", ")} ${oneOf.join(", ")}).`);
  }
  const found = columns;
  const preambleCurrency = preamble.get("currency")?.trim().toUpperCase();
  const fallback = headerCurrency ?? (preambleCurrency && /^[A-Z]{3}$/.test(preambleCurrency) ? preambleCurrency : null);

  let currency: string | null = null;
  const usage = new Map<string, CsvBillLine>();
  const discounts: bigint[] = [];
  const credits: bigint[] = [];
  const [year = "", month = ""] = period.split("-");
  const firstDay = `${year}-${month}-01`;

  for (const row of sheet.rows.slice(start)) {
    const column = (role: Role) => found[role] ?? null;
    const value = (role: Role) => cell(row, column(role));
    const lower = (role: Role) => value(role).toLowerCase();
    const problem = (role: Role | null, message: string) => new CsvBillProblem(row.line, role ? (column(role)?.name ?? null) : null, message);

    // Whether the row is the cloud's AI service.
    let ai: boolean;
    if (cloud === "bedrock") {
      const coded = (["product_code", "service_code"] as Role[]).some((role) => ["amazonbedrock", "amazonbedrockservice"].includes(lower(role)));
      const named = (["product_name", "item_description"] as Role[]).some((role) => lower(role).includes("bedrock"));
      const marketplace = lower("billing_entity").includes("marketplace") && (["product_name", "item_description", "description"] as Role[]).some((role) => lower(role).includes("claude") || lower(role).includes("anthropic"));
      ai = coded || named || marketplace;
    } else if (cloud === "vertex") {
      const service = lower("service_description");
      ai = service === "vertex ai" || service.includes("claude") || service.includes("anthropic") || lower("seller").includes("anthropic");
    } else {
      const category = (["meter_category", "service_name"] as Role[]).some((role) => ["foundry", "openai", "cognitive services", "ai services"].some((word) => lower(role).includes(word)));
      const product = lower("product_name");
      ai = category || lower("consumed_service") === "microsoft.cognitiveservices" || lower("publisher").includes("anthropic") || ["claude", "openai", "foundry"].some((word) => product.includes(word));
    }
    if (!ai) continue;

    // Whether it belongs to the period: by its invoice month, else the day its usage started.
    const invoice = value("invoice_month");
    if (invoice) {
      const spelled = invoice.length === 6 ? `${invoice.slice(0, 4)}-${invoice.slice(4)}` : invoice;
      if (!isPeriod(spelled)) throw problem("invoice_month", `${quoted(invoice)} is not an invoice month such as 202609.`);
      if (spelled !== period) continue;
    } else {
      const day = readDay(value("date"));
      if (!day) throw problem("date", `${quoted(value("date"))} is not a date. Exports write dates such as 2026-09-01.`);
      if (day.slice(0, 7) !== period) continue;
    }

    const kindText = value("kind");
    const kind = kindText.replace(/[^A-Za-z0-9]/g, "").toLowerCase();
    const creditType = value("credit_type");
    let charge: Charge | null = null;
    if (cloud === "bedrock") {
      if (["", "usage", "discountedusage", "savingsplancoveredusage"].includes(kind)) charge = "usage";
      else if (["credit", "refund"].includes(kind)) charge = "credit";
      else if (kind === "savingsplannegation" || kind.endsWith("discount")) charge = "discount";
      else if (["tax", "fee", "rifee", "savingsplanrecurringfee", "savingsplanupfrontfee"].includes(kind)) charge = "skip";
    } else if (cloud === "vertex" && creditType) charge = creditCharge(creditType);
    else if (cloud === "vertex") {
      if (["", "usage", "regular"].includes(kind)) charge = "usage";
      else if (kind === "adjustment") charge = "credit";
      else if (["tax", "total", "roundingerror"].includes(kind)) charge = "skip";
    } else {
      if (["", "usage"].includes(kind)) charge = "usage";
      else if (["refund", "credit", "adjustment"].includes(kind)) charge = "credit";
      else if (["tax", "purchase", "unusedreservation", "unusedsavingsplan", "roundingadjustment"].includes(kind)) charge = "skip";
    }
    if (!charge) throw problem("kind", `${quoted(kindText)} is a kind of charge Token Controller does not know, so it cannot tell whether it is usage, a discount or a credit.`);
    if (charge === "skip") continue;

    const code = value("currency");
    let rowCurrency: string;
    if (!code) {
      if (!fallback) throw problem(null, "The export does not say its currency. Include the currency column.");
      rowCurrency = fallback;
    } else {
      rowCurrency = code.toUpperCase();
      if (!/^[A-Z]{3}$/.test(rowCurrency)) throw problem("currency", `${quoted(code)} is not a currency. Exports use codes such as USD.`);
    }
    if (currency && currency !== rowCurrency) {
      throw problem("currency", `This row is in ${rowCurrency}, but the rows before are in ${currency}. A bill has one currency, so export one billing account at a time.`);
    }
    currency = rowCurrency;

    const costText = value("cost");
    const cost = costText ? readAmount(costText, sheet.decimalComma) : 0n;
    if (cost === null) throw problem("cost", `${quoted(costText)} is not an amount. Write a plain number such as 12.34.`);
    if (charge === "usage") {
      const usageDay = readDay(value("date"));
      const lineDay = usageDay && usageDay.slice(0, 7) === period ? usageDay : firstDay;
      const workspace = value("workspace") || null;
      const description = value("description") || SERVICE[cloud];
      const key = JSON.stringify([lineDay, workspace ?? "", workspace === null, description]);
      const earlier = usage.get(key);
      usage.set(key, { date: lineDay, workspace, description, amount: (earlier?.amount ?? 0n) + cost });
    } else if (charge === "discount") discounts.push(cost);
    else credits.push(cost);

    // The credits a BigQuery export carries on a usage row.
    const creditsText = value("credits");
    if (creditsText) {
      const unreadable = () => problem("credits", `${quoted(creditsText)} is not a credit amount or a list of credits with amounts.`);
      if (creditsText.startsWith("[")) {
        let list: unknown;
        try {
          list = JSON.parse(creditsText);
        } catch {
          throw unreadable();
        }
        if (!Array.isArray(list)) throw unreadable();
        for (const item of list as unknown[]) {
          const credit = item as { amount?: unknown; type?: unknown } | null;
          if (typeof credit?.amount !== "number") throw unreadable();
          const amount = decimalNanos(String(credit.amount));
          if (amount === null) throw unreadable();
          if (creditCharge(typeof credit.type === "string" ? credit.type : "") === "discount") discounts.push(amount);
          else credits.push(amount);
        }
      } else {
        const amount = readAmount(creditsText, sheet.decimalComma);
        if (amount === null) throw unreadable();
        credits.push(amount);
      }
    }
  }

  if (!currency) {
    throw fileProblem(`No row of this export is ${SERVICE[cloud]} cost in ${period}. Check that the export covers ${period} and includes ${SERVICE[cloud]}.`);
  }
  const lines = [...usage.values()].sort((a, b) => a.date.localeCompare(b.date) || (a.workspace === null ? (b.workspace === null ? 0 : -1) : b.workspace === null ? 1 : a.workspace.localeCompare(b.workspace)) || a.description.localeCompare(b.description));
  const gross = lines.reduce((sum, line) => sum + line.amount, 0n);
  const discount = -discounts.reduce((sum, amount) => sum + amount, 0n);
  const credit = -credits.reduce((sum, amount) => sum + amount, 0n);
  return { period, provider: PROVIDER[cloud], cost_source: "cloud_marketplace", currency, gross, discount, credit, net: gross - discount - credit, exchange_rate: null, lines };
}
