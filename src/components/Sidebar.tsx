import { useState } from "react";
import { useStore } from "../store";
import * as api from "../api";
import { centsToInput, formatMoney, parseAmountToCents } from "../format";
import {
  buildAccountTree,
  countDescendantAccounts,
  findNode,
  type AccountNode,
} from "../accounts";
import { Modal } from "./Modal";
import type { Account, View } from "../types";

interface Props {
  view: View;
  onNavigate: (view: View, accountId?: number | null) => void;
  selectedAccountId: number | null;
}

export function Sidebar({ view, onNavigate, selectedAccountId }: Props) {
  const { accounts, refreshAll, toast } = useStore();
  const [editing, setEditing] = useState<Account | "new" | null>(null);
  const [deleting, setDeleting] = useState<Account | null>(null);
  const [addingSubTo, setAddingSubTo] = useState<Account | null>(null);
  const [name, setName] = useState("");
  const [subName, setSubName] = useState("");
  // Starting balance as a raw decimal string, shared by the new/rename and the
  // add-subaccount modals. Empty means "leave at zero".
  const [opening, setOpening] = useState("");
  const [saving, setSaving] = useState(false);

  const tree = buildAccountTree(accounts);
  // Subaccounts and totals for the delete confirmation of whatever is selected.
  const deletingNode = deleting ? findNode(tree, deleting.id) : undefined;
  const deletingSubCount = deletingNode
    ? countDescendantAccounts(deletingNode)
    : 0;

  function openNew() {
    setEditing("new");
    setName("");
    setOpening("");
  }
  function openRename(acc: Account) {
    setEditing(acc);
    setName(acc.name);
    setOpening(acc.opening_balance ? centsToInput(acc.opening_balance) : "");
  }
  function openAddSub(acc: Account) {
    setAddingSubTo(acc);
    setSubName("");
    setOpening("");
  }

  /** Parse the shared starting-balance input to cents. Returns `null` for a
   *  blank field (leave at zero) and the sentinel `"invalid"` for junk. */
  function readOpeningCents(): number | null | "invalid" {
    if (!opening.trim()) return null;
    const cents = parseAmountToCents(opening);
    return cents == null ? "invalid" : cents;
  }

  async function save() {
    const trimmed = name.trim();
    if (!trimmed) return;
    const openingCents = readOpeningCents();
    if (openingCents === "invalid") {
      toast("Enter a valid starting balance, or leave it blank", "error");
      return;
    }
    setSaving(true);
    try {
      if (editing === "new") {
        const id = await api.createAccount(trimmed);
        if (openingCents != null && openingCents !== 0) {
          await api.setAccountOpeningBalance(id, openingCents);
        }
        toast(`Account “${trimmed}” created`);
      } else if (editing) {
        await api.renameAccount(editing.id, trimmed);
        // A blank field clears the starting balance back to zero.
        const target = openingCents ?? 0;
        if (target !== editing.opening_balance) {
          await api.setAccountOpeningBalance(editing.id, target);
        }
        toast("Account updated");
      }
      await refreshAll();
      setEditing(null);
    } catch (err) {
      toast(String(err), "error");
    } finally {
      setSaving(false);
    }
  }

  async function addSub() {
    if (!addingSubTo) return;
    const trimmed = subName.trim();
    if (!trimmed) return;
    const openingCents = readOpeningCents();
    if (openingCents === "invalid") {
      toast("Enter a valid starting balance, or leave it blank", "error");
      return;
    }
    setSaving(true);
    try {
      const id = await api.addSubaccount(addingSubTo.id, trimmed);
      if (openingCents != null && openingCents !== 0) {
        await api.setAccountOpeningBalance(id, openingCents);
      }
      toast(`Subaccount “${trimmed}” added to “${addingSubTo.name}”`);
      await refreshAll();
      setAddingSubTo(null);
    } catch (err) {
      toast(String(err), "error");
    } finally {
      setSaving(false);
    }
  }

  async function remove() {
    if (!deleting) return;
    const acc = deleting;
    setSaving(true);
    try {
      await api.deleteAccount(acc.id);
      toast(`Account “${acc.name}” deleted`);
      if (selectedAccountId === acc.id) onNavigate("transactions", null);
      await refreshAll();
      setDeleting(null);
    } catch (err) {
      toast(String(err), "error");
    } finally {
      setSaving(false);
    }
  }

  const navItems: { id: View; icon: string; label: string }[] = [
    { id: "dashboard", icon: "◧", label: "Dashboard" },
    { id: "transactions", icon: "≡", label: "Transactions" },
    { id: "import", icon: "↑", label: "Import CSV" },
  ];

  function renderNode(node: AccountNode): React.ReactNode {
    const acc = node.account;
    const isChild = node.depth > 0;
    return (
      <div key={acc.id}>
        <div
          className={
            "account-row" +
            (isChild ? " subaccount" : "") +
            (view === "transactions" && selectedAccountId === acc.id
              ? " active"
              : "")
          }
          style={{ paddingLeft: 12 + node.depth * 14 }}
        >
          <button
            className="acc-name"
            style={{
              border: "none",
              background: "transparent",
              padding: 0,
              cursor: "pointer",
              color: "inherit",
              textAlign: "left",
            }}
            onClick={() => onNavigate("transactions", acc.id)}
            title={acc.name}
          >
            {isChild && <span className="subaccount-tick">↳</span>}
            {acc.name}
          </button>
          <span className="acc-balance">{formatMoney(node.rollupBalance)}</span>
          <button
            className="icon-btn"
            onClick={() => openAddSub(acc)}
            title="Add subaccount"
          >
            ＋
          </button>
          <button
            className="icon-btn"
            onClick={() => openRename(acc)}
            title="Edit"
          >
            ✎
          </button>
        </div>
        {node.children.map(renderNode)}
      </div>
    );
  }

  return (
    <aside className="sidebar">
      <div className="brand">
        <span className="flame">🔥</span> Fire
      </div>

      {navItems.map((n) => (
        <button
          key={n.id}
          className={"nav-item" + (view === n.id ? " active" : "")}
          onClick={() => onNavigate(n.id)}
        >
          <span className="icon">{n.icon}</span>
          {n.label}
        </button>
      ))}

      <div className="sidebar-section">
        <span>Accounts</span>
        <button onClick={openNew} title="New account">
          +
        </button>
      </div>

      {accounts.length === 0 && (
        <div className="empty-hint">No accounts yet. Click + to add one.</div>
      )}

      {tree.map(renderNode)}

      {editing && (
        <Modal
          title={editing === "new" ? "New account" : "Edit account"}
          onClose={() => setEditing(null)}
          footer={
            <>
              {editing !== "new" && (
                <button
                  className="btn danger"
                  style={{ marginRight: "auto" }}
                  onClick={() => {
                    setDeleting(editing);
                    setEditing(null);
                  }}
                >
                  🗑 Delete
                </button>
              )}
              <button className="btn" onClick={() => setEditing(null)}>
                Cancel
              </button>
              <button
                className="btn primary"
                onClick={save}
                disabled={saving || !name.trim()}
              >
                {editing === "new" ? "Create" : "Save"}
              </button>
            </>
          }
        >
          <div className="field">
            <label>Account name</label>
            <input
              autoFocus
              value={name}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && save()}
              placeholder="e.g. Checking, Savings, Credit Card"
            />
          </div>
          <div className="field">
            <label>Starting balance</label>
            <input
              inputMode="decimal"
              value={opening}
              onChange={(e) => setOpening(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && save()}
              placeholder="0.00"
              style={{ textAlign: "right" }}
            />
            <p className="muted" style={{ fontSize: 12.5, marginTop: 6 }}>
              The balance before your earliest transaction. Set this when you’re
              only importing part of the account’s history. It counts toward the
              balance but not income or expenses.
            </p>
          </div>
        </Modal>
      )}

      {addingSubTo && (
        <Modal
          title={`Add subaccount to “${addingSubTo.name}”`}
          onClose={() => setAddingSubTo(null)}
          footer={
            <>
              <button className="btn" onClick={() => setAddingSubTo(null)}>
                Cancel
              </button>
              <button
                className="btn primary"
                onClick={addSub}
                disabled={saving || !subName.trim()}
              >
                Add subaccount
              </button>
            </>
          }
        >
          <div className="field">
            <label>Subaccount name</label>
            <input
              autoFocus
              value={subName}
              onChange={(e) => setSubName(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && addSub()}
              placeholder="e.g. Checking, Savings, Brokerage"
            />
          </div>
          <div className="field">
            <label>Starting balance</label>
            <input
              inputMode="decimal"
              value={opening}
              onChange={(e) => setOpening(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && addSub()}
              placeholder="0.00"
              style={{ textAlign: "right" }}
            />
          </div>
          <p className="muted" style={{ fontSize: 12.5, marginTop: 0 }}>
            The subaccount starts with no transactions; set a starting balance
            if you’re only importing part of its history. Its balance rolls up
            into “{addingSubTo.name}”.
          </p>
        </Modal>
      )}

      {deleting && (
        <Modal
          title="Delete account"
          onClose={() => setDeleting(null)}
          footer={
            <>
              <button className="btn" onClick={() => setDeleting(null)}>
                Cancel
              </button>
              <button className="btn danger" onClick={remove} disabled={saving}>
                Delete
              </button>
            </>
          }
        >
          {deletingNode && deletingSubCount > 0 ? (
            <p>
              Delete account “{deleting.name}”, its {deletingSubCount}{" "}
              subaccount(s) and all {deletingNode.rollupTxCount} transaction(s)?
              This cannot be undone.
            </p>
          ) : (
            <p>
              Delete account “{deleting.name}” and its {deleting.tx_count}{" "}
              transaction(s)? This cannot be undone.
            </p>
          )}
        </Modal>
      )}
    </aside>
  );
}
