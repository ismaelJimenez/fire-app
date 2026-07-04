use crate::importers::{self, validate_date};
use crate::models::{Account, Category, ClassificationRule, ImportResult, Summary, Transaction};
use crate::AppState;
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
        .prepare("SELECT id, name, is_transfer FROM categories ORDER BY name COLLATE NOCASE")
        .map_err(e)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(Category {
                id: r.get(0)?,
                name: r.get(1)?,
                is_transfer: r.get::<_, i64>(2)? != 0,
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
    // The built-in transfer category is load-bearing for the dashboard totals and
    // must not be deleted out from under them.
    let is_transfer: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM categories WHERE id = ?1 AND is_transfer = 1",
            params![id],
            |r| r.get(0),
        )
        .map_err(e)?;
    if is_transfer > 0 {
        return Err("The Transfer category is built in and can't be deleted.".into());
    }
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
        t.counterparty, t.category_id, c.name,
        t.is_verified, t.is_auto_classified, t.created_at
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
        counterparty: r.get(6)?,
        category_id: r.get(7)?,
        category_name: r.get(8)?,
        is_verified: r.get::<_, i64>(9)? != 0,
        is_auto_classified: r.get::<_, i64>(10)? != 0,
        created_at: r.get(11)?,
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

#[tauri::command]
pub fn create_transaction(
    state: State<AppState>,
    account_id: i64,
    date: String,
    amount: i64,
    description: String,
    category_id: Option<i64>,
) -> CmdResult<i64> {
    validate_date(&date)?;
    let conn = state.db.lock().map_err(e)?;
    ensure_account_exists(&conn, account_id)?;
    conn.execute(
        "INSERT INTO transactions
            (account_id, date, amount, description, category_id)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![account_id, date, amount, description.trim(), category_id],
    )
    .map_err(e)?;
    Ok(conn.last_insert_rowid())
}

#[tauri::command]
pub fn update_transaction(
    state: State<AppState>,
    id: i64,
    date: String,
    amount: i64,
    description: String,
    category_id: Option<i64>,
) -> CmdResult<()> {
    validate_date(&date)?;
    let conn = state.db.lock().map_err(e)?;
    conn.execute(
        "UPDATE transactions
         SET date = ?1, amount = ?2, description = ?3, category_id = ?4
         WHERE id = ?5",
        params![date, amount, description.trim(), category_id, id],
    )
    .map_err(e)?;
    Ok(())
}

/// Lightweight update used by the inline category picker.
///
/// Setting a category by hand also *teaches* a classification rule for the
/// transaction's counterparty (its "concept"), then applies that category to every
/// other **unverified** transaction sharing the concept. Returns how many other
/// rows were re-classified, so the UI can report the ripple effect. Clearing the
/// category (`None`) just clears this row and learns nothing.
#[tauri::command]
pub fn set_transaction_category(
    state: State<AppState>,
    id: i64,
    category_id: Option<i64>,
) -> CmdResult<i64> {
    let conn = state.db.lock().map_err(e)?;

    // The user's choice is authoritative for this row: mark it non-auto so it is
    // never silently overwritten later.
    conn.execute(
        "UPDATE transactions SET category_id = ?1, is_auto_classified = 0 WHERE id = ?2",
        params![category_id, id],
    )
    .map_err(e)?;

    let Some(cat) = category_id else {
        return Ok(0);
    };

    // Learn the rule and propagate it, keyed on this row's concept.
    let concept: String = conn
        .query_row(
            "SELECT counterparty FROM transactions WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .map_err(e)?;
    if concept.trim().is_empty() {
        return Ok(0);
    }

    upsert_rule(&conn, &concept, cat)?;
    apply_rule(&conn, &concept, cat, Some(id))
}

/// Mark a transaction reviewed (or un-review it). Verified rows are locked: they
/// are never re-categorized by rules and are skipped as duplicates on re-import.
#[tauri::command]
pub fn set_transaction_verified(state: State<AppState>, id: i64, verified: bool) -> CmdResult<()> {
    let conn = state.db.lock().map_err(e)?;
    conn.execute(
        "UPDATE transactions SET is_verified = ?1 WHERE id = ?2",
        params![verified as i64, id],
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
// Classification rules
// ----------------------------------------------------------------------------

/// Return the category a concept currently maps to, if a rule exists.
fn lookup_rule(conn: &Connection, concept: &str) -> CmdResult<Option<i64>> {
    conn.query_row(
        "SELECT category_id FROM classification_rules WHERE concept = ?1 COLLATE NOCASE",
        params![concept],
        |r| r.get::<_, i64>(0),
    )
    .map(Some)
    .or_else(|err| match err {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        other => Err(e(other)),
    })
}

/// Remember (or update) the category for a concept.
fn upsert_rule(conn: &Connection, concept: &str, category_id: i64) -> CmdResult<()> {
    conn.execute(
        "INSERT INTO classification_rules (concept, category_id) VALUES (?1, ?2)
         ON CONFLICT(concept) DO UPDATE SET category_id = excluded.category_id",
        params![concept, category_id],
    )
    .map_err(e)?;
    Ok(())
}

/// Apply a concept's category to every unverified transaction sharing it,
/// optionally excluding one row (the one the user just set by hand). Verified rows
/// are left untouched. Returns the number of rows changed.
fn apply_rule(
    conn: &Connection,
    concept: &str,
    category_id: i64,
    exclude_id: Option<i64>,
) -> CmdResult<i64> {
    let changed = conn
        .execute(
            "UPDATE transactions
             SET category_id = ?1, is_auto_classified = 1
             WHERE counterparty = ?2 COLLATE NOCASE
               AND is_verified = 0
               AND id IS NOT ?3",
            params![category_id, concept, exclude_id],
        )
        .map_err(e)?;
    Ok(changed as i64)
}

#[tauri::command]
pub fn list_rules(state: State<AppState>) -> CmdResult<Vec<ClassificationRule>> {
    let conn = state.db.lock().map_err(e)?;
    let mut stmt = conn
        .prepare(
            "SELECT r.id, r.concept, r.category_id, c.name
             FROM classification_rules r
             JOIN categories c ON c.id = r.category_id
             ORDER BY r.concept COLLATE NOCASE",
        )
        .map_err(e)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ClassificationRule {
                id: r.get(0)?,
                concept: r.get(1)?,
                category_id: r.get(2)?,
                category_name: r.get(3)?,
            })
        })
        .map_err(e)?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(e)
}

/// Forget a learned rule. Existing transactions keep whatever category they have;
/// only future auto-classification stops.
#[tauri::command]
pub fn delete_rule(state: State<AppState>, id: i64) -> CmdResult<()> {
    let conn = state.db.lock().map_err(e)?;
    conn.execute(
        "DELETE FROM classification_rules WHERE id = ?1",
        params![id],
    )
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
    // Money moved between the user's own accounts sits in the built-in transfer
    // category (flagged `is_transfer`, not matched by name); those rows are left
    // out of income/expense totals but still count toward balances. `IS NOT` is
    // null-safe, so uncategorized rows (category_id IS NULL) are kept.
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
             WHERE amount > 0
               AND category_id IS NOT (SELECT id FROM categories WHERE is_transfer = 1)",
            [],
            |r| r.get(0),
        )
        .map_err(e)?;
    let expenses: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM transactions
             WHERE amount < 0
               AND category_id IS NOT (SELECT id FROM categories WHERE is_transfer = 1)",
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
/// The format is auto-detected (see `importers`): the app's own
/// `date,amount,description,category` template, or a supported bank export such as
/// the ING-DiBa Girokonto "Umsatzanzeige" or a comdirect account "Umsätze" export.
/// The caller passes already-decoded text — the front end handles the file's encoding.
///
/// Categories are resolved per row: an explicit `category` column wins; otherwise a
/// learned classification rule for the row's counterparty applies; otherwise the row
/// lands uncategorized. Rows identical to an existing transaction (same account,
/// date, amount and description) are skipped, so re-importing the same file is safe.
/// Report which bank format a CSV would be parsed as, without importing anything.
///
/// The front end calls this when a file is loaded so it can show the detected
/// format (and make a silent misdetection — e.g. an unrecognized bank falling back
/// to the canonical template — visible before the user imports).
#[tauri::command]
pub fn detect_bank(csv_text: String) -> String {
    importers::detect_format(&csv_text).label().to_string()
}

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
    let (rows, mut errors) = importers::parse(csv_text)?;

    let mut result = ImportResult {
        imported: 0,
        skipped_duplicates: 0,
        errors: Vec::new(),
    };

    let tx = conn.transaction().map_err(e)?;
    for row in &rows {
        // Category precedence: explicit column, then a learned rule for the
        // concept, then nothing. `is_auto_classified` marks rule-driven matches.
        let (category_id, auto): (Option<i64>, bool) = if let Some(name) = &row.category {
            match get_or_create_category(&tx, name) {
                Ok(id) => (Some(id), false),
                Err(err) => {
                    errors.push(err);
                    (None, false)
                }
            }
        } else if !row.counterparty.trim().is_empty() {
            match lookup_rule(&tx, &row.counterparty)? {
                Some(id) => (Some(id), true),
                None => (None, false),
            }
        } else {
            (None, false)
        };

        // Duplicate guard (also makes re-import of a verified row a no-op).
        let exists: bool = tx
            .query_row(
                "SELECT 1 FROM transactions
                 WHERE account_id = ?1 AND date = ?2 AND amount = ?3 AND description = ?4
                 LIMIT 1",
                params![account_id, row.date, row.amount_cents, row.description],
                |_| Ok(()),
            )
            .is_ok();
        if exists {
            result.skipped_duplicates += 1;
            continue;
        }

        tx.execute(
            "INSERT INTO transactions
                (account_id, date, amount, description, counterparty, category_id, is_auto_classified)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                account_id,
                row.date,
                row.amount_cents,
                row.description,
                row.counterparty,
                category_id,
                auto as i64
            ],
        )
        .map_err(e)?;
        result.imported += 1;
    }
    tx.commit().map_err(e)?;

    result.errors = errors;
    Ok(result)
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

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

    // --- ING import + classification rules -----------------------------------

    const ING: &str = "Umsatzanzeige;Datei erstellt am: 26.06.2026\n\
\n\
Bank;ING\n\
\n\
Buchung;Wertstellungsdatum;Auftraggeber/Empfänger;Buchungstext;Verwendungszweck;Saldo;Währung;Betrag;Währung\n\
19.06.2026;19.06.2026;BMW Car IT GmbH;Gehalt/Rente;Mai;31.021,35;EUR;5.082,40;EUR\n\
22.05.2026;22.05.2026;BMW Car IT GmbH;Gehalt/Rente;April;25.961,86;EUR;5.082,40;EUR\n\
02.06.2026;02.06.2026;ING;Entgelt;GIROCARD;25.960,37;EUR;-1,49;EUR\n";

    fn cat(conn: &Connection, name: &str) -> i64 {
        get_or_create_category(conn, name).unwrap()
    }

    #[test]
    fn ing_import_parses_amounts_and_stores_counterparty() {
        let mut conn = db::open_in_memory().unwrap();
        let acc = seed_account(&conn, "ING");
        let res = import_csv_into(&mut conn, acc, ING).unwrap();
        assert_eq!(res.imported, 3);
        assert!(res.errors.is_empty(), "unexpected errors: {:?}", res.errors);
        // German amount parsed to cents; counterparty captured as the concept.
        let (amount, cp): (i64, String) = conn
            .query_row(
                "SELECT amount, counterparty FROM transactions WHERE description LIKE 'Gehalt%Mai'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(amount, 508240);
        assert_eq!(cp, "BMW Car IT GmbH");
    }

    #[test]
    fn import_auto_classifies_via_learned_rule() {
        let mut conn = db::open_in_memory().unwrap();
        let acc = seed_account(&conn, "ING");
        let income = cat(&conn, "Income");
        upsert_rule(&conn, "BMW Car IT GmbH", income).unwrap();

        import_csv_into(&mut conn, acc, ING).unwrap();
        // Both BMW rows are auto-categorized; the ING "Entgelt" row stays unset.
        let auto: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM transactions WHERE category_id = ?1 AND is_auto_classified = 1",
                params![income],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(auto, 2);
        let uncat: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM transactions WHERE category_id IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(uncat, 1);
    }

    #[test]
    fn apply_rule_hits_unverified_and_skips_verified() {
        let conn = db::open_in_memory().unwrap();
        let acc = seed_account(&conn, "ING");
        let income = cat(&conn, "Income");
        let insert = |desc: &str, verified: i64| {
            conn.execute(
                "INSERT INTO transactions
                    (account_id, date, amount, description, counterparty, is_verified)
                 VALUES (?1, '2026-01-01', 100, ?2, 'Acme', ?3)",
                params![acc, desc, verified],
            )
            .unwrap();
            conn.last_insert_rowid()
        };
        let a = insert("a", 0); // the row "just set" by hand
        let b = insert("b", 0); // sibling, should be swept in
        let c = insert("c", 1); // verified: locked

        upsert_rule(&conn, "Acme", income).unwrap();
        let changed = apply_rule(&conn, "Acme", income, Some(a)).unwrap();
        assert_eq!(changed, 1); // only `b`

        let is_null = |id: i64| {
            conn.query_row(
                "SELECT category_id IS NULL FROM transactions WHERE id = ?1",
                params![id],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
                == 1
        };
        assert!(is_null(a), "excluded row untouched");
        assert!(!is_null(b), "sibling updated");
        assert!(is_null(c), "verified row locked");
    }

    #[test]
    fn upsert_rule_overwrites_existing_concept() {
        let conn = db::open_in_memory().unwrap();
        let one = cat(&conn, "Income");
        let two = cat(&conn, "Savings");
        upsert_rule(&conn, "Acme", one).unwrap();
        upsert_rule(&conn, "Acme", two).unwrap();
        assert_eq!(lookup_rule(&conn, "Acme").unwrap(), Some(two));
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM classification_rules", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn verified_row_survives_reimport_as_duplicate() {
        let mut conn = db::open_in_memory().unwrap();
        let acc = seed_account(&conn, "ING");
        import_csv_into(&mut conn, acc, ING).unwrap();

        // Hand-categorize and verify one row.
        let savings = cat(&conn, "Savings");
        conn.execute(
            "UPDATE transactions SET is_verified = 1, category_id = ?1
             WHERE description LIKE 'Entgelt%'",
            params![savings],
        )
        .unwrap();

        // Re-importing the same file changes nothing.
        let res = import_csv_into(&mut conn, acc, ING).unwrap();
        assert_eq!(res.imported, 0);
        assert_eq!(res.skipped_duplicates, 3);
        let cat_id: Option<i64> = conn
            .query_row(
                "SELECT category_id FROM transactions WHERE description LIKE 'Entgelt%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cat_id, Some(savings));
    }

    // --- compute_summary -----------------------------------------------------

    #[test]
    fn summary_excludes_transfers_from_income_and_expense() {
        let conn = db::open_in_memory().unwrap();
        let acc = seed_account(&conn, "Checking");
        let transfer_cat: i64 = conn
            .query_row("SELECT id FROM categories WHERE is_transfer = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        let insert = |amount: i64, category: Option<i64>| {
            conn.execute(
                "INSERT INTO transactions (account_id, date, amount, category_id)
                 VALUES (?1, '2026-01-01', ?2, ?3)",
                params![acc, amount, category],
            )
            .unwrap();
        };
        insert(150000, None); // income, uncategorized
        insert(-42_90, None); // expense, uncategorized
        insert(-80000, Some(transfer_cat)); // transfer: balance only

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
