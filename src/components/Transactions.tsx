import { useCallback, useEffect, useState } from "react";
import { useStore } from "../store";
import * as api from "../api";
import { formatDate, formatMoney } from "../format";
import { accountSelectOptions } from "../accounts";
import { TransactionForm } from "./TransactionForm";
import { Modal } from "./Modal";
import type { Transaction } from "../types";

interface Props {
  accountId: number | null;
  onSelectAccount: (id: number | null) => void;
}

export function Transactions({ accountId, onSelectAccount }: Props) {
  const { accounts, categories, refreshAll, toast } = useStore();
  // Transfers are identified by the built-in category's id, not its name, so a
  // rename never breaks the badge.
  const transferCategoryId = categories.find((c) => c.is_transfer)?.id ?? null;
  const [rows, setRows] = useState<Transaction[]>([]);
  const [search, setSearch] = useState("");
  const [loading, setLoading] = useState(true);
  const [editing, setEditing] = useState<Transaction | "new" | null>(null);
  const [deleting, setDeleting] = useState<Transaction | null>(null);
  const [removing, setRemoving] = useState(false);

  // `silent` refreshes the rows in place without flipping the loading state,
  // which would unmount the table and reset the scroll position — jarring when
  // verifying rows one after another.
  const load = useCallback(
    async (silent = false) => {
      if (!silent) setLoading(true);
      try {
        setRows(await api.listTransactions(accountId, search));
      } catch (err) {
        toast(String(err), "error");
      } finally {
        if (!silent) setLoading(false);
      }
    },
    [accountId, search, toast],
  );

  // Debounce search; reload on account change.
  useEffect(() => {
    const t = setTimeout(load, search ? 200 : 0);
    return () => clearTimeout(t);
  }, [load, search]);

  async function afterMutation() {
    await Promise.all([load(true), refreshAll()]);
  }

  async function changeCategory(tx: Transaction, value: string) {
    const categoryId = value ? Number(value) : null;
    try {
      const propagated = await api.setTransactionCategory(tx.id, categoryId);
      if (propagated > 0) {
        toast(
          `Learned this payee — also classified ${propagated} matching transaction(s)`,
        );
      }
      await afterMutation();
    } catch (err) {
      toast(String(err), "error");
    }
  }

  async function toggleVerified(tx: Transaction) {
    try {
      await api.setTransactionVerified(tx.id, !tx.is_verified);
      toast(tx.is_verified ? "Marked as unverified" : "Marked as verified");
      await afterMutation();
    } catch (err) {
      toast(String(err), "error");
    }
  }

  async function remove() {
    if (!deleting) return;
    setRemoving(true);
    try {
      await api.deleteTransaction(deleting.id);
      toast("Transaction deleted");
      await afterMutation();
      setDeleting(null);
    } catch (err) {
      toast(String(err), "error");
    } finally {
      setRemoving(false);
    }
  }

  const activeAccount = accounts.find((a) => a.id === accountId) ?? null;
  const unverifiedCount = rows.filter((tx) => !tx.is_verified).length;
  const uncategorizedCount = rows.filter((tx) => tx.category_id == null).length;

  return (
    <div>
      <div className="page-head">
        <div>
          <h1>Transactions</h1>
          <p>
            {activeAccount ? `Showing ${activeAccount.name}` : "All accounts"}
            {" · "}
            {rows.length} shown
            {uncategorizedCount > 0 && (
              <>
                {" · "}
                <span className="review-count">
                  <span aria-hidden="true">⚠</span> {uncategorizedCount}{" "}
                  uncategorized
                </span>
              </>
            )}
            {unverifiedCount > 0 && (
              <>
                {" · "}
                <span className="review-count">
                  <span aria-hidden="true">⚠</span> {unverifiedCount} unverified
                </span>
              </>
            )}
          </p>
        </div>
        <button
          className="btn primary"
          onClick={() => setEditing("new")}
          disabled={accounts.length === 0}
        >
          + New transaction
        </button>
      </div>

      <div className="toolbar">
        <div className="search">
          <span className="glyph">⌕</span>
          <input
            placeholder="Search description or category…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
        <select
          value={accountId ?? ""}
          onChange={(e) =>
            onSelectAccount(e.target.value ? Number(e.target.value) : null)
          }
        >
          <option value="">All accounts</option>
          {accountSelectOptions(accounts).map((a) => (
            <option key={a.id} value={a.id}>
              {a.label}
            </option>
          ))}
        </select>
      </div>

      {loading ? (
        <div className="empty">
          <p>Loading…</p>
        </div>
      ) : rows.length === 0 ? (
        <div className="empty card">
          <div className="big">🧾</div>
          <h3>No transactions yet</h3>
          <p>
            {accounts.length === 0
              ? "Create an account first, then import a CSV or add transactions manually."
              : "Import a CSV or add a transaction to get started."}
          </p>
          {accounts.length > 0 && (
            <button className="btn primary" onClick={() => setEditing("new")}>
              + New transaction
            </button>
          )}
        </div>
      ) : (
        <div className="card table-wrap">
          <table>
            <thead>
              <tr>
                <th style={{ width: 110 }}>Date</th>
                {!activeAccount && <th style={{ width: 130 }}>Account</th>}
                <th>Description</th>
                <th style={{ width: 180 }}>Category</th>
                <th style={{ width: 130, textAlign: "right" }}>Amount</th>
                <th style={{ width: 96 }}></th>
              </tr>
            </thead>
            <tbody>
              {rows.map((tx) => (
                <tr key={tx.id}>
                  <td className="date">{formatDate(tx.date)}</td>
                  {!activeAccount && (
                    <td className="muted">{tx.account_name}</td>
                  )}
                  <td>
                    <div>
                      {tx.description || <span className="muted">—</span>}
                      {transferCategoryId != null &&
                        tx.category_id === transferCategoryId && (
                          <span
                            className="badge transfer"
                            style={{ marginLeft: 8 }}
                          >
                            ⇄ transfer
                          </span>
                        )}
                    </div>
                    {tx.counterparty && (
                      <div className="tx-payee">{tx.counterparty}</div>
                    )}
                    {!tx.is_verified && (
                      <div
                        className="unverified-warning"
                        title="Not yet verified"
                      >
                        <span aria-hidden="true">⚠</span> unverified
                      </div>
                    )}
                  </td>
                  <td>
                    <select
                      className={
                        "cat-select" + (tx.category_id ? "" : " unset")
                      }
                      value={tx.category_id ?? ""}
                      disabled={tx.is_verified}
                      title={
                        tx.is_verified
                          ? "Verified — unverify to change the category"
                          : undefined
                      }
                      onChange={(e) => changeCategory(tx, e.target.value)}
                    >
                      <option value="">Uncategorized</option>
                      {categories.map((c) => (
                        <option key={c.id} value={c.id}>
                          {c.name}
                        </option>
                      ))}
                    </select>
                    {tx.category_id == null && (
                      <span
                        className="cat-warning"
                        title="Uncategorized"
                        aria-label="Uncategorized"
                      >
                        ⚠
                      </span>
                    )}
                    {tx.is_auto_classified &&
                      tx.category_id != null &&
                      !tx.is_verified && (
                        <span
                          className="badge auto"
                          style={{ marginLeft: 6 }}
                          title="Classified automatically from a learned payee rule"
                        >
                          auto
                        </span>
                      )}
                  </td>
                  <td
                    className="amount"
                    style={{
                      color:
                        tx.amount < 0 ? "var(--negative)" : "var(--positive)",
                    }}
                  >
                    {formatMoney(tx.amount)}
                  </td>
                  <td>
                    <div className="row-actions">
                      <button
                        className={"icon-btn" + (tx.is_verified ? " on" : "")}
                        title={
                          tx.is_verified
                            ? "Verified — click to unverify"
                            : "Mark as verified (reviewed)"
                        }
                        onClick={() => toggleVerified(tx)}
                      >
                        ✓
                      </button>
                      <button
                        className="icon-btn"
                        title="Edit"
                        onClick={() => setEditing(tx)}
                      >
                        ✎
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {editing && (
        <TransactionForm
          tx={editing === "new" ? null : editing}
          defaultAccountId={accountId}
          onClose={() => setEditing(null)}
          onSaved={afterMutation}
          onDelete={
            editing !== "new"
              ? () => {
                  setDeleting(editing);
                  setEditing(null);
                }
              : undefined
          }
        />
      )}

      {deleting && (
        <Modal
          title="Delete transaction"
          onClose={() => setDeleting(null)}
          footer={
            <>
              <button className="btn" onClick={() => setDeleting(null)}>
                Cancel
              </button>
              <button
                className="btn danger"
                onClick={remove}
                disabled={removing}
              >
                Delete
              </button>
            </>
          }
        >
          <p>
            Delete this transaction
            {deleting.description
              ? ` (“${deleting.description}”)`
              : ""} for {formatMoney(deleting.amount)}? This cannot be undone.
          </p>
        </Modal>
      )}
    </div>
  );
}
