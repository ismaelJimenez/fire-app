import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Import } from "./Import";
import type { Account } from "../types";

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

const refreshAll = vi.fn().mockResolvedValue(undefined);
const toast = vi.fn();
const mockStore = { accounts, refreshAll, toast };
const onNavigate = vi.fn();

function renderImport() {
  render(<Import accountId={1} onNavigate={onNavigate} />);
}

beforeEach(() => vi.clearAllMocks());

describe("Import", () => {
  it("imports pasted CSV into the selected account and shows a summary", async () => {
    vi.mocked(api.importCsv).mockResolvedValue({
      imported: 2,
      skipped_duplicates: 1,
      errors: [],
    });
    renderImport();
    const user = userEvent.setup();

    const csv = "date,amount,description\n2026-01-05,-42.90,Coffee\n";
    await user.type(screen.getByRole("textbox"), csv);
    await user.click(
      screen.getByRole("button", { name: /import transactions/i }),
    );

    await waitFor(() => expect(api.importCsv).toHaveBeenCalledWith(1, csv));
    expect(refreshAll).toHaveBeenCalled();
    // Summary panel reflects the backend result.
    expect(await screen.findByText("Import summary")).toBeInTheDocument();
    expect(screen.getByText("2")).toBeInTheDocument(); // imported count
  });

  it("decodes an ISO-8859-1 bank export without mangling umlauts", async () => {
    const { container } = render(
      <Import accountId={1} onNavigate={onNavigate} />,
    );
    // "AüB" in ISO-8859-1 / windows-1252 (ü = 0xFC) — invalid UTF-8, so the
    // decoder must fall back rather than emit replacement characters.
    const file = new File([new Uint8Array([0x41, 0xfc, 0x42])], "ing.csv", {
      type: "text/csv",
    });
    const input = container.querySelector(
      'input[type="file"]',
    ) as HTMLInputElement;
    await userEvent.upload(input, file);

    const textarea = screen.getByRole("textbox") as HTMLTextAreaElement;
    await waitFor(() => expect(textarea.value).toBe("AüB"));
    expect(textarea.value).not.toContain("�");
  });

  it("does not call the backend when there is nothing to import", async () => {
    renderImport();
    // The button is disabled with an empty textarea, so the backend is untouched.
    expect(
      screen.getByRole("button", { name: /import transactions/i }),
    ).toBeDisabled();
    expect(api.importCsv).not.toHaveBeenCalled();
  });
});
