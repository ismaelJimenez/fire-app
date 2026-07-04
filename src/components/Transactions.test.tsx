import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Transactions } from "./Transactions";
import type { Account, Category, Transaction } from "../types";

vi.mock("../api");
vi.mock("../store", () => ({ useStore: () => mockStore }));

import * as api from "../api";

const accounts: Account[] = [
  {
    id: 1,
    name: "Checking",
    parent_id: null,
    created_at: "2026-01-01",
    balance: 0,
    tx_count: 0,
  },
];
const categories: Category[] = [
  { id: 7, name: "Groceries", is_transfer: false },
  { id: 8, name: "Rent", is_transfer: false },
  { id: 9, name: "Transfer", is_transfer: true },
];

function tx(p: Partial<Transaction> & { id: number }): Transaction {
  return {
    account_id: 1,
    account_name: "Checking",
    date: "2026-01-05",
    amount: -1000,
    description: "row",
    counterparty: "",
    category_id: null,
    category_name: null,
    is_verified: false,
    is_auto_classified: false,
    created_at: "2026-01-05",
    ...p,
  };
}

const rows: Transaction[] = [
  tx({ id: 100, description: "Coffee shop", amount: -450 }),
  tx({ id: 101, description: "Rent payment", amount: -80000 }),
];

const refreshAll = vi.fn().mockResolvedValue(undefined);
const mockStore = { accounts, categories, refreshAll, toast: vi.fn() };
const onSelectAccount = vi.fn();

function renderTransactions(accountId: number | null = null) {
  render(
    <Transactions accountId={accountId} onSelectAccount={onSelectAccount} />,
  );
}

/** Find the table row that contains the given text. */
function rowWith(text: string): HTMLElement {
  return screen.getByText(text).closest("tr") as HTMLElement;
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.listTransactions).mockResolvedValue(rows);
});

describe("Transactions", () => {
  it("loads and lists transactions with a count", async () => {
    renderTransactions();
    expect(await screen.findByText("Coffee shop")).toBeInTheDocument();
    expect(screen.getByText("Rent payment")).toBeInTheDocument();
    expect(screen.getByText(/2 shown/)).toBeInTheDocument();
    expect(api.listTransactions).toHaveBeenCalledWith(null, "");
  });

  it("shows an empty state when there are no transactions", async () => {
    vi.mocked(api.listTransactions).mockResolvedValue([]);
    renderTransactions();
    expect(await screen.findByText("No transactions yet")).toBeInTheDocument();
  });

  it("changes a transaction's category inline", async () => {
    vi.mocked(api.setTransactionCategory).mockResolvedValue(0);
    renderTransactions();
    await screen.findByText("Coffee shop");

    const select = within(rowWith("Coffee shop")).getByRole("combobox");
    await userEvent.selectOptions(select, "8");

    await waitFor(() =>
      expect(api.setTransactionCategory).toHaveBeenCalledWith(100, 8),
    );
    expect(refreshAll).toHaveBeenCalled();
  });

  it("reports how many matching transactions a learned rule swept in", async () => {
    vi.mocked(api.setTransactionCategory).mockResolvedValue(3);
    renderTransactions();
    await screen.findByText("Coffee shop");

    await userEvent.selectOptions(
      within(rowWith("Coffee shop")).getByRole("combobox"),
      "8",
    );
    await waitFor(() =>
      expect(mockStore.toast).toHaveBeenCalledWith(
        expect.stringContaining("3"),
      ),
    );
  });

  it("hides the unverified warning and locks the category select when verified", async () => {
    vi.mocked(api.listTransactions).mockResolvedValue([
      tx({
        id: 200,
        description: "Salary",
        is_verified: true,
        category_id: 7,
        category_name: "Groceries",
      }),
    ]);
    renderTransactions();
    await screen.findByText("Salary");

    const row = rowWith("Salary");
    expect(within(row).queryByText(/unverified/i)).not.toBeInTheDocument();
    expect(within(row).getByRole("combobox")).toBeDisabled();
  });

  it("marks a transaction as verified", async () => {
    vi.mocked(api.setTransactionVerified).mockResolvedValue(undefined);
    renderTransactions();
    await screen.findByText("Coffee shop");

    await userEvent.click(
      within(rowWith("Coffee shop")).getByTitle(/mark as verified/i),
    );
    await waitFor(() =>
      expect(api.setTransactionVerified).toHaveBeenCalledWith(100, true),
    );
  });

  it("badges rows in the transfer category, identified by its id not its name", async () => {
    vi.mocked(api.listTransactions).mockResolvedValue([
      tx({ id: 100, description: "Coffee shop", amount: -450 }),
      tx({
        id: 102,
        description: "To savings",
        amount: -80000,
        category_id: 9, // the is_transfer category, even though renamed below
        category_name: "Umbuchung",
      }),
    ]);
    renderTransactions();
    await screen.findByText("To savings");

    expect(
      within(rowWith("To savings")).getByText(/⇄ transfer/i),
    ).toBeInTheDocument();
    expect(
      within(rowWith("Coffee shop")).queryByText(/⇄ transfer/i),
    ).not.toBeInTheDocument();
  });

  it("filters by account through the toolbar dropdown", async () => {
    renderTransactions();
    await screen.findByText("Coffee shop");

    // The first combobox is the account filter (row category pickers follow).
    const filter = screen.getAllByRole("combobox")[0];
    await userEvent.selectOptions(filter, "1");
    expect(onSelectAccount).toHaveBeenCalledWith(1);
  });

  it("deletes a transaction via the edit form and confirmation", async () => {
    vi.mocked(api.deleteTransaction).mockResolvedValue(undefined);
    renderTransactions();
    await screen.findByText("Coffee shop");
    const user = userEvent.setup();

    // Open the edit form for the first row...
    await user.click(within(rowWith("Coffee shop")).getByTitle("Edit"));
    // ...request deletion, which swaps in the confirmation dialog...
    await user.click(screen.getByRole("button", { name: /🗑 Delete/ }));
    // ...and confirm.
    await user.click(screen.getByRole("button", { name: "Delete" }));

    await waitFor(() =>
      expect(api.deleteTransaction).toHaveBeenCalledWith(100),
    );
  });
});
