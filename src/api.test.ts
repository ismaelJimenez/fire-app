import { describe, it, expect, vi, beforeEach } from "vitest";
import { readFileSync } from "node:fs";

// Mock the Tauri bridge so we can assert exactly what each wrapper sends.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(() => Promise.resolve()),
}));

import { invoke } from "@tauri-apps/api/core";
import * as api from "./api";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => mockInvoke.mockClear());

describe("api command/argument mapping", () => {
  // Each wrapper must call the exact Rust command name with camelCase arg keys.
  // A mismatch here fails only at runtime in the real app, so pin it.

  it("maps account commands", () => {
    api.listAccounts();
    expect(mockInvoke).toHaveBeenLastCalledWith("list_accounts");

    api.createAccount("Checking");
    expect(mockInvoke).toHaveBeenLastCalledWith("create_account", {
      name: "Checking",
    });

    api.renameAccount(3, "Savings");
    expect(mockInvoke).toHaveBeenLastCalledWith("rename_account", {
      id: 3,
      name: "Savings",
    });

    api.deleteAccount(3);
    expect(mockInvoke).toHaveBeenLastCalledWith("delete_account", { id: 3 });

    api.addSubaccount(1, "Vacation");
    expect(mockInvoke).toHaveBeenLastCalledWith("add_subaccount", {
      parentId: 1,
      name: "Vacation",
    });
  });

  it("maps category commands", () => {
    api.listCategories();
    expect(mockInvoke).toHaveBeenLastCalledWith("list_categories");

    api.createCategory("Coffee");
    expect(mockInvoke).toHaveBeenLastCalledWith("create_category", {
      name: "Coffee",
    });

    api.deleteCategory(5);
    expect(mockInvoke).toHaveBeenLastCalledWith("delete_category", { id: 5 });
  });

  it("maps transaction commands with camelCase keys", () => {
    api.listTransactions(2, "coffee");
    expect(mockInvoke).toHaveBeenLastCalledWith("list_transactions", {
      accountId: 2,
      search: "coffee",
    });

    // An empty search string is normalized to null.
    api.listTransactions(null, "");
    expect(mockInvoke).toHaveBeenLastCalledWith("list_transactions", {
      accountId: null,
      search: null,
    });

    const tx = {
      account_id: 1,
      date: "2026-01-01",
      amount: -4290,
      description: "Groceries",
      category_id: 7,
    };
    api.createTransaction(tx);
    expect(mockInvoke).toHaveBeenLastCalledWith("create_transaction", {
      accountId: 1,
      date: "2026-01-01",
      amount: -4290,
      description: "Groceries",
      categoryId: 7,
    });

    api.updateTransaction(9, tx);
    expect(mockInvoke).toHaveBeenLastCalledWith("update_transaction", {
      id: 9,
      date: "2026-01-01",
      amount: -4290,
      description: "Groceries",
      categoryId: 7,
    });

    api.setTransactionCategory(9, null);
    expect(mockInvoke).toHaveBeenLastCalledWith("set_transaction_category", {
      id: 9,
      categoryId: null,
    });

    api.deleteTransaction(9);
    expect(mockInvoke).toHaveBeenLastCalledWith("delete_transaction", {
      id: 9,
    });
  });

  it("maps summary and import commands", () => {
    api.getSummary();
    expect(mockInvoke).toHaveBeenLastCalledWith("get_summary");

    api.importCsv(4, "date,amount\n2026-01-01,-1.00\n");
    expect(mockInvoke).toHaveBeenLastCalledWith("import_csv", {
      accountId: 4,
      csvText: "date,amount\n2026-01-01,-1.00\n",
    });
  });

  it("only invokes commands the Rust backend actually registers", () => {
    // Vitest runs from the project root.
    const root = process.cwd();
    const apiSrc = readFileSync(`${root}/src/api.ts`, "utf8");
    const libSrc = readFileSync(`${root}/src-tauri/src/lib.rs`, "utf8");

    // Command names used on the front end...
    const used = [...apiSrc.matchAll(/invoke<[^>]*>\("([a-z_]+)"/g)].map(
      (m) => m[1],
    );
    // ...vs. names registered in tauri::generate_handler![...].
    const registered = new Set(
      [...libSrc.matchAll(/commands::([a-z_]+)/g)].map((m) => m[1]),
    );

    expect(used.length).toBeGreaterThan(0);
    const missing = used.filter((cmd) => !registered.has(cmd));
    expect(missing, `commands invoked but not registered in Rust`).toEqual([]);
  });
});
