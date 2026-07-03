export type View = "dashboard" | "transactions" | "import";

export interface Account {
  id: number;
  name: string;
  created_at: string;
  /** Balance in cents. */
  balance: number;
  tx_count: number;
}

export interface Category {
  id: number;
  name: string;
}

export interface Transaction {
  id: number;
  account_id: number;
  account_name: string;
  date: string;
  /** Amount in cents; negative = expense. */
  amount: number;
  description: string;
  category_id: number | null;
  category_name: string | null;
  is_internal_transfer: boolean;
  created_at: string;
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
