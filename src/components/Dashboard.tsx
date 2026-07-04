import { useEffect, useState } from "react";
import { useStore } from "../store";
import * as api from "../api";
import { formatDate, formatMoney } from "../format";
import { buildAccountTree, flattenTree, type AccountNode } from "../accounts";
import type { Transaction, View } from "../types";

interface Props {
  onNavigate: (view: View, accountId?: number | null) => void;
}

export function Dashboard({ onNavigate }: Props) {
  const { accounts, summary, toast } = useStore();
  const [recent, setRecent] = useState<Transaction[]>([]);

  useEffect(() => {
    api
      .listTransactions(null, "")
      .then((rows) => setRecent(rows.slice(0, 8)))
      .catch((err) => toast(String(err), "error"));
  }, [summary, toast]);

  if (accounts.length === 0) {
    return (
      <div>
        <div className="page-head">
          <div>
            <h1>Dashboard</h1>
            <p>An overview of your money.</p>
          </div>
        </div>
        <div className="empty card">
          <div className="big">🔥</div>
          <h3>Welcome to Fire</h3>
          <p>
            Start by creating an account in the sidebar, then import a CSV of
            your transactions or add them manually.
          </p>
          <button
            className="btn primary"
            onClick={() => onNavigate("import")}
          >
            Go to import
          </button>
        </div>
      </div>
    );
  }

  return (
    <div>
      <div className="page-head">
        <div>
          <h1>Dashboard</h1>
          <p>An overview of your money.</p>
        </div>
        <button className="btn" onClick={() => onNavigate("transactions")}>
          View all transactions →
        </button>
      </div>

      <div className="stat-grid">
        <div className="card stat">
          <div className="label">Total balance</div>
          <div
            className={
              "value " +
              (summary && summary.total_balance < 0 ? "neg" : "")
            }
          >
            {summary ? formatMoney(summary.total_balance) : "—"}
          </div>
        </div>
        <div className="card stat">
          <div className="label">Income</div>
          <div className="value pos">
            {summary ? formatMoney(summary.income) : "—"}
          </div>
        </div>
        <div className="card stat">
          <div className="label">Expenses</div>
          <div className="value neg">
            {summary ? formatMoney(summary.expenses) : "—"}
          </div>
        </div>
        <div className="card stat">
          <div className="label">Transactions</div>
          <div className="value">{summary?.transaction_count ?? "—"}</div>
        </div>
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "1fr 1.4fr", gap: 20 }}>
        <div>
          <div className="section-title">Accounts</div>
          <div className="card">
            <table>
              <tbody>
                {flattenTree(buildAccountTree(accounts)).map((node) =>
                  accountRow(node, onNavigate),
                )}
              </tbody>
            </table>
          </div>
        </div>

        <div>
          <div className="section-title">Recent activity</div>
          <div className="card table-wrap">
            {recent.length === 0 ? (
              <div className="empty" style={{ padding: 40 }}>
                <p>No transactions yet.</p>
              </div>
            ) : (
              <table>
                <tbody>
                  {recent.map((tx) => (
                    <tr key={tx.id}>
                      <td className="date">{formatDate(tx.date)}</td>
                      <td>
                        {tx.description || <span className="muted">—</span>}
                        <div className="muted" style={{ fontSize: 12 }}>
                          {tx.account_name}
                          {tx.category_name ? ` · ${tx.category_name}` : ""}
                        </div>
                      </td>
                      <td
                        className="amount"
                        style={{
                          color:
                            tx.amount < 0
                              ? "var(--negative)"
                              : "var(--positive)",
                        }}
                      >
                        {formatMoney(tx.amount)}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function accountRow(
  node: AccountNode,
  onNavigate: (view: View, accountId?: number | null) => void,
) {
  const a = node.account;
  const isChild = node.depth > 0;
  return (
    <tr
      key={a.id}
      style={{ cursor: "pointer" }}
      onClick={() => onNavigate("transactions", a.id)}
    >
      <td style={{ fontWeight: isChild ? 400 : 500 }}>
        <span
          className={isChild ? "muted" : undefined}
          style={{ paddingLeft: node.depth * 16 }}
        >
          {isChild ? `↳ ${a.name}` : a.name}
        </span>
      </td>
      <td className="muted" style={{ textAlign: "right" }}>
        {node.rollupTxCount} txn
      </td>
      <td
        className="amount"
        style={{
          color: node.rollupBalance < 0 ? "var(--negative)" : "var(--positive)",
        }}
      >
        {formatMoney(node.rollupBalance)}
      </td>
    </tr>
  );
}
