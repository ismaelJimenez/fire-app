import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Dashboard } from "./Dashboard";
import { formatMoney } from "../format";
import type { Account, Summary, Transaction } from "../types";

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
    balance: 123456,
    tx_count: 3,
  },
];

const summary: Summary = {
  total_balance: 123456,
  income: 200000,
  expenses: -50000,
  account_count: 1,
  transaction_count: 7,
};

const recent: Transaction[] = [
  {
    id: 10,
    account_id: 1,
    account_name: "Checking",
    date: "2026-01-05",
    amount: -4290,
    description: "Corner store",
    counterparty: "",
    category_id: null,
    category_name: null,
    is_verified: false,
    is_auto_classified: false,
    created_at: "2026-01-05",
  },
];

let mockStore: {
  accounts: Account[];
  summary: Summary | null;
  toast: ReturnType<typeof vi.fn>;
};
const onNavigate = vi.fn();

beforeEach(() => {
  vi.clearAllMocks();
  mockStore = { accounts, summary, toast: vi.fn() };
  vi.mocked(api.listTransactions).mockResolvedValue(recent);
});

describe("Dashboard", () => {
  it("shows a welcome/empty state when there are no accounts", async () => {
    mockStore.accounts = [];
    render(<Dashboard onNavigate={onNavigate} />);

    expect(screen.getByText("Welcome to Fire")).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: /go to import/i }),
    );
    expect(onNavigate).toHaveBeenCalledWith("import");
  });

  it("renders summary stats and recent activity", async () => {
    render(<Dashboard onNavigate={onNavigate} />);

    // Scope to the stat cards; the balance value also appears in the accounts
    // table for this single-account fixture.
    const statValues = [...document.querySelectorAll(".stat .value")].map(
      (v) => v.textContent,
    );
    expect(statValues).toContain(formatMoney(123456)); // balance
    expect(statValues).toContain(formatMoney(200000)); // income
    expect(statValues).toContain(formatMoney(-50000)); // expenses
    expect(statValues).toContain("7"); // transaction count

    // Recent activity is loaded from the backend on mount.
    expect(await screen.findByText("Corner store")).toBeInTheDocument();
    expect(api.listTransactions).toHaveBeenCalledWith(null, "");
  });

  it("navigates to an account's transactions from the accounts table", async () => {
    render(<Dashboard onNavigate={onNavigate} />);
    await userEvent.click(screen.getByText("Checking"));
    expect(onNavigate).toHaveBeenCalledWith("transactions", 1);
  });

  it("surfaces a load failure as an error toast", async () => {
    vi.mocked(api.listTransactions).mockRejectedValue("network down");
    render(<Dashboard onNavigate={onNavigate} />);
    await waitFor(() =>
      expect(mockStore.toast).toHaveBeenCalledWith("network down", "error"),
    );
  });
});
