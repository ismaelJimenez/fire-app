export type View = "dashboard" | "trends" | "transactions" | "import";

export interface Account {
  id: number;
  name: string;
  /** Parent account this is a subaccount of, or null for a top-level account. */
  parent_id: number | null;
  created_at: string;
  /** Starting balance in cents, for accounts with only partial history.
   *  Included in `balance`; excluded from income/expense totals. */
  opening_balance: number;
  /** This account's own balance in cents — opening balance plus its
   *  transactions (excludes subaccounts). */
  balance: number;
  /** This account's own transaction count (excludes subaccounts). */
  tx_count: number;
}

export interface Category {
  id: number;
  name: string;
  /** Built-in role: transactions here are transfers between the user's own
   *  accounts, excluded from income/expense totals. At most one category has it,
   *  and it can't be deleted. */
  is_transfer: boolean;
}

export interface Transaction {
  id: number;
  account_id: number;
  account_name: string;
  date: string;
  /** Amount in cents; negative = expense. */
  amount: number;
  description: string;
  /** Payee/counterparty ("concept") that drives auto-classification; may be "". */
  counterparty: string;
  category_id: number | null;
  category_name: string | null;
  /** Reviewed by the user; locked from rule-based re-categorization. */
  is_verified: boolean;
  /** Category was applied by a learned rule rather than set by hand. */
  is_auto_classified: boolean;
  created_at: string;
}

export interface ClassificationRule {
  id: number;
  concept: string;
  category_id: number;
  category_name: string;
}

export interface ImportResult {
  imported: number;
  skipped_duplicates: number;
  errors: string[];
  /** Populated only on a dry run: the outcome each row would have. */
  preview: ImportPreviewRow[];
}

/** One parsed row as it would land, computed by a dry run without writing. */
export interface ImportPreviewRow {
  date: string;
  /** Amount in cents; negative = expense. */
  amount: number;
  description: string;
  counterparty: string;
  /** The category this row would receive, if any. */
  category: string | null;
  /** True when the category came from a learned classification rule. */
  auto_classified: boolean;
  /** True when importing would create `category` as a brand-new category. */
  new_category: boolean;
  /** True when an identical transaction already exists (row would be skipped). */
  duplicate: boolean;
}

export interface Summary {
  total_balance: number;
  income: number;
  expenses: number;
  account_count: number;
  transaction_count: number;
}

/** One calendar month of income/expense flow for the Trends view. Buckets are
 *  dense: every month in range is present, zero-filled where there's no data. */
export interface MonthlyPoint {
  /** Calendar month as `YYYY-MM`. */
  month: string;
  /** Positive-amount total for the month, in cents. Transfers excluded. */
  income: number;
  /** Negative-amount total for the month, in cents (kept negative). Transfers
   *  excluded. */
  expenses: number;
}

/** Cumulative net worth (a stock, not a flow) at the end of one calendar month:
 *  opening balances plus every transaction up to month end, transfers included.
 *  The final month of an all-time range equals the dashboard's total balance. */
export interface NetWorthPoint {
  /** Calendar month as `YYYY-MM`. */
  month: string;
  /** Cumulative total balance across all accounts at month end, in cents. */
  balance: number;
}

/** Total spend in one category over a period (expenses only, transfers excluded).
 *  `total` is kept negative; uncategorized spend has a null id/name. */
export interface CategorySpend {
  category_id: number | null;
  category_name: string | null;
  /** Sum of negative amounts in this category over the period, in cents. */
  total: number;
}
