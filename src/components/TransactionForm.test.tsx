import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TransactionForm } from "./TransactionForm";
import type { Account, Category, Transaction } from "../types";

// Isolate the form from the Tauri bridge and the global store.
vi.mock("../api");
vi.mock("../store", () => ({ useStore: () => mockStore }));

import * as api from "../api";

const accounts: Account[] = [
  {
    id: 1,
    name: "Checking",
    parent_id: null,
    created_at: "2026-01-01",
    opening_balance: 0,
    balance: 0,
    tx_count: 0,
  },
  {
    id: 2,
    name: "Savings",
    parent_id: null,
    created_at: "2026-01-01",
    opening_balance: 0,
    balance: 0,
    tx_count: 0,
  },
];
const categories: Category[] = [
  { id: 7, name: "Groceries", is_transfer: false },
];

const toast = vi.fn();
const mockStore = { accounts, categories, toast };

beforeEach(() => {
  vi.clearAllMocks();
});

function renderNew() {
  const onSaved = vi.fn();
  const onClose = vi.fn();
  render(
    <TransactionForm
      tx={null}
      defaultAccountId={1}
      onClose={onClose}
      onSaved={onSaved}
    />,
  );
  return { onSaved, onClose };
}

describe("TransactionForm (create)", () => {
  it("saves an expense as a negative cents amount", async () => {
    vi.mocked(api.createTransaction).mockResolvedValue(1);
    const { onSaved, onClose } = renderNew();
    const user = userEvent.setup();

    await user.type(screen.getByPlaceholderText("0.00"), "42.90");
    await user.click(screen.getByRole("button", { name: /add transaction/i }));

    await waitFor(() => expect(api.createTransaction).toHaveBeenCalledTimes(1));
    expect(api.createTransaction).toHaveBeenCalledWith(
      expect.objectContaining({ account_id: 1, amount: -4290 }),
    );
    expect(onSaved).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  it("saves income as a positive amount when the sign is +", async () => {
    vi.mocked(api.createTransaction).mockResolvedValue(1);
    renderNew();
    const user = userEvent.setup();

    // Comboboxes in DOM order: [account, sign, category]; the sign picker is #1.
    const signSelect = screen.getAllByRole("combobox")[1];
    await user.selectOptions(signSelect, "+");
    await user.type(screen.getByPlaceholderText("0.00"), "1500");
    await user.click(screen.getByRole("button", { name: /add transaction/i }));

    await waitFor(() =>
      expect(api.createTransaction).toHaveBeenCalledWith(
        expect.objectContaining({ amount: 150000 }),
      ),
    );
  });

  it("rejects an empty amount without calling the backend", async () => {
    renderNew();
    const user = userEvent.setup();

    await user.click(screen.getByRole("button", { name: /add transaction/i }));

    expect(api.createTransaction).not.toHaveBeenCalled();
    expect(toast).toHaveBeenCalledWith(
      expect.stringMatching(/valid non-zero amount/i),
      "error",
    );
  });
});

describe("TransactionForm (edit)", () => {
  const tx: Transaction = {
    id: 9,
    account_id: 1,
    account_name: "Checking",
    date: "2026-01-05",
    amount: -4290,
    description: "Corner store",
    counterparty: "",
    category_id: 7,
    category_name: "Groceries",
    is_verified: false,
    is_auto_classified: false,
    created_at: "2026-01-05",
  };

  it("persists a moved account when editing", async () => {
    vi.mocked(api.updateTransaction).mockResolvedValue();
    const onSaved = vi.fn();
    const onClose = vi.fn();
    render(
      <TransactionForm
        tx={tx}
        defaultAccountId={1}
        onClose={onClose}
        onSaved={onSaved}
      />,
    );
    const user = userEvent.setup();

    // Move the transaction from Checking to Savings, then save.
    const accountSelect = screen.getAllByRole("combobox")[0];
    await user.selectOptions(accountSelect, "2");
    await user.click(screen.getByRole("button", { name: /save changes/i }));

    await waitFor(() => expect(api.updateTransaction).toHaveBeenCalledTimes(1));
    expect(api.updateTransaction).toHaveBeenCalledWith(
      9,
      expect.objectContaining({ account_id: 2, amount: -4290 }),
    );
    expect(onSaved).toHaveBeenCalled();
  });
});
