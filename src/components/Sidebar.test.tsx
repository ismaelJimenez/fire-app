import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Sidebar } from "./Sidebar";
import { formatMoney } from "../format";
import type { Account } from "../types";

vi.mock("../api");
vi.mock("../store", () => ({ useStore: () => mockStore }));

import * as api from "../api";

function acc(p: Partial<Account> & { id: number; name: string }): Account {
  return {
    parent_id: null,
    created_at: "2026-01-01",
    balance: 0,
    tx_count: 0,
    ...p,
  };
}

// Checking (own 100.00) with a Savings subaccount (own 50.00).
const accounts: Account[] = [
  acc({ id: 1, name: "Checking", balance: 10000, tx_count: 2 }),
  acc({ id: 2, name: "Savings", parent_id: 1, balance: 5000, tx_count: 1 }),
];

const refreshAll = vi.fn().mockResolvedValue(undefined);
const toast = vi.fn();
const mockStore = { accounts, refreshAll, toast };

const onNavigate = vi.fn();

function renderSidebar() {
  render(
    <Sidebar
      view="dashboard"
      onNavigate={onNavigate}
      selectedAccountId={null}
    />,
  );
}

beforeEach(() => vi.clearAllMocks());

describe("Sidebar", () => {
  it("renders the account tree with rolled-up balances", () => {
    renderSidebar();
    // Exact match on the balance spans (parent first, then its child), since
    // "150.00" contains "50.00" as a substring.
    const balances = [...document.querySelectorAll(".acc-balance")].map(
      (b) => b.textContent,
    );
    expect(balances[0]).toBe(formatMoney(15000)); // Checking: 100.00 + 50.00
    expect(balances[1]).toBe(formatMoney(5000)); // Savings: 50.00
    expect(
      screen.getByRole("button", { name: /Checking/ }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Savings/ })).toBeInTheDocument();
  });

  it("navigates to the account's transactions when its name is clicked", async () => {
    renderSidebar();
    await userEvent.click(screen.getByRole("button", { name: /Checking/ }));
    expect(onNavigate).toHaveBeenCalledWith("transactions", 1);
  });

  it("creates a new account through the modal", async () => {
    vi.mocked(api.createAccount).mockResolvedValue(3);
    renderSidebar();
    const user = userEvent.setup();

    // The section "+" button carries its label via the title attribute.
    await user.click(screen.getByTitle("New account"));
    const dialog = screen
      .getByText("New account")
      .closest(".modal") as HTMLElement;
    await user.type(within(dialog).getByRole("textbox"), "Credit Card");
    await user.click(within(dialog).getByRole("button", { name: "Create" }));

    await waitFor(() =>
      expect(api.createAccount).toHaveBeenCalledWith("Credit Card"),
    );
    expect(refreshAll).toHaveBeenCalled();
  });
});
