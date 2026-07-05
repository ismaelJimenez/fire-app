import { describe, it, expect } from "vitest";
import {
  formatMoney,
  formatMoneyCompact,
  formatMonth,
  parseAmountToCents,
  centsToInput,
} from "./format";

describe("parseAmountToCents", () => {
  it("parses plain decimals without float error", () => {
    expect(parseAmountToCents("12.34")).toBe(1234);
    expect(parseAmountToCents("-12.34")).toBe(-1234);
    expect(parseAmountToCents("1500.00")).toBe(150000);
    expect(parseAmountToCents("0.01")).toBe(1);
    // The classic binary-float trap: 0.29 * 100 is 28.9999… in IEEE 754.
    expect(parseAmountToCents("0.29")).toBe(29);
  });

  it("parses shorthand and partial decimals", () => {
    expect(parseAmountToCents("5")).toBe(500);
    expect(parseAmountToCents("5.5")).toBe(550);
    expect(parseAmountToCents(".5")).toBe(50);
    expect(parseAmountToCents("-.5")).toBe(-50);
  });

  it("strips currency symbols and separators", () => {
    expect(parseAmountToCents("$1234.56")).toBe(123456);
    expect(parseAmountToCents("€ 42.90")).toBe(4290);
  });

  it("rounds half-up on the third decimal", () => {
    expect(parseAmountToCents("12.345")).toBe(1235);
    expect(parseAmountToCents("12.344")).toBe(1234);
    expect(parseAmountToCents("-12.345")).toBe(-1235);
  });

  it("returns null for malformed input", () => {
    for (const bad of ["", "-", "abc", "1.2.3", ".", "--5"]) {
      expect(parseAmountToCents(bad)).toBeNull();
    }
  });

  it("round-trips through centsToInput", () => {
    for (const cents of [1234, -1234, 0, 1, 150000]) {
      expect(parseAmountToCents(centsToInput(cents))).toBe(cents);
    }
  });
});

describe("formatMoney", () => {
  it("renders cents as a currency string", () => {
    // Assert on the digits rather than the symbol/locale placement.
    expect(formatMoney(1234)).toContain("12.34");
    expect(formatMoney(-1234)).toContain("12.34");
    expect(formatMoney(0)).toContain("0.00");
  });
});

describe("formatMoneyCompact", () => {
  it("abbreviates large amounts for axis labels", () => {
    // Locale-agnostic: assert on the compacted magnitude, not the symbol.
    expect(formatMoneyCompact(123456).toUpperCase()).toContain("1.2K");
    expect(formatMoneyCompact(0)).toContain("0");
  });
});

describe("formatMonth", () => {
  it("formats a YYYY-MM key with and without the year", () => {
    expect(formatMonth("2026-01")).toMatch(/2026/);
    // Short form drops the year.
    expect(formatMonth("2026-01", true)).not.toMatch(/2026/);
  });

  it("falls back to the raw key when unparseable", () => {
    expect(formatMonth("not-a-month")).toBe("not-a-month");
  });
});
