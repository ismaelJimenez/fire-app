import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { StoreProvider, useStore } from "./store";
import type { Account } from "./types";

vi.mock("./api");
import * as api from "./api";

const account: Account = {
  id: 1,
  name: "Checking",
  parent_id: null,
  created_at: "2026-01-01",
  opening_balance: 0,
  balance: 1000,
  tx_count: 2,
};

function Probe() {
  const { accounts, loading, toasts, toast, dismissToast } = useStore();
  return (
    <div>
      <div data-testid="loading">{String(loading)}</div>
      <div data-testid="accounts">{accounts.length}</div>
      <div data-testid="toasts">
        {toasts.map((t) => `${t.kind}:${t.message}`).join("|")}
      </div>
      <button onClick={() => toast("saved")}>add toast</button>
      {toasts.map((t) => (
        <button key={t.id} onClick={() => dismissToast(t.id)}>
          dismiss {t.id}
        </button>
      ))}
    </div>
  );
}

function renderStore() {
  return render(
    <StoreProvider>
      <Probe />
    </StoreProvider>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.listAccounts).mockResolvedValue([account]);
  vi.mocked(api.listCategories).mockResolvedValue([]);
  vi.mocked(api.getSummary).mockResolvedValue({
    total_balance: 1000,
    income: 1000,
    expenses: 0,
    account_count: 1,
    transaction_count: 2,
  });
});

describe("StoreProvider", () => {
  it("loads all data on mount and clears the loading flag", async () => {
    renderStore();
    await waitFor(() =>
      expect(screen.getByTestId("loading")).toHaveTextContent("false"),
    );
    expect(screen.getByTestId("accounts")).toHaveTextContent("1");
    expect(api.listAccounts).toHaveBeenCalledTimes(1);
    expect(api.getSummary).toHaveBeenCalledTimes(1);
  });

  it("surfaces a load failure as an error toast", async () => {
    vi.mocked(api.listAccounts).mockRejectedValue("boom");
    renderStore();
    await waitFor(() =>
      expect(screen.getByTestId("toasts")).toHaveTextContent("error:boom"),
    );
    // Loading still resolves so the UI isn't stuck.
    expect(screen.getByTestId("loading")).toHaveTextContent("false");
  });

  it("adds and dismisses toasts", async () => {
    renderStore();
    await waitFor(() =>
      expect(screen.getByTestId("loading")).toHaveTextContent("false"),
    );
    const user = userEvent.setup();

    await user.click(screen.getByRole("button", { name: "add toast" }));
    expect(screen.getByTestId("toasts")).toHaveTextContent("success:saved");

    await user.click(screen.getByRole("button", { name: /^dismiss/ }));
    expect(screen.getByTestId("toasts")).toBeEmptyDOMElement();
  });

  it("auto-dismisses a toast after its timeout", async () => {
    vi.useFakeTimers();
    try {
      renderStore();
      // Flush the mount effect's promises under fake timers.
      await vi.waitFor(() =>
        expect(screen.getByTestId("loading")).toHaveTextContent("false"),
      );

      act(() => {
        screen.getByRole("button", { name: "add toast" }).click();
      });
      expect(screen.getByTestId("toasts")).toHaveTextContent("success:saved");

      act(() => {
        vi.advanceTimersByTime(4000);
      });
      expect(screen.getByTestId("toasts")).toBeEmptyDOMElement();
    } finally {
      vi.useRealTimers();
    }
  });
});
