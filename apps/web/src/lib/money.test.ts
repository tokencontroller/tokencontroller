import { describe, expect, test } from "vitest";
import { formatMoney, formatShare, fromDecimal, money, parseAmount, shareOf, sum, toDecimal, toMinorUnits } from "./money";

const usd = (nanos: bigint) => money(nanos, "USD");

describe("money", () => {
  test("rounds half away from zero to the minor unit", () => {
    expect(toMinorUnits(usd(4_185_000_000n))).toBe(419n);
    expect(toMinorUnits(usd(4_184_999_999n))).toBe(418n);
    expect(toMinorUnits(usd(-4_185_000_000n))).toBe(-419n);
    expect(toMinorUnits(money(1_500_000_000n, "JPY"))).toBe(2n);
  });

  test("adds nano-units without floats", () => {
    // 0.1 + 0.2 is 0.30000000000000004 as floats.
    expect(toDecimal(sum([usd(100_000_000n), usd(200_000_000n)], "USD"))).toBe("0.30");
    // Amounts beyond 2^53 nano-units stay exact.
    expect(sum([usd(9_007_199_254_740_993n), usd(1n)], "USD").nanos).toBe("9007199254740994");
  });

  test("accepts nanos as a number or a string", () => {
    expect(toDecimal({ nanos: 212_400_000_000, currency: "USD" })).toBe("212.40");
    expect(toDecimal({ nanos: "212400000000", currency: "USD" })).toBe("212.40");
  });

  test("parses typed amounts", () => {
    expect(parseAmount("1,500.00", "USD")).toEqual(usd(1_500_000_000_000n));
    expect(parseAmount("-18.4", "USD")).toEqual(usd(-18_400_000_000n));
    expect(parseAmount("12.5 EUR", "EUR")).toBeNull();
    expect(parseAmount("", "USD")).toBeNull();
  });

  test("formats money and shares the way the docs show them", () => {
    expect(formatMoney(usd(212_400_000_000n))).toBe("$212.40");
    expect(formatMoney(usd(-18_400_000_000n))).toBe("−$18.40");
    expect(formatMoney(money(1_500_000_000_000n, "EUR"))).toBe("€1,500.00");
    expect(formatShare(9500)).toBe("95%");
    expect(formatShare(310)).toBe("3.1%");
    expect(shareOf(usd(202_650_000_000n), usd(212_400_000_000n))).toBe(9540);
  });

  test("reads the service's decimal strings", () => {
    expect(fromDecimal("-41.87", "EUR")).toEqual({ nanos: "-41870000000", currency: "EUR" });
    expect(formatMoney(fromDecimal("1500.00", "USD"))).toBe("$1,500.00");
    expect(() => fromDecimal("about 3", "USD")).toThrow();
  });
});
