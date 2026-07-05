import { invoke } from "@tauri-apps/api/core";
import type {
  Account,
  Category,
  ClassificationRule,
  ImportResult,
  Summary,
  Transaction,
} from "./types";

// Accounts
export const listAccounts = () => invoke<Account[]>("list_accounts");
export const createAccount = (name: string) =>
  invoke<number>("create_account", { name });
export const renameAccount = (id: number, name: string) =>
  invoke<void>("rename_account", { id, name });
/** Set an account's starting balance (cents); pass 0 to clear it. */
export const setAccountOpeningBalance = (id: number, openingBalance: number) =>
  invoke<void>("set_account_opening_balance", { id, openingBalance });
export const deleteAccount = (id: number) =>
  invoke<void>("delete_account", { id });
export const addSubaccount = (parentId: number, name: string) =>
  invoke<number>("add_subaccount", { parentId, name });

// Categories
export const listCategories = () => invoke<Category[]>("list_categories");
export const createCategory = (name: string) =>
  invoke<number>("create_category", { name });
export const deleteCategory = (id: number) =>
  invoke<void>("delete_category", { id });

// Transactions
export const listTransactions = (
  accountId: number | null,
  search: string,
  limit?: number,
) =>
  invoke<Transaction[]>("list_transactions", {
    accountId,
    search: search || null,
    limit: limit ?? null,
  });

export interface TxInput {
  account_id: number;
  date: string;
  amount: number;
  description: string;
  category_id: number | null;
}

export const createTransaction = (t: TxInput) =>
  invoke<number>("create_transaction", {
    accountId: t.account_id,
    date: t.date,
    amount: t.amount,
    description: t.description,
    categoryId: t.category_id,
  });

export const updateTransaction = (id: number, t: TxInput) =>
  invoke<void>("update_transaction", {
    id,
    accountId: t.account_id,
    date: t.date,
    amount: t.amount,
    description: t.description,
    categoryId: t.category_id,
  });

/** Sets the category and learns/propagates a rule; resolves to how many other
 *  (unverified) transactions with the same concept were re-classified. */
export const setTransactionCategory = (id: number, categoryId: number | null) =>
  invoke<number>("set_transaction_category", { id, categoryId });

export const setTransactionVerified = (id: number, verified: boolean) =>
  invoke<void>("set_transaction_verified", { id, verified });

export const deleteTransaction = (id: number) =>
  invoke<void>("delete_transaction", { id });

// Classification rules
export const listRules = () => invoke<ClassificationRule[]>("list_rules");
export const deleteRule = (id: number) => invoke<void>("delete_rule", { id });

// Summary + import
export const getSummary = () => invoke<Summary>("get_summary");
export const importCsv = (accountId: number, csvText: string, dryRun = false) =>
  invoke<ImportResult>("import_csv", { accountId, csvText, dryRun });
export const detectBank = (csvText: string) =>
  invoke<string>("detect_bank", { csvText });
