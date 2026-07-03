import { useState } from "react";
import { useStore } from "../store";
import * as api from "../api";
import { Modal } from "./Modal";
import { parseAmountToCents, todayIso } from "../format";
import type { Transaction } from "../types";

interface Props {
  /** Existing transaction to edit, or null to create a new one. */
  tx: Transaction | null;
  /** Pre-selected account for a new transaction. */
  defaultAccountId: number | null;
  onClose: () => void;
  onSaved: () => void;
}

export function TransactionForm({
  tx,
  defaultAccountId,
  onClose,
  onSaved,
}: Props) {
  const { accounts, categories, toast } = useStore();

  const [accountId, setAccountId] = useState<number | null>(
    tx?.account_id ?? defaultAccountId ?? accounts[0]?.id ?? null,
  );
  const [date, setDate] = useState(tx?.date ?? todayIso());
  const [sign, setSign] = useState<"-" | "+">(
    tx && tx.amount > 0 ? "+" : "-",
  );
  const [amount, setAmount] = useState(
    tx ? Math.abs(tx.amount / 100).toFixed(2) : "",
  );
  const [description, setDescription] = useState(tx?.description ?? "");
  const [categoryId, setCategoryId] = useState<number | null>(
    tx?.category_id ?? null,
  );
  const [isTransfer, setIsTransfer] = useState(tx?.is_internal_transfer ?? false);
  const [saving, setSaving] = useState(false);

  async function save() {
    if (accountId == null) {
      toast("Please choose an account", "error");
      return;
    }
    const magnitude = parseAmountToCents(amount);
    if (magnitude == null || magnitude === 0) {
      toast("Please enter a valid non-zero amount", "error");
      return;
    }
    const cents = sign === "-" ? -Math.abs(magnitude) : Math.abs(magnitude);

    const payload = {
      account_id: accountId,
      date,
      amount: cents,
      description: description.trim(),
      category_id: categoryId,
      is_internal_transfer: isTransfer,
    };

    setSaving(true);
    try {
      if (tx) {
        await api.updateTransaction(tx.id, payload);
        toast("Transaction updated");
      } else {
        await api.createTransaction(payload);
        toast("Transaction added");
      }
      onSaved();
      onClose();
    } catch (err) {
      toast(String(err), "error");
    } finally {
      setSaving(false);
    }
  }

  return (
    <Modal
      title={tx ? "Edit transaction" : "New transaction"}
      onClose={onClose}
      footer={
        <>
          <button className="btn" onClick={onClose}>
            Cancel
          </button>
          <button className="btn primary" onClick={save} disabled={saving}>
            {tx ? "Save changes" : "Add transaction"}
          </button>
        </>
      }
    >
      <div className="field">
        <label>Account</label>
        <select
          value={accountId ?? ""}
          onChange={(e) => setAccountId(Number(e.target.value))}
        >
          {accounts.length === 0 && <option value="">No accounts</option>}
          {accounts.map((a) => (
            <option key={a.id} value={a.id}>
              {a.name}
            </option>
          ))}
        </select>
      </div>

      <div className="field-row">
        <div className="field">
          <label>Date</label>
          <input
            type="date"
            value={date}
            onChange={(e) => setDate(e.target.value)}
          />
        </div>
        <div className="field">
          <label>Amount</label>
          <div style={{ display: "flex", gap: 8 }}>
            <select
              value={sign}
              onChange={(e) => setSign(e.target.value as "-" | "+")}
              style={{ width: 92, flex: "0 0 auto" }}
            >
              <option value="-">− Expense</option>
              <option value="+">+ Income</option>
            </select>
            <input
              inputMode="decimal"
              placeholder="0.00"
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
              style={{ textAlign: "right" }}
            />
          </div>
        </div>
      </div>

      <div className="field">
        <label>Description</label>
        <input
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder="e.g. Grocery store"
        />
      </div>

      <div className="field">
        <label>Category</label>
        <select
          value={categoryId ?? ""}
          onChange={(e) =>
            setCategoryId(e.target.value ? Number(e.target.value) : null)
          }
        >
          <option value="">— Uncategorized —</option>
          {categories.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </select>
      </div>

      <label className="checkbox" style={{ marginTop: 4 }}>
        <input
          type="checkbox"
          checked={isTransfer}
          onChange={(e) => setIsTransfer(e.target.checked)}
        />
        Internal transfer (excluded from income &amp; expense totals)
      </label>
      <div style={{ height: 4 }} />
      {/* keep helper text alignment consistent */}
      <p className="muted" style={{ fontSize: 12.5, marginTop: 0 }}>
        Use for moving money between your own accounts.
      </p>
    </Modal>
  );
}
