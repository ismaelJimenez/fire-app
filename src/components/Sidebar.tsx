import { useState } from "react";
import { useStore } from "../store";
import * as api from "../api";
import { formatMoney } from "../format";
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
  const [name, setName] = useState("");
  const [saving, setSaving] = useState(false);

  function openNew() {
    setEditing("new");
    setName("");
  }
  function openRename(acc: Account) {
    setEditing(acc);
    setName(acc.name);
  }

  async function save() {
    const trimmed = name.trim();
    if (!trimmed) return;
    setSaving(true);
    try {
      if (editing === "new") {
        await api.createAccount(trimmed);
        toast(`Account “${trimmed}” created`);
      } else if (editing) {
        await api.renameAccount(editing.id, trimmed);
        toast("Account renamed");
      }
      await refreshAll();
      setEditing(null);
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

      {accounts.map((acc) => (
        <div
          key={acc.id}
          className={
            "account-row" +
            (view === "transactions" && selectedAccountId === acc.id
              ? " active"
              : "")
          }
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
            {acc.name}
          </button>
          <span className="acc-balance">{formatMoney(acc.balance)}</span>
          <button
            className="icon-btn"
            onClick={() => openRename(acc)}
            title="Rename"
          >
            ✎
          </button>
          <button
            className="icon-btn danger"
            onClick={() => setDeleting(acc)}
            title="Delete"
          >
            🗑
          </button>
        </div>
      ))}

      {editing && (
        <Modal
          title={editing === "new" ? "New account" : "Rename account"}
          onClose={() => setEditing(null)}
          footer={
            <>
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
              <button
                className="btn danger"
                onClick={remove}
                disabled={saving}
              >
                Delete
              </button>
            </>
          }
        >
          <p>
            Delete account “{deleting.name}” and its {deleting.tx_count}{" "}
            transaction(s)? This cannot be undone.
          </p>
        </Modal>
      )}
    </aside>
  );
}
