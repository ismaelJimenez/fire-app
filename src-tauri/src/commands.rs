use crate::models::{Account, Category, ImportResult, Summary, Transaction};
use crate::AppState;
use chrono::NaiveDate;
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection};
use tauri::State;

type CmdResult<T> = Result<T, String>;

/// Map any error to a String so it crosses the Tauri boundary cleanly.
fn e<E: std::fmt::Display>(err: E) -> String {
    err.to_string()
}

// ----------------------------------------------------------------------------
// Accounts
// ----------------------------------------------------------------------------

#[tauri::command]
pub fn list_accounts(state: State<AppState>) -> CmdResult<Vec<Account>> {
    let conn = state.db.lock().map_err(e)?;
    let mut stmt = conn
        .prepare(
            "SELECT a.id, a.name, a.created_at,
                    COALESCE(SUM(t.amount), 0) AS balance,
                    COUNT(t.id) AS tx_count
             FROM accounts a
             LEFT JOIN transactions t ON t.account_id = a.id
             GROUP BY a.id
             ORDER BY a.name COLLATE NOCASE",
        )
        .map_err(e)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Account {
                id: r.get(0)?,
                name: r.get(1)?,
                created_at: r.get(2)?,
                balance: r.get(3)?,
                tx_count: r.get(4)?,
            })
        })
        .map_err(e)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(e)
}

#[tauri::command]
pub fn create_account(state: State<AppState>, name: String) -> CmdResult<i64> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Account name cannot be empty".into());
    }
    let conn = state.db.lock().map_err(e)?;
    conn.execute("INSERT INTO accounts (name) VALUES (?1)", params![name])
        .map_err(|err| match err {
            rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation => {
                format!("An account named \"{name}\" already exists")
            }
            other => e(other),
        })?;
    Ok(conn.last_insert_rowid())
}

#[tauri::command]
pub fn rename_account(state: State<AppState>, id: i64, name: String) -> CmdResult<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Account name cannot be empty".into());
    }
    let conn = state.db.lock().map_err(e)?;
    conn.execute(
        "UPDATE accounts SET name = ?1 WHERE id = ?2",
        params![name, id],
    )
    .map_err(|err| match err {
        rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation => {
            format!("An account named \"{name}\" already exists")
        }
        other => e(other),
    })?;
    Ok(())
}

#[tauri::command]
pub fn delete_account(state: State<AppState>, id: i64) -> CmdResult<()> {
    let conn = state.db.lock().map_err(e)?;
    // Transactions are removed via ON DELETE CASCADE.
    conn.execute("DELETE FROM accounts WHERE id = ?1", params![id])
        .map_err(e)?;
    Ok(())
}

// ----------------------------------------------------------------------------
// Categories
// ----------------------------------------------------------------------------

#[tauri::command]
pub fn list_categories(state: State<AppState>) -> CmdResult<Vec<Category>> {
    let conn = state.db.lock().map_err(e)?;
    let mut stmt = conn
        .prepare("SELECT id, name FROM categories ORDER BY name COLLATE NOCASE")
        .map_err(e)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Category {
                id: r.get(0)?,
                name: r.get(1)?,
            })
        })
        .map_err(e)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(e)
}

#[tauri::command]
pub fn create_category(state: State<AppState>, name: String) -> CmdResult<i64> {
    let conn = state.db.lock().map_err(e)?;
    get_or_create_category(&conn, name.trim())
}

#[tauri::command]
pub fn delete_category(state: State<AppState>, id: i64) -> CmdResult<()> {
    let conn = state.db.lock().map_err(e)?;
    // Transactions keep their row but category_id is set to NULL (ON DELETE SET NULL).
    conn.execute("DELETE FROM categories WHERE id = ?1", params![id])
        .map_err(e)?;
    Ok(())
}

/// Return the id of the category with this name, creating it if needed.
fn get_or_create_category(conn: &Connection, name: &str) -> CmdResult<i64> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Category name cannot be empty".into());
    }
    if let Ok(id) = conn.query_row(
        "SELECT id FROM categories WHERE name = ?1 COLLATE NOCASE",
        params![name],
        |r| r.get::<_, i64>(0),
    ) {
        return Ok(id);
    }
    conn.execute("INSERT INTO categories (name) VALUES (?1)", params![name])
        .map_err(e)?;
    Ok(conn.last_insert_rowid())
}

// ----------------------------------------------------------------------------
// Transactions
// ----------------------------------------------------------------------------

const TX_SELECT: &str = "SELECT t.id, t.account_id, a.name, t.date, t.amount, t.description,
        t.category_id, c.name, t.is_internal_transfer, t.created_at
 FROM transactions t
 JOIN accounts a ON a.id = t.account_id
 LEFT JOIN categories c ON c.id = t.category_id";

