# 🔥 Fire — Expense Tracker

A clean, local-first desktop expense tracker built with **Tauri v2**, **React + TypeScript**,
and a **SQLite** database. Import transactions from CSV, organize them into accounts,
categorize, and mark internal transfers.

## Features

- **Accounts** — create, rename, and delete accounts from the sidebar (deleting an
  account removes its transactions).
- **Subaccounts** — add subaccounts under any account; their balances roll up into
  the parent, and deleting a parent removes its subaccounts too.
- **CSV import** — drop or paste a CSV; columns are matched by header name and duplicate
  rows are skipped automatically so re-imports are safe.
- **Manual entry** — add and edit transactions with a simple form.
- **Fix-ups** — change a transaction's category inline, mark/unmark it as an internal
  transfer (excluded from income/expense totals), or delete it.
- **Dashboard** — total balance, income, expenses, per-account balances, and recent activity.
- **Local storage** — everything is stored in a SQLite database on your machine; no cloud,
  no accounts.

## CSV format

A header row followed by one transaction per line:

```csv
date,amount,description,category
2026-01-05,-42.90,Grocery store,Groceries
2026-01-06,1500.00,Monthly salary,Income
2026-01-07,-12.00,Coffee shop,Dining
```

- `date` — `YYYY-MM-DD` (also accepts `DD/MM/YYYY`)
- `amount` — decimal; **negative = expense**, **positive = income**
- `description` — free text
- `category` — optional; created automatically if new

Columns are matched by header name, so their order does not matter. A "Download template"
button is available on the Import screen.

## Where is my data?

The SQLite file `fire.db` lives in the platform app-data directory, e.g. on macOS:
`~/Library/Application Support/com.q602768.fire-app/fire.db`

## Development

```bash
pnpm install
pnpm tauri dev      # run the app with hot reload
pnpm tauri build    # produce a distributable bundle
```

## Project layout

```
src/                     React front end
  api.ts                 typed wrappers around Tauri commands
  store.tsx              global state (accounts, categories, summary, toasts)
  format.ts              money/date helpers (amounts stored as integer cents)
  components/            Sidebar, Dashboard, Transactions, Import, TransactionForm, Modal
src-tauri/src/
  db.rs                  SQLite connection + schema/migrations
  models.rs              serde structs shared with the front end
  commands.rs            Tauri commands (accounts, transactions, categories, CSV import)
  lib.rs                 app setup, DB state, command registration
```

Amounts are stored as **integer cents** throughout to avoid floating-point rounding errors.
The app is single-currency (formatted as EUR by default — change in `src/format.ts`).
