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

/**
 * Parse a user-entered decimal string into integer cents.
 *
 * Uses integer arithmetic (no `num * 100`) so binary-float rounding can never
 * shift a value by a cent. Mirrors `parse_amount_cents` in the Rust backend
 * (`src-tauri/src/commands.rs`); keep the two in sync.
 */
export function parseAmountToCents(value: string): number | null {
  // Strip currency symbols, spaces and thousands separators.
  const cleaned = value.replace(/[^0-9.\-]/g, "");
  const negative = cleaned.startsWith("-");
  const body = negative ? cleaned.slice(1) : cleaned;
  if (body === "") return null;

  const dot = body.indexOf(".");
  const intPart = dot === -1 ? body : body.slice(0, dot);
  const fracPart = dot === -1 ? "" : body.slice(dot + 1);

  // A second '.' (e.g. "1.2.3") or any non-digit makes it malformed.
  if (fracPart.includes(".")) return null;
  if (intPart === "" && fracPart === "") return null;
  if (!/^\d*$/.test(intPart) || !/^\d*$/.test(fracPart)) return null;

  const digit = (i: number) =>
    i < fracPart.length ? fracPart.charCodeAt(i) - 48 : 0;
  let cents = (intPart === "" ? 0 : Number(intPart)) * 100 + digit(0) * 10 + digit(1);
  if (digit(2) >= 5) cents += 1; // round half-up on the third decimal

  return negative ? -cents : cents;
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