fn map_tx(r: &rusqlite::Row) -> rusqlite::Result<Transaction> {
    Ok(Transaction {
        id: r.get(0)?,
        account_id: r.get(1)?,
        account_name: r.get(2)?,
        date: r.get(3)?,
        amount: r.get(4)?,
        description: r.get(5)?,
        category_id: r.get(6)?,
        category_name: r.get(7)?,
        is_internal_transfer: r.get::<_, i64>(8)? != 0,
        created_at: r.get(9)?,
    })
}

#[tauri::command]
pub fn list_transactions(
    state: State<AppState>,
    account_id: Option<i64>,
    search: Option<String>,
) -> CmdResult<Vec<Transaction>> {
    let conn = state.db.lock().map_err(e)?;
    let mut sql = String::from(TX_SELECT);
    let mut clauses: Vec<String> = Vec::new();
    let mut values: Vec<Value> = Vec::new();

    if let Some(aid) = account_id {
        clauses.push(format!("t.account_id = ?{}", values.len() + 1));
        values.push(Value::Integer(aid));
    }
    if let Some(q) = search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        clauses.push(format!(
            "(t.description LIKE ?{0} OR c.name LIKE ?{0})",
            values.len() + 1
        ));
        values.push(Value::Text(format!("%{q}%")));
    }
    if !clauses.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&clauses.join(" AND "));
    }
    sql.push_str(" ORDER BY t.date DESC, t.id DESC");

    let mut stmt = conn.prepare(&sql).map_err(e)?;
    let rows = stmt
        .query_map(params_from_iter(values.iter()), map_tx)
        .map_err(e)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(e)
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn create_transaction(
    state: State<AppState>,
    account_id: i64,
    date: String,
    amount: i64,
    description: String,
    category_id: Option<i64>,
    is_internal_transfer: bool,
) -> CmdResult<i64> {
    validate_date(&date)?;
    let conn = state.db.lock().map_err(e)?;
    conn.execute(
        "INSERT INTO transactions
            (account_id, date, amount, description, category_id, is_internal_transfer)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            account_id,
            date,
            amount,
            description.trim(),
            category_id,
            is_internal_transfer as i64
        ],
    )
    .map_err(e)?;
    Ok(conn.last_insert_rowid())
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn update_transaction(
    state: State<AppState>,
    id: i64,
    date: String,
    amount: i64,
    description: String,
    category_id: Option<i64>,
    is_internal_transfer: bool,
) -> CmdResult<()> {
    validate_date(&date)?;
    let conn = state.db.lock().map_err(e)?;
    conn.execute(
        "UPDATE transactions
         SET date = ?1, amount = ?2, description = ?3,
             category_id = ?4, is_internal_transfer = ?5
         WHERE id = ?6",
        params![
            date,
            amount,
            description.trim(),
            category_id,
            is_internal_transfer as i64,
            id
        ],
    )
    .map_err(e)?;
    Ok(())
}

/// Lightweight update used by the inline category picker.
#[tauri::command]
pub fn set_transaction_category(
    state: State<AppState>,
    id: i64,
    category_id: Option<i64>,
) -> CmdResult<()> {
    let conn = state.db.lock().map_err(e)?;
    conn.execute(
        "UPDATE transactions SET category_id = ?1 WHERE id = ?2",
        params![category_id, id],
    )
    .map_err(e)?;
    Ok(())
}

/// Toggle a transaction's "internal transfer" flag.
#[tauri::command]
pub fn set_internal_transfer(
    state: State<AppState>,
    id: i64,
    is_internal_transfer: bool,
) -> CmdResult<()> {
    let conn = state.db.lock().map_err(e)?;
    conn.execute(
        "UPDATE transactions SET is_internal_transfer = ?1 WHERE id = ?2",
        params![is_internal_transfer as i64, id],
    )
    .map_err(e)?;
    Ok(())
}

#[tauri::command]
pub fn delete_transaction(state: State<AppState>, id: i64) -> CmdResult<()> {
    let conn = state.db.lock().map_err(e)?;
    conn.execute("DELETE FROM transactions WHERE id = ?1", params![id])
        .map_err(e)?;
    Ok(())
}

// ----------------------------------------------------------------------------
// Dashboard summary
// ----------------------------------------------------------------------------

#[tauri::command]
pub fn get_summary(state: State<AppState>) -> CmdResult<Summary> {
    let conn = state.db.lock().map_err(e)?;
    // Internal transfers are excluded from income/expense totals but still
    // count toward account balances.
    let total_balance: i64 = conn
        .query_row("SELECT COALESCE(SUM(amount), 0) FROM transactions", [], |r| {
            r.get(0)
        })
        .map_err(e)?;
    let income: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM transactions
             WHERE amount > 0 AND is_internal_transfer = 0",
            [],
            |r| r.get(0),
        )
        .map_err(e)?;
    let expenses: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM transactions
             WHERE amount < 0 AND is_internal_transfer = 0",
            [],
            |r| r.get(0),
        )
        .map_err(e)?;
    let account_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))
        .map_err(e)?;
    let transaction_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .map_err(e)?;

    Ok(Summary {
        total_balance,
        income,
        expenses,
        account_count,
        transaction_count,
    })
}

