// Pure date-range math for the Trends view, kept out of the component so it can
// be unit-tested directly. Ranges are expressed as ISO `YYYY-MM-DD` bounds that
// the backend Trends commands accept (a null bound means "the full history on
// that side"; all-time is both null).

export type Period = "ytd" | "12m" | "all";

export interface DateRange {
  /** Inclusive lower bound (ISO date), or null for "since the beginning". */
  from: string | null;
  /** Inclusive upper bound (ISO date), or null for "through the latest data". */
  to: string | null;
}

/**
 * Resolve a period preset to concrete date bounds, relative to `today`
 * (ISO `YYYY-MM-DD`).
 *
 * - `ytd`  — Jan 1st of this year through today.
 * - `12m`  — the first day of the month 11 months back through today, i.e. a
 *            trailing window of 12 calendar months including the current one.
 * - `all`  — unbounded on both sides; the backend anchors it to the data.
 */
export function periodRange(period: Period, today: string): DateRange {
  if (period === "all") return { from: null, to: null };

  const year = Number(today.slice(0, 4));
  const month = Number(today.slice(5, 7)); // 1–12

  if (period === "ytd") {
    return { from: `${pad4(year)}-01-01`, to: today };
  }

  // 12m: step back 11 whole months from the current month.
  let sy = year;
  let sm = month - 11;
  while (sm <= 0) {
    sm += 12;
    sy -= 1;
  }
  return { from: `${pad4(sy)}-${pad2(sm)}-01`, to: today };
}

function pad2(n: number): string {
  return String(n).padStart(2, "0");
}
function pad4(n: number): string {
  return String(n).padStart(4, "0");
}
