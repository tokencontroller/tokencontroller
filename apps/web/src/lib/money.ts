import type { Money } from "@/api/contract";

// Money arrives in whole nano-units (ADR 0004). It is added up as BigInt and
// rounded to the currency's minor unit only when shown or exported.

export const NANOS_PER_UNIT = 1_000_000_000n;

export function nanosOf(money: Money): bigint {
  return BigInt(money.nanos);
}

export function money(nanos: bigint, currency: string): Money {
  return { nanos: nanos.toString(), currency };
}

export function sum(amounts: Money[], currency: string): Money {
  let total = 0n;
  for (const amount of amounts) total += nanosOf(amount);
  return money(total, currency);
}

export function minorDigits(currency: string): number {
  return new Intl.NumberFormat("en", { style: "currency", currency }).resolvedOptions()
    .maximumFractionDigits ?? 2;
}

/** Rounds half away from zero to the currency's minor unit, as the core does. */
export function toMinorUnits(amount: Money): bigint {
  const step = NANOS_PER_UNIT / 10n ** BigInt(minorDigits(amount.currency));
  const nanos = nanosOf(amount);
  const whole = nanos / step;
  const rest = nanos % step;
  const twice = (rest < 0n ? -rest : rest) * 2n;
  if (twice >= step) return nanos < 0n ? whole - 1n : whole + 1n;
  return whole;
}

/** The amount as a plain decimal string in major units, such as `212.40`. */
export function toDecimal(amount: Money): string {
  const digits = minorDigits(amount.currency);
  const minor = toMinorUnits(amount);
  const negative = minor < 0n;
  const text = (negative ? -minor : minor).toString().padStart(digits + 1, "0");
  const major = digits === 0 ? text : `${text.slice(0, -digits)}.${text.slice(-digits)}`;
  return negative ? `-${major}` : major;
}

/** Parses a typed amount such as `1,500.00` into money. Returns null if it is not a number. */
export function parseAmount(text: string, currency: string): Money | null {
  const clean = text.replace(/[,\s]/g, "");
  const match = /^(-?)(\d+)(?:\.(\d{0,9}))?$/.exec(clean);
  if (!match) return null;
  const [, sign, whole = "0", fraction = ""] = match;
  const nanos = BigInt(whole) * NANOS_PER_UNIT + BigInt(fraction.padEnd(9, "0"));
  return money(sign === "-" ? -nanos : nanos, currency);
}

const formatters = new Map<string, Intl.NumberFormat>();

export function formatMoney(amount: Money, options: { signed?: boolean } = {}): string {
  const key = `${amount.currency}:${options.signed ? "s" : ""}`;
  let formatter = formatters.get(key);
  if (!formatter) {
    formatter = new Intl.NumberFormat("en-US", {
      style: "currency",
      currency: amount.currency,
      signDisplay: options.signed ? "exceptZero" : "auto",
    });
    formatters.set(key, formatter);
  }
  // Intl takes a decimal string, so no float ever holds the amount.
  return formatter.format(toDecimal(amount) as unknown as number).replace("-", "−");
}

/** Basis points as a percentage, such as `95%` or `3.1%`. */
export function formatShare(basisPoints: number): string {
  const percent = basisPoints / 100;
  return `${Number.isInteger(percent) || Math.abs(percent) >= 10 ? Math.round(percent) : percent.toFixed(1)}%`;
}

/** `part` as basis points of `whole`. */
export function shareOf(part: Money, whole: Money): number {
  const total = nanosOf(whole);
  if (total === 0n) return 0;
  return Number((nanosOf(part) * 10000n) / total);
}
