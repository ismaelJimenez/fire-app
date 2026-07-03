// Single-currency formatting helpers. Amounts are stored as integer cents.

const currencyFmt = new Intl.NumberFormat(undefined, {
  style: "currency",
  currency: "EUR",
  minimumFractionDigits: 2,
});

/** Format cents as a currency string, e.g. -1234 -> "-€12.34". */
export function formatMoney(cents: number): string {
  return currencyFmt.format(cents / 100);
}

/** Parse a user-entered decimal string into integer cents. */
export function parseAmountToCents(value: string): number | null {
  const cleaned = value.replace(/[^0-9.\-]/g, "");
  if (cleaned === "" || cleaned === "-") return null;
  const num = Number(cleaned);
  if (Number.isNaN(num)) return null;
  return Math.round(num * 100);
}

/** cents -> plain decimal string for editing, e.g. -1234 -> "-12.34". */
export function centsToInput(cents: number): string {
  return (cents / 100).toFixed(2);
}

const dateFmt = new Intl.DateTimeFormat(undefined, {
  year: "numeric",
  month: "short",
  day: "numeric",
});

export function formatDate(iso: string): string {
  const d = new Date(iso + "T00:00:00");
  if (Number.isNaN(d.getTime())) return iso;
  return dateFmt.format(d);
}

export function todayIso(): string {
  return new Date().toISOString().slice(0, 10);
}
