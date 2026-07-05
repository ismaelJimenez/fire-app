import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Trends } from "./Trends";
import { formatMoney } from "../format";
import type {
  Account,
  CategorySpend,
  MonthlyPoint,
  NetWorthPoint,
} from "../types";

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
    balance: 600000,
    tx_count: 5,
  },
];

const monthly: MonthlyPoint[] = [
  { month: "2026-06", income: 100000, expenses: -40000 },
  { month: "2026-07", income: 200000, expenses: -60000 },
];
const networth: NetWorthPoint[] = [
  { month: "2026-06", balance: 500000 },
  { month: "2026-07", balance: 600000 },
];
const categories: CategorySpend[] = [
  { category_id: 1, category_name: "Rent", total: -90000 },
  { category_id: null, category_name: null, total: -10000 },
];

let mockStore: {
  accounts: Account[];
  toast: ReturnType<typeof vi.fn>;
};
const onNavigate = vi.fn();

beforeEach(() => {
  vi.clearAllMocks();
  mockStore = { accounts, toast: vi.fn() };
  vi.mocked(api.monthlySeries).mockResolvedValue(monthly);
  vi.mocked(api.networthSeries).mockResolvedValue(networth);
  vi.mocked(api.categoryBreakdown).mockResolvedValue(categories);
});

describe("Trends", () => {
  it("shows an empty state when there are no accounts", async () => {
    mockStore.accounts = [];
    render(<Trends onNavigate={onNavigate} />);
    expect(screen.getByText("Nothing to chart yet")).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: /go to import/i }),
    );
    expect(onNavigate).toHaveBeenCalledWith("import");
  });

  it("derives period KPIs from the monthly series", async () => {
    render(<Trends onNavigate={onNavigate} />);
    await screen.findByText("Income vs. expenses");

    const values = [...document.querySelectorAll(".stat .value")].map(
      (v) => v.textContent,
    );
    expect(values).toContain(formatMoney(300000)); // income 100k + 200k
    expect(values).toContain(formatMoney(-100000)); // expenses -40k + -60k
    expect(values).toContain(formatMoney(200000)); // net savings
    expect(values).toContain("67%"); // savings rate 200k/300k
    expect(values).toContain(formatMoney(150000)); // mean income / 2 months
    expect(values).toContain(formatMoney(-50000)); // mean expense / 2 months
    expect(values).toContain(formatMoney(100000)); // mean savings / 2 months
  });

  it("shows category share (%) and mean spend per month, not the absolute", async () => {
    render(<Trends onNavigate={onNavigate} />);
    expect(await screen.findByText("Rent")).toBeInTheDocument();
    expect(screen.getByText("Uncategorized")).toBeInTheDocument();

    // Rent is -90k of -100k total spend → 90% share; over 2 months the mean is
    // -45k (not the -90k absolute).
    expect(screen.getByText("90%")).toBeInTheDocument();
    expect(screen.getByText("10%")).toBeInTheDocument();
    expect(screen.getByText(formatMoney(-45000))).toBeInTheDocument();
    expect(screen.getByText(formatMoney(-5000))).toBeInTheDocument();
    // The absolute total is no longer shown.
    expect(screen.queryByText(formatMoney(-90000))).not.toBeInTheDocument();
  });

  it("shows a dash for savings rate when there is no income", async () => {
    vi.mocked(api.monthlySeries).mockResolvedValue([
      { month: "2026-07", income: 0, expenses: -5000 },
    ]);
    render(<Trends onNavigate={onNavigate} />);
    await screen.findByText("Income vs. expenses");
    const values = [...document.querySelectorAll(".stat .value")].map(
      (v) => v.textContent,
    );
    expect(values).toContain("—");
  });

  it("refetches with an unbounded range when All time is selected", async () => {
    render(<Trends onNavigate={onNavigate} />);
    await screen.findByText("Income vs. expenses");

    await userEvent.click(screen.getByRole("tab", { name: /all time/i }));
    await waitFor(() =>
      expect(api.monthlySeries).toHaveBeenLastCalledWith(null, null),
    );
    expect(api.networthSeries).toHaveBeenLastCalledWith(null, null);
    expect(api.categoryBreakdown).toHaveBeenLastCalledWith(null, null);
  });

  it("shows a no-data state when the period has no transactions", async () => {
    vi.mocked(api.monthlySeries).mockResolvedValue([]);
    render(<Trends onNavigate={onNavigate} />);
    expect(
      await screen.findByText("No transactions in this period"),
    ).toBeInTheDocument();
  });

  it("surfaces a load failure as an error toast", async () => {
    vi.mocked(api.monthlySeries).mockRejectedValue("boom");
    render(<Trends onNavigate={onNavigate} />);
    await waitFor(() =>
      expect(mockStore.toast).toHaveBeenCalledWith("boom", "error"),
    );
  });
});
