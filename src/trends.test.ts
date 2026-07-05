import { describe, it, expect } from "vitest";
import { periodRange } from "./trends";

describe("periodRange", () => {
  it("YTD spans Jan 1st of the current year through today", () => {
    expect(periodRange("ytd", "2026-07-05")).toEqual({
      from: "2026-01-01",
      to: "2026-07-05",
    });
  });

  it("trailing 12 months starts 11 whole months back, crossing the year", () => {
    // July 2026 → back to August 2025 (12 months inclusive).
    expect(periodRange("12m", "2026-07-05")).toEqual({
      from: "2025-08-01",
      to: "2026-07-05",
    });
    // January stays in-bounds by wrapping into the previous year.
    expect(periodRange("12m", "2026-01-15")).toEqual({
      from: "2025-02-01",
      to: "2026-01-15",
    });
  });

  it("all-time is unbounded on both sides (the backend anchors it)", () => {
    expect(periodRange("all", "2026-07-05")).toEqual({ from: null, to: null });
  });
});
