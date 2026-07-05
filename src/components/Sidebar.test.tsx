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
    opening_balance: 0,
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

beforeEach(() => {
  vi.clearAllMocks();
  // Some tests swap in a custom account set; restore the default each time.
  mockStore.accounts = accounts;
});

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
    await user.type(
      within(dialog).getByPlaceholderText(/Checking, Savings, Credit Card/),
      "Credit Card",
    );
    await user.click(within(dialog).getByRole("button", { name: "Create" }));

    await waitFor(() =>
      expect(api.createAccount).toHaveBeenCalledWith("Credit Card"),
    );
    // No starting balance entered, so it is left untouched.
    expect(api.setAccountOpeningBalance).not.toHaveBeenCalled();
    expect(refreshAll).toHaveBeenCalled();
  });

  it("sets a starting balance when creating an account", async () => {
    vi.mocked(api.createAccount).mockResolvedValue(3);
    renderSidebar();
    const user = userEvent.setup();

    await user.click(screen.getByTitle("New account"));
    const dialog = screen
      .getByText("New account")
      .closest(".modal") as HTMLElement;
    await user.type(
      within(dialog).getByPlaceholderText(/Checking, Savings, Credit Card/),
      "Credit Card",
    );
    await user.type(within(dialog).getByPlaceholderText("0.00"), "1234.56");
    await user.click(within(dialog).getByRole("button", { name: "Create" }));

    await waitFor(() =>
      expect(api.setAccountOpeningBalance).toHaveBeenCalledWith(3, 123456),
    );
  });

  it("prefills and updates an existing account's starting balance", async () => {
    renderSidebar();
    const user = userEvent.setup();

    // Edit "Checking" (own balance 100.00, opening 0). Open its edit modal.
    const checkingRow = screen
      .getByRole("button", { name: /Checking/ })
      .closest(".account-row") as HTMLElement;
    await user.click(within(checkingRow).getByTitle("Edit"));

    const dialog = screen
      .getByText("Edit account")
      .closest(".modal") as HTMLElement;
    const opening = within(dialog).getByPlaceholderText("0.00");
    await user.type(opening, "500");
    await user.click(within(dialog).getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(api.setAccountOpeningBalance).toHaveBeenCalledWith(1, 50000),
    );
  });

  it("adds a subaccount through the modal", async () => {
    vi.mocked(api.addSubaccount).mockResolvedValue(9);
    renderSidebar();
    const user = userEvent.setup();

    const checkingRow = screen
      .getByRole("button", { name: /Checking/ })
      .closest(".account-row") as HTMLElement;
    await user.click(within(checkingRow).getByTitle("Add subaccount"));

    const dialog = screen
      .getByText(/Add subaccount to/)
      .closest(".modal") as HTMLElement;
    await user.type(
      within(dialog).getByPlaceholderText(/Checking, Savings, Brokerage/),
      "Brokerage",
    );
    await user.click(
      within(dialog).getByRole("button", { name: "Add subaccount" }),
    );

    await waitFor(() =>
      expect(api.addSubaccount).toHaveBeenCalledWith(1, "Brokerage"),
    );
    // No starting balance entered, so it is left untouched.
    expect(api.setAccountOpeningBalance).not.toHaveBeenCalled();
    expect(refreshAll).toHaveBeenCalled();
  });

  it("sets a starting balance on a new subaccount", async () => {
    vi.mocked(api.addSubaccount).mockResolvedValue(9);
    renderSidebar();
    const user = userEvent.setup();

    const checkingRow = screen
      .getByRole("button", { name: /Checking/ })
      .closest(".account-row") as HTMLElement;
    await user.click(within(checkingRow).getByTitle("Add subaccount"));

    const dialog = screen
      .getByText(/Add subaccount to/)
      .closest(".modal") as HTMLElement;
    await user.type(
      within(dialog).getByPlaceholderText(/Checking, Savings, Brokerage/),
      "Brokerage",
    );
    await user.type(within(dialog).getByPlaceholderText("0.00"), "250.50");
    await user.click(
      within(dialog).getByRole("button", { name: "Add subaccount" }),
    );

    await waitFor(() =>
      expect(api.setAccountOpeningBalance).toHaveBeenCalledWith(9, 25050),
    );
  });

  it("rejects an invalid starting balance instead of saving", async () => {
    renderSidebar();
    const user = userEvent.setup();

    await user.click(screen.getByTitle("New account"));
    const dialog = screen
      .getByText("New account")
      .closest(".modal") as HTMLElement;
    await user.type(
      within(dialog).getByPlaceholderText(/Checking, Savings, Credit Card/),
      "Credit Card",
    );
    await user.type(
      within(dialog).getByPlaceholderText("0.00"),
      "not-a-number",
    );
    await user.click(within(dialog).getByRole("button", { name: "Create" }));

    await waitFor(() =>
      expect(toast).toHaveBeenCalledWith(
        expect.stringMatching(/valid starting balance/),
        "error",
      ),
    );
    expect(api.createAccount).not.toHaveBeenCalled();
  });

  it("clears the starting balance when the field is emptied on rename", async () => {
    // Checking already carries a 200.00 opening balance; blanking the field
    // resets it to zero rather than leaving it untouched.
    mockStore.accounts = [
      acc({
        id: 1,
        name: "Checking",
        balance: 10000,
        opening_balance: 20000,
        tx_count: 2,
      }),
    ];
    renderSidebar();
    const user = userEvent.setup();

    const checkingRow = screen
      .getByRole("button", { name: /Checking/ })
      .closest(".account-row") as HTMLElement;
    await user.click(within(checkingRow).getByTitle("Edit"));

    const dialog = screen
      .getByText("Edit account")
      .closest(".modal") as HTMLElement;
    await user.clear(within(dialog).getByPlaceholderText("0.00"));
    await user.click(within(dialog).getByRole("button", { name: "Save" }));

    await waitFor(() =>
      expect(api.setAccountOpeningBalance).toHaveBeenCalledWith(1, 0),
    );
  });

  it("surfaces a backend error as a toast", async () => {
    vi.mocked(api.createAccount).mockRejectedValue("name already exists");
    renderSidebar();
    const user = userEvent.setup();

    await user.click(screen.getByTitle("New account"));
    const dialog = screen
      .getByText("New account")
      .closest(".modal") as HTMLElement;
    await user.type(
      within(dialog).getByPlaceholderText(/Checking, Savings, Credit Card/),
      "Checking",
    );
    await user.click(within(dialog).getByRole("button", { name: "Create" }));

    await waitFor(() =>
      expect(toast).toHaveBeenCalledWith("name already exists", "error"),
    );
    expect(refreshAll).not.toHaveBeenCalled();
  });

  it("deletes an account, warning about its subaccounts and navigating away", async () => {
    vi.mocked(api.deleteAccount).mockResolvedValue(undefined);
    // Rendered with Checking selected, so deleting it must navigate away.
    render(
      <Sidebar
        view="transactions"
        onNavigate={onNavigate}
        selectedAccountId={1}
      />,
    );
    const user = userEvent.setup();

    const checkingRow = screen
      .getByRole("button", { name: /Checking/ })
      .closest(".account-row") as HTMLElement;
    await user.click(within(checkingRow).getByTitle("Edit"));
    // The edit modal's delete button hands off to the confirm modal.
    await user.click(screen.getByRole("button", { name: /🗑 Delete/ }));

    const dialog = screen
      .getByText("Delete account")
      .closest(".modal") as HTMLElement;
    // Checking has one subaccount (Savings) and 3 rolled-up transactions.
    expect(dialog.textContent).toMatch(/1 subaccount/);
    expect(dialog.textContent).toMatch(/3 transaction/);
    await user.click(within(dialog).getByRole("button", { name: "Delete" }));

    await waitFor(() => expect(api.deleteAccount).toHaveBeenCalledWith(1));
    expect(onNavigate).toHaveBeenCalledWith("transactions", null);
    expect(refreshAll).toHaveBeenCalled();
  });

  it("shows a plain confirmation for a leaf account with no subaccounts", async () => {
    renderSidebar();
    const user = userEvent.setup();

    // Savings (id 2) is a leaf with a single transaction.
    const savingsRow = screen
      .getByRole("button", { name: /Savings/ })
      .closest(".account-row") as HTMLElement;
    await user.click(within(savingsRow).getByTitle("Edit"));
    await user.click(screen.getByRole("button", { name: /🗑 Delete/ }));

    const dialog = screen
      .getByText("Delete account")
      .closest(".modal") as HTMLElement;
    expect(dialog.textContent).not.toMatch(/subaccount/);
    expect(dialog.textContent).toMatch(/1 transaction/);
  });
});
