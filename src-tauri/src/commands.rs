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

/// Return an error unless an account with this id exists. Gives callers a clear
/// message instead of an opaque foreign-key failure.
fn ensure_account_exists(conn: &Connection, id: i64) -> CmdResult<()> {
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM accounts WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .map_err(e)?;
    if exists == 0 {
        return Err("Account not found".into());
    }
    Ok(())
}

/// Escape LIKE metacharacters (`%`, `_`) and the escape char itself so user
/// search text matches literally. Pair with `ESCAPE '\'` in the query.
fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

// ----------------------------------------------------------------------------
// Accounts
// ----------------------------------------------------------------------------

#[tauri::command]
pub fn list_accounts(state: State<AppState>) -> CmdResult<Vec<Account>> {
    let conn = state.db.lock().map_err(e)?;
    // `balance` and `tx_count` are each account's own transactions only; the
    // front end rolls a parent's subaccounts up for display.
    let mut stmt = conn
        .prepare(
            "SELECT a.id, a.name, a.parent_id, a.created_at,
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
                parent_id: r.get(2)?,
                created_at: r.get(3)?,
                balance: r.get(4)?,
                tx_count: r.get(5)?,
            })
        })
        .map_err(e)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(e)
}

/// Add a subaccount under any account (nesting is unlimited).
///
/// The subaccount starts empty; transactions can then be created or moved into
/// it. Balances roll up through every ancestor.
#[tauri::command]
pub fn add_subaccount(state: State<AppState>, parent_id: i64, name: String) -> CmdResult<i64> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Subaccount name cannot be empty".into());
    }
    let conn = state.db.lock().map_err(e)?;

    // The parent must exist; new accounts are always leaves, so no cycle is possible.
    ensure_account_exists(&conn, parent_id)?;

    conn.execute(
        "INSERT INTO accounts (name, parent_id) VALUES (?1, ?2)",
        params![name, parent_id],
    )
    .map_err(|err| match err {
        rusqlite::Error::SqliteFailure(f, _)
            if f.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            format!("This account already has a subaccount named \"{name}\"")
        }
        other => e(other),
    })?;
    Ok(conn.last_insert_rowid())
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
            rusqlite::Error::SqliteFailure(f, _)
                if f.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
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
        rusqlite::Error::SqliteFailure(f, _)
            if f.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
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
    query_transactions(&conn, account_id, search.as_deref())
}

/// Core of `list_transactions`, decoupled from Tauri state so the filter/search
/// SQL builder can be tested directly against a `Connection`.
fn query_transactions(
    conn: &Connection,
    account_id: Option<i64>,
    search: Option<&str>,
) -> CmdResult<Vec<Transaction>> {
    let mut sql = String::from(TX_SELECT);
    let mut clauses: Vec<String> = Vec::new();
    let mut values: Vec<Value> = Vec::new();

    if let Some(aid) = account_id {
        clauses.push(format!("t.account_id = ?{}", values.len() + 1));
        values.push(Value::Integer(aid));
    }
    if let Some(q) = search.map(str::trim).filter(|s| !s.is_empty()) {
        // Escape LIKE wildcards so a search for e.g. "50%" matches literally.
        clauses.push(format!(
            r"(t.description LIKE ?{0} ESCAPE '\' OR c.name LIKE ?{0} ESCAPE '\')",
            values.len() + 1
        ));
        values.push(Value::Text(format!("%{}%", escape_like(q))));
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
    ensure_account_exists(&conn, account_id)?;
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
    compute_summary(&conn)
}

/// Core of `get_summary`, decoupled from Tauri state so it can be tested
/// directly against a `Connection`.
fn compute_summary(conn: &Connection) -> CmdResult<Summary> {
    // Internal transfers are excluded from income/expense totals but still
    // count toward account balances.
    let total_balance: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM transactions",
            [],
            |r| r.get(0),
        )
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
    import_csv_into(&mut conn, account_id, &csv_text)
}