// ----------------------------------------------------------------------------
// CSV import
// ----------------------------------------------------------------------------

/// Import a CSV document into an account.
///
/// Expected header columns (case-insensitive, order-independent):
///   date, amount, description, category (category optional)
///   - date:   YYYY-MM-DD
///   - amount: decimal with '.' separator; negative = expense, positive = income
///   - category: created automatically if it does not exist
///
/// Rows identical to an existing transaction (same account, date, amount and
/// description) are skipped so re-importing the same file is safe.
#[tauri::command]
pub fn import_csv(
    state: State<AppState>,
    account_id: i64,
    csv_text: String,
) -> CmdResult<ImportResult> {
    let mut conn = state.db.lock().map_err(e)?;

    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .flexible(true)
        .from_reader(csv_text.as_bytes());

    let headers = reader.headers().map_err(e)?.clone();
    let idx = |name: &str| headers.iter().position(|h| h.eq_ignore_ascii_case(name));
    let date_i = idx("date").ok_or("CSV is missing a 'date' column")?;
    let amount_i = idx("amount").ok_or("CSV is missing an 'amount' column")?;
    let desc_i = idx("description").ok_or("CSV is missing a 'description' column")?;
    let cat_i = idx("category");

    let mut result = ImportResult {
        imported: 0,
        skipped_duplicates: 0,
        errors: Vec::new(),
    };

    let tx = conn.transaction().map_err(e)?;
    for (n, record) in reader.records().enumerate() {
        let line = n + 2; // +1 for header, +1 for 1-based
        let record = match record {
            Ok(r) => r,
            Err(err) => {
                result.errors.push(format!("Row {line}: {err}"));
                continue;
            }
        };

        let raw_date = record.get(date_i).unwrap_or("").trim();
        let date = match normalize_date(raw_date) {
            Ok(d) => d,
            Err(err) => {
                result.errors.push(format!("Row {line}: {err}"));
                continue;
            }
        };

        let raw_amount = record.get(amount_i).unwrap_or("").trim();
        let amount = match parse_amount_cents(raw_amount) {
            Ok(a) => a,
            Err(err) => {
                result.errors.push(format!("Row {line}: {err}"));
                continue;
            }
        };

        let description = record.get(desc_i).unwrap_or("").trim();

        let category_id: Option<i64> = match cat_i.and_then(|i| record.get(i)).map(str::trim) {
            Some(c) if !c.is_empty() => match get_or_create_category(&tx, c) {
                Ok(id) => Some(id),
                Err(err) => {
                    result.errors.push(format!("Row {line}: {err}"));
                    None
                }
            },
            _ => None,
        };

        // Duplicate guard.
        let exists: bool = tx
            .query_row(
                "SELECT 1 FROM transactions
                 WHERE account_id = ?1 AND date = ?2 AND amount = ?3 AND description = ?4
                 LIMIT 1",
                params![account_id, date, amount, description],
                |_| Ok(()),
            )
            .is_ok();
        if exists {
            result.skipped_duplicates += 1;
            continue;
        }

        tx.execute(
            "INSERT INTO transactions (account_id, date, amount, description, category_id)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![account_id, date, amount, description, category_id],
        )
        .map_err(e)?;
        result.imported += 1;
    }
    tx.commit().map_err(e)?;

    Ok(result)
}

fn validate_date(date: &str) -> CmdResult<()> {
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map(|_| ())
        .map_err(|_| format!("Invalid date \"{date}\" (expected YYYY-MM-DD)"))
}

/// Accept a few common date layouts and normalize to YYYY-MM-DD.
fn normalize_date(raw: &str) -> CmdResult<String> {
    for fmt in ["%Y-%m-%d", "%d/%m/%Y", "%m/%d/%Y", "%Y/%m/%d", "%d-%m-%Y"] {
        if let Ok(d) = NaiveDate::parse_from_str(raw, fmt) {
            return Ok(d.format("%Y-%m-%d").to_string());
        }
    }
    Err(format!("Invalid date \"{raw}\" (expected YYYY-MM-DD)"))
}

/// Parse a decimal amount string into integer cents.
fn parse_amount_cents(raw: &str) -> CmdResult<i64> {
    // Strip currency symbols, spaces and thousands separators.
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '-' || *c == '.')
        .collect();
    if cleaned.is_empty() || cleaned == "-" {
        return Err(format!("Invalid amount \"{raw}\""));
    }
    let value: f64 = cleaned
        .parse()
        .map_err(|_| format!("Invalid amount \"{raw}\""))?;
    Ok((value * 100.0).round() as i64)
}
