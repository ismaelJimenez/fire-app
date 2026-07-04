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
  { id: 7, name: "Groceries" },
  { id: 8, name: "Rent" },
];

function tx(p: Partial<Transaction> & { id: number }): Transaction {
  return {
    account_id: 1,
    account_name: "Checking",
    date: "2026-01-05",
    amount: -1000,
    description: "row",
    category_id: null,
    category_name: null,
    is_internal_transfer: false,
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
    vi.mocked(api.setTransactionCategory).mockResolvedValue(undefined);
    renderTransactions();
    await screen.findByText("Coffee shop");

    const select = within(rowWith("Coffee shop")).getByRole("combobox");
    await userEvent.selectOptions(select, "8");

    await waitFor(() =>
      expect(api.setTransactionCategory).toHaveBeenCalledWith(100, 8),
    );
    expect(refreshAll).toHaveBeenCalled();
  });

  it("toggles a transaction's internal-transfer flag", async () => {
    vi.mocked(api.setInternalTransfer).mockResolvedValue(undefined);
    renderTransactions();
    await screen.findByText("Coffee shop");

    await userEvent.click(
      within(rowWith("Coffee shop")).getByTitle(/internal transfer/i),
    );
    await waitFor(() =>
      expect(api.setInternalTransfer).toHaveBeenCalledWith(100, true),
    );
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
