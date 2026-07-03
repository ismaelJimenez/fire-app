import { invoke } from "@tauri-apps/api/core";
import type {
  Account,
  Category,
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
export const deleteAccount = (id: number) =>
  invoke<void>("delete_account", { id });

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
) =>
  invoke<Transaction[]>("list_transactions", {
    accountId,
    search: search || null,
  });

export interface TxInput {
  account_id: number;
  date: string;
  amount: number;
  description: string;
  category_id: number | null;
  is_internal_transfer: boolean;
}

export const createTransaction = (t: TxInput) =>
  invoke<number>("create_transaction", {
    accountId: t.account_id,
    date: t.date,
    amount: t.amount,
    description: t.description,
    categoryId: t.category_id,
    isInternalTransfer: t.is_internal_transfer,
  });

export const updateTransaction = (id: number, t: TxInput) =>
  invoke<void>("update_transaction", {
    id,
    date: t.date,
    amount: t.amount,
    description: t.description,
    categoryId: t.category_id,
    isInternalTransfer: t.is_internal_transfer,
  });

export const setTransactionCategory = (id: number, categoryId: number | null) =>
  invoke<void>("set_transaction_category", { id, categoryId });

export const setInternalTransfer = (id: number, value: boolean) =>
  invoke<void>("set_internal_transfer", { id, isInternalTransfer: value });

export const deleteTransaction = (id: number) =>
  invoke<void>("delete_transaction", { id });

// Summary + import
export const getSummary = () => invoke<Summary>("get_summary");
export const importCsv = (accountId: number, csvText: string) =>
  invoke<ImportResult>("import_csv", { accountId, csvText });