/// Core of `import_csv`, decoupled from Tauri state so it can be tested directly
/// against a `Connection`.
fn import_csv_into(
    conn: &mut Connection,
    account_id: i64,
    csv_text: &str,
) -> CmdResult<ImportResult> {
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
///
/// Currency symbols, spaces and thousands separators are stripped first, then
/// the remaining decimal is converted to cents using integer arithmetic so no
/// floating-point rounding error can creep in. Mirrors `parseAmountToCents` in
/// the front end (`src/format.ts`); keep the two in sync.
fn parse_amount_cents(raw: &str) -> CmdResult<i64> {
    // Strip currency symbols, spaces and thousands separators.
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '-' || *c == '.')
        .collect();
    decimal_str_to_cents(&cleaned).ok_or_else(|| format!("Invalid amount \"{raw}\""))
}

/// Convert a cleaned decimal string (digits, an optional leading `-`, and at
/// most one `.`) into integer cents, rounding half-up on the third decimal.
/// Returns `None` for anything that is not a well-formed number.
fn decimal_str_to_cents(s: &str) -> Option<i64> {
    let negative = s.starts_with('-');
    let body = s.strip_prefix('-').unwrap_or(s);
    if body.is_empty() {
        return None;
    }

    let mut parts = body.splitn(2, '.');
    let int_part = parts.next().unwrap_or("");
    let frac_part = parts.next().unwrap_or("");

    // A second '.' (e.g. "1.2.3") lands inside frac_part; reject it.
    if frac_part.contains('.') {
        return None;
    }
    // Need at least one digit somewhere, and every char must be a digit.
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    if !int_part.chars().all(|c| c.is_ascii_digit())
        || !frac_part.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }

    let int_val: i64 = if int_part.is_empty() {
        0
    } else {
        int_part.parse().ok()?
    };

    let digit = |i: usize| frac_part.as_bytes().get(i).map_or(0, |b| (b - b'0') as i64);
    let mut cents = int_val.checked_mul(100)? + digit(0) * 10 + digit(1);
    // Round half-up based on the third fractional digit, if present.
    if digit(2) >= 5 {
        cents += 1;
    }

    Some(if negative { -cents } else { cents })
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    // --- parse_amount_cents / decimal_str_to_cents ---------------------------

    #[test]
    fn parses_plain_decimals_without_float_error() {
        assert_eq!(parse_amount_cents("12.34").unwrap(), 1234);
        assert_eq!(parse_amount_cents("-12.34").unwrap(), -1234);
        assert_eq!(parse_amount_cents("1500.00").unwrap(), 150000);
        assert_eq!(parse_amount_cents("0.01").unwrap(), 1);
        assert_eq!(parse_amount_cents("0").unwrap(), 0);
        // 0.29 is the classic binary-float trap; integer parsing nails it.
        assert_eq!(parse_amount_cents("0.29").unwrap(), 29);
    }

    #[test]
    fn parses_partial_and_shorthand_decimals() {
        assert_eq!(parse_amount_cents("5").unwrap(), 500);
        assert_eq!(parse_amount_cents("5.5").unwrap(), 550);
        assert_eq!(parse_amount_cents(".5").unwrap(), 50);
        assert_eq!(parse_amount_cents("-.5").unwrap(), -50);
    }

    #[test]
    fn strips_currency_symbols_and_thousands_separators() {
        assert_eq!(parse_amount_cents("$1234.56").unwrap(), 123456);
        assert_eq!(parse_amount_cents("€ 42.90").unwrap(), 4290);
    }

    #[test]
    fn rounds_half_up_on_the_third_decimal() {
        assert_eq!(parse_amount_cents("12.345").unwrap(), 1235);
        assert_eq!(parse_amount_cents("12.344").unwrap(), 1234);
        assert_eq!(parse_amount_cents("-12.345").unwrap(), -1235);
    }

    #[test]
    fn rejects_malformed_amounts() {
        for bad in ["", "-", "abc", "1.2.3", ".", "--5"] {
            assert!(parse_amount_cents(bad).is_err(), "expected {bad:?} to fail");
        }
    }

    // --- normalize_date / validate_date --------------------------------------

    #[test]
    fn normalizes_common_date_layouts() {
        assert_eq!(normalize_date("2026-01-05").unwrap(), "2026-01-05");
        assert_eq!(normalize_date("05/01/2026").unwrap(), "2026-01-05"); // DD/MM/YYYY
        assert_eq!(normalize_date("2026/01/05").unwrap(), "2026-01-05");
    }

    #[test]
    fn rejects_invalid_dates() {
        assert!(normalize_date("not-a-date").is_err());
        assert!(validate_date("2026-13-40").is_err());
        assert!(validate_date("2026-02-15").is_ok());
    }

    // --- import_csv_into (integration against a real SQLite schema) -----------

    /// Insert a top-level account and return its id.
    fn seed_account(conn: &Connection, name: &str) -> i64 {
        conn.execute("INSERT INTO accounts (name) VALUES (?1)", params![name])
            .unwrap();
        conn.last_insert_rowid()
    }

    /// Insert a minimal transaction with the given description.
    fn seed_tx(conn: &Connection, account_id: i64, description: &str) {
        conn.execute(
            "INSERT INTO transactions (account_id, date, amount, description)
             VALUES (?1, '2026-01-01', -100, ?2)",
            params![account_id, description],
        )
        .unwrap();
    }

    #[test]
    fn import_inserts_rows_and_creates_categories() {
        let mut conn = db::open_in_memory().unwrap();
        let acc = seed_account(&conn, "Checking");
        let csv = "date,amount,description,category\n\
                   2026-01-05,-42.90,Grocery store,Groceries\n\
                   2026-01-06,1500.00,Salary,Income\n";

        let result = import_csv_into(&mut conn, acc, csv).unwrap();
        assert_eq!(result.imported, 2);
        assert_eq!(result.skipped_duplicates, 0);
        assert!(result.errors.is_empty());

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
        // A brand-new "Groceries"/"Income" pair are among the seeded defaults, so
        // no duplicates should have been created.
        let cats: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM categories WHERE name IN ('Groceries','Income')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cats, 2);
    }

    #[test]
    fn import_is_idempotent_on_reimport() {
        let mut conn = db::open_in_memory().unwrap();
        let acc = seed_account(&conn, "Checking");
        let csv = "date,amount,description\n2026-01-05,-42.90,Grocery store\n";

        let first = import_csv_into(&mut conn, acc, csv).unwrap();
        assert_eq!(first.imported, 1);

        let second = import_csv_into(&mut conn, acc, csv).unwrap();
        assert_eq!(second.imported, 0);
        assert_eq!(second.skipped_duplicates, 1);
    }

    #[test]
    fn import_reports_bad_rows_but_keeps_going() {
        let mut conn = db::open_in_memory().unwrap();
        let acc = seed_account(&conn, "Checking");
        let csv = "date,amount,description\n\
                   not-a-date,-1.00,Bad date\n\
                   2026-01-06,oops,Bad amount\n\
                   2026-01-07,-9.99,Good row\n";

        let result = import_csv_into(&mut conn, acc, csv).unwrap();
        assert_eq!(result.imported, 1);
        assert_eq!(result.errors.len(), 2);
    }

    #[test]
    fn import_requires_expected_columns() {
        let mut conn = db::open_in_memory().unwrap();
        let acc = seed_account(&conn, "Checking");
        let err = import_csv_into(&mut conn, acc, "foo,bar\n1,2\n").unwrap_err();
        assert!(err.contains("date"), "unexpected error: {err}");
    }

    // --- compute_summary -----------------------------------------------------

    #[test]
    fn summary_excludes_internal_transfers_from_income_and_expense() {
        let conn = db::open_in_memory().unwrap();
        let acc = seed_account(&conn, "Checking");
        let insert = |amount: i64, transfer: i64| {
            conn.execute(
                "INSERT INTO transactions (account_id, date, amount, is_internal_transfer)
                 VALUES (?1, '2026-01-01', ?2, ?3)",
                params![acc, amount, transfer],
            )
            .unwrap();
        };
        insert(150000, 0); // income
        insert(-42_90, 0); // expense
        insert(-80000, 1); // internal transfer: balance only

        let s = compute_summary(&conn).unwrap();
        assert_eq!(s.income, 150000);
        assert_eq!(s.expenses, -4290);
        assert_eq!(s.total_balance, 150000 - 4290 - 80000);
        assert_eq!(s.transaction_count, 3);
        assert_eq!(s.account_count, 1);
    }

    #[test]
    fn deleting_account_cascades_to_transactions() {
        let conn = db::open_in_memory().unwrap();
        let acc = seed_account(&conn, "Checking");
        conn.execute(
            "INSERT INTO transactions (account_id, date, amount) VALUES (?1, '2026-01-01', 100)",
            params![acc],
        )
        .unwrap();
        conn.execute("DELETE FROM accounts WHERE id = ?1", params![acc])
            .unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn deleting_parent_cascades_through_nested_subaccounts() {
        let conn = db::open_in_memory().unwrap();
        let parent = seed_account(&conn, "Parent");
        conn.execute(
            "INSERT INTO accounts (name, parent_id) VALUES ('Child', ?1)",
            params![parent],
        )
        .unwrap();
        let child = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO accounts (name, parent_id) VALUES ('Grandchild', ?1)",
            params![child],
        )
        .unwrap();
        let grandchild = conn.last_insert_rowid();
        seed_tx(&conn, grandchild, "deep");

        conn.execute("DELETE FROM accounts WHERE id = ?1", params![parent])
            .unwrap();

        // The cascade recurses through every level, taking the transaction too.
        let accts: i64 = conn
            .query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))
            .unwrap();
        let txs: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!((accts, txs), (0, 0));
    }

    // --- categories ----------------------------------------------------------

    #[test]
    fn get_or_create_category_dedupes_case_insensitively() {
        let conn = db::open_in_memory().unwrap();
        let a = get_or_create_category(&conn, "Coffee").unwrap();
        let b = get_or_create_category(&conn, "  coffee ").unwrap();
        assert_eq!(a, b, "trim + case should map to the same category");
        // A seeded default is matched, not duplicated.
        get_or_create_category(&conn, "groceries").unwrap();
        let dupes: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM categories WHERE name = 'Groceries' COLLATE NOCASE",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(dupes, 1);
        assert!(get_or_create_category(&conn, "   ").is_err());
    }

    // --- helpers & query builder ---------------------------------------------

    #[test]
    fn ensure_account_exists_reports_missing_accounts() {
        let conn = db::open_in_memory().unwrap();
        let acc = seed_account(&conn, "Checking");
        assert!(ensure_account_exists(&conn, acc).is_ok());
        assert!(ensure_account_exists(&conn, 9999).is_err());
    }

    #[test]
    fn escape_like_neutralizes_wildcards() {
        assert_eq!(escape_like("50%"), r"50\%");
        assert_eq!(escape_like("a_b"), r"a\_b");
        assert_eq!(escape_like(r"c\d"), r"c\\d");
        assert_eq!(escape_like("plain"), "plain");
    }

    #[test]
    fn search_matches_wildcard_characters_literally() {
        let conn = db::open_in_memory().unwrap();
        let acc = seed_account(&conn, "Checking");
        seed_tx(&conn, acc, "50% off sale");
        seed_tx(&conn, acc, "regular price");

        // Without escaping, "%" would match every row; escaped, it matches one.
        let hits = query_transactions(&conn, None, Some("50%")).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].description, "50% off sale");

        let plain = query_transactions(&conn, None, Some("price")).unwrap();
        assert_eq!(plain.len(), 1);
    }

    #[test]
    fn query_transactions_filters_by_account() {
        let conn = db::open_in_memory().unwrap();
        let a = seed_account(&conn, "A");
        let b = seed_account(&conn, "B");
        seed_tx(&conn, a, "in a");
        seed_tx(&conn, b, "in b");

        assert_eq!(query_transactions(&conn, Some(a), None).unwrap().len(), 1);
        assert_eq!(query_transactions(&conn, None, None).unwrap().len(), 2);
        // Blank/whitespace search is ignored, not treated as a filter.
        assert_eq!(
            query_transactions(&conn, None, Some("  ")).unwrap().len(),
            2
        );
    }
}
