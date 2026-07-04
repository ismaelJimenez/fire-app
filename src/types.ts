export type View = "dashboard" | "transactions" | "import";

export interface Account {
  id: number;
  name: string;
  /** Parent account this is a subaccount of, or null for a top-level account. */
  parent_id: number | null;
  created_at: string;
  /** This account's own balance in cents (excludes subaccounts). */
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
}

export interface Summary {
  total_balance: number;
  income: number;
  expenses: number;
  account_count: number;
  transaction_count: number;
}
