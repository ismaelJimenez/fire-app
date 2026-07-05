use crate::importers::{self, validate_date};
use crate::models::{
    Account, Category, ClassificationRule, ImportPreviewRow, ImportResult, Summary, Transaction,
};
use crate::AppState;
use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use std::collections::HashSet;
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
    // `balance` (opening balance plus each account's own transactions) and
    // `tx_count` cover the account itself only; the front end rolls a parent's
    // subaccounts up for display.
    let mut stmt = conn
        .prepare(
            "SELECT a.id, a.name, a.parent_id, a.created_at, a.opening_balance,
                    a.opening_balance + COALESCE(SUM(t.amount), 0) AS balance,
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
                opening_balance: r.get(4)?,
                balance: r.get(5)?,
                tx_count: r.get(6)?,
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

/// Set an account's starting balance, in cents.
///
/// Bank exports are often partial — they only reach back so far — so this lets a
/// user anchor an account to a known figure. The opening balance is added to the
/// transaction sum for the displayed balance and the dashboard total, but is
/// never counted as income or expense. Pass `0` to clear it.
#[tauri::command]
pub fn set_account_opening_balance(
    state: State<AppState>,
    id: i64,
    opening_balance: i64,
) -> CmdResult<()> {
    let conn = state.db.lock().map_err(e)?;
    ensure_account_exists(&conn, id)?;
    conn.execute(
        "UPDATE accounts SET opening_balance = ?1 WHERE id = ?2",
        params![opening_balance, id],
    )
    .map_err(e)?;
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
    limit: Option<i64>,
) -> CmdResult<Vec<Transaction>> {
    let conn = state.db.lock().map_err(e)?;
    query_transactions(&conn, account_id, search.as_deref(), limit)
}

/// Core of `list_transactions`, decoupled from Tauri state so the filter/search
/// SQL builder can be tested directly against a `Connection`.
///
/// `limit` caps the number of rows returned (newest first); callers that only
/// need a preview — e.g. the dashboard's recent activity — pass a small value so
/// the whole table isn't loaded to show a handful of rows.
fn query_transactions(
    conn: &Connection,
    account_id: Option<i64>,
    search: Option<&str>,
    limit: Option<i64>,
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
    if let Some(n) = limit {
        sql.push_str(&format!(" LIMIT ?{}", values.len() + 1));
        values.push(Value::Integer(n));
    }

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
    account_id: i64,
    date: String,
    amount: i64,
    description: String,
    category_id: Option<i64>,
) -> CmdResult<()> {
    let conn = state.db.lock().map_err(e)?;
    update_transaction_into(
        &conn,
        id,
        account_id,
        &date,
        amount,
        &description,
        category_id,
    )
}

/// Core of `update_transaction`, decoupled from Tauri state so it can be tested
/// directly against a `Connection`.
fn update_transaction_into(
    conn: &Connection,
    id: i64,
    account_id: i64,
    date: &str,
    amount: i64,
    description: &str,
    category_id: Option<i64>,
) -> CmdResult<()> {
    validate_date(date)?;
    // The edit form can move a transaction to another account, so persist
    // account_id too; guard it the same way create_transaction does.
    ensure_account_exists(conn, account_id)?;
    conn.execute(
        "UPDATE transactions
         SET account_id = ?1, date = ?2, amount = ?3, description = ?4, category_id = ?5
         WHERE id = ?6",
        params![
            account_id,
            date,
            amount,
            description.trim(),
            category_id,
            id
        ],
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
    // null-safe, so uncategorized rows (category_id IS NULL) are kept — but only
    // because the transfer category is always present (seeded on init, deletion
    // blocked by an app guard and a DB trigger). If that subquery could ever
    // return NULL, `category_id IS NOT NULL` would wrongly drop the uncategorized
    // rows; the invariant is what keeps this correct.
    // Balance is every transaction plus every account's starting balance, so the
    // dashboard total matches the sum of the per-account balances.
    let total_balance: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(amount), 0) FROM transactions",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map_err(e)?
        + conn
            .query_row(
                "SELECT COALESCE(SUM(opening_balance), 0) FROM accounts",
                [],
                |r| r.get::<_, i64>(0),
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

/// Report which bank format a CSV would be parsed as, without importing anything.
///
/// The front end calls this when a file is loaded so it can show the detected
/// format (and make a silent misdetection — e.g. an unrecognized bank falling back
/// to the canonical template — visible before the user imports).
#[tauri::command]
pub fn detect_bank(csv_text: String) -> String {
    importers::detect_format(&csv_text).label().to_string()
}

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
#[tauri::command]
pub fn import_csv(
    state: State<AppState>,
    account_id: i64,
    csv_text: String,
    dry_run: bool,
) -> CmdResult<ImportResult> {
    let mut conn = state.db.lock().map_err(e)?;
    import_csv_into(&mut conn, account_id, &csv_text, dry_run)
}

/// Core of `import_csv`, decoupled from Tauri state so it can be tested directly
/// against a `Connection`.
///
/// With `dry_run` set nothing is written: no categories are created, no rows are
/// inserted, and the transaction is rolled back. Instead `result.preview` is filled
/// with the outcome each parsed row *would* have, so the UI can show the user what
/// committing would do.
fn import_csv_into(
    conn: &mut Connection,
    account_id: i64,
    csv_text: &str,
    dry_run: bool,
) -> CmdResult<ImportResult> {
    let (rows, mut errors) = importers::parse(csv_text)?;

    let mut result = ImportResult {
        imported: 0,
        skipped_duplicates: 0,
        errors: Vec::new(),
        preview: Vec::new(),
    };

    // On a dry run nothing is committed, so the DB duplicate check can't see rows
    // earlier in this same file. Track their identity keys here so an in-file repeat
    // still reads as a duplicate, matching what a real import (which sees its own
    // inserts) does.
    let mut seen: HashSet<String> = HashSet::new();

    let tx = conn.transaction().map_err(e)?;
    for row in &rows {
        // Category precedence: explicit column, then a learned rule for the
        // concept, then nothing. `is_auto_classified` marks rule-driven matches.
        // `new_category` flags an explicit category that doesn't exist yet.
        let (category_id, category_name, auto, new_category): (
            Option<i64>,
            Option<String>,
            bool,
            bool,
        ) = if let Some(name) = &row.category {
            if dry_run {
                let existing = find_category(&tx, name)?;
                (existing, Some(name.clone()), false, existing.is_none())
            } else {
                match get_or_create_category(&tx, name) {
                    Ok(id) => (Some(id), Some(name.clone()), false, false),
                    Err(err) => {
                        errors.push(err);
                        (None, None, false, false)
                    }
                }
            }
        } else if !row.counterparty.trim().is_empty() {
            match lookup_rule(&tx, &row.counterparty)? {
                Some(id) => (Some(id), category_name_by_id(&tx, id)?, true, false),
                None => (None, None, false, false),
            }
        } else {
            (None, None, false, false)
        };

        // Duplicate guard (also makes re-import of a verified row a no-op). When the
        // export carries a per-transaction reference, that is the identity — so two
        // genuinely distinct charges sharing a date, amount, and merchant are kept
        // apart. Rows without a reference (and legacy rows imported before references
        // existed) fall back to the date/amount/description identity.
        let has_ref = !row.import_ref.is_empty();
        let exists_in_db: bool = if has_ref {
            tx.query_row(
                "SELECT 1 FROM transactions
                 WHERE account_id = ?1
                   AND (import_ref = ?2
                        OR (import_ref = '' AND date = ?3 AND amount = ?4 AND description = ?5))
                 LIMIT 1",
                params![
                    account_id,
                    row.import_ref,
                    row.date,
                    row.amount_cents,
                    row.description
                ],
                |_| Ok(()),
            )
            .is_ok()
        } else {
            tx.query_row(
                "SELECT 1 FROM transactions
                 WHERE account_id = ?1 AND date = ?2 AND amount = ?3 AND description = ?4
                 LIMIT 1",
                params![account_id, row.date, row.amount_cents, row.description],
                |_| Ok(()),
            )
            .is_ok()
        };

        // On a dry run nothing is committed, so the DB check can't see rows earlier
        // in this same file; track them here so an in-file repeat still reads as a
        // duplicate, matching a real import (which sees its own inserts). Keyed by
        // reference when there is one, otherwise by date/amount/description.
        let seen_key = if has_ref {
            format!("ref:{}", row.import_ref)
        } else {
            format!("{}|{}|{}", row.date, row.amount_cents, row.description)
        };
        let exists = exists_in_db || (dry_run && !seen.insert(seen_key));

        if dry_run {
            result.preview.push(ImportPreviewRow {
                date: row.date.clone(),
                amount: row.amount_cents,
                description: row.description.clone(),
                counterparty: row.counterparty.clone(),
                category: category_name,
                auto_classified: auto,
                new_category: new_category && !exists,
                duplicate: exists,
            });
        }

        if exists {
            result.skipped_duplicates += 1;
            continue;
        }

        if !dry_run {
            tx.execute(
                "INSERT INTO transactions
                    (account_id, date, amount, description, counterparty, category_id, is_auto_classified, import_ref)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    account_id,
                    row.date,
                    row.amount_cents,
                    row.description,
                    row.counterparty,
                    category_id,
                    auto as i64,
                    row.import_ref
                ],
            )
            .map_err(e)?;
        }
        result.imported += 1;
    }

    // A dry run rolls back (drop without commit); a real run persists.
    if dry_run {
        tx.rollback().map_err(e)?;
    } else {
        tx.commit().map_err(e)?;
    }

    result.errors = errors;
    Ok(result)
}

/// Read-only lookup of a category id by name (case-insensitive). Unlike
/// [`get_or_create_category`], never creates — used by dry-run preview.
fn find_category(conn: &Connection, name: &str) -> CmdResult<Option<i64>> {
    conn.query_row(
        "SELECT id FROM categories WHERE name = ?1 COLLATE NOCASE",
        params![name.trim()],
        |r| r.get::<_, i64>(0),
    )
    .optional()
    .map_err(e)
}

/// The display name of a category by id, if it still exists.
fn category_name_by_id(conn: &Connection, id: i64) -> CmdResult<Option<String>> {
    conn.query_row(
        "SELECT name FROM categories WHERE id = ?1",
        params![id],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .map_err(e)
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
    fn deutsche_bank_shared_kundenreferenz_is_not_a_duplicate() {
        // Deutsche Bank routinely fills Kundenreferenz with a repeated placeholder
        // ("NOTPROVIDED") on unrelated rows. These are three distinct purchases and
        // must all import — none flagged as duplicates — even against an empty
        // account in a dry run.
        let csv = "\
Umsätze\n\
Buchungstag;Wert;Umsatzart;Begünstigter / Auftraggeber;Verwendungszweck;IBAN / Kontonummer;BIC;Kundenreferenz;Mandatsreferenz;Gläubiger ID;Fremde Gebühren;Betrag;Abweichender Empfänger;Anzahl der Aufträge;Anzahl der Schecks;Soll;Haben;Währung\n\
15.6.2026;15.6.2026;Kartenzahlung;REWE Markt;Einkauf;;;NOTPROVIDED;;;;-12,90;;;;-12,90;;EUR\n\
16.6.2026;16.6.2026;Kartenzahlung;ALDI;Einkauf;;;NOTPROVIDED;;;;-8,40;;;;-8,40;;EUR\n\
17.6.2026;17.6.2026;Kartenzahlung;DM;Einkauf;;;NOTPROVIDED;;;;-5,10;;;;-5,10;;EUR\n";

        let mut conn = db::open_in_memory().unwrap();
        let acc = seed_account(&conn, "Giro");
        let res = import_csv_into(&mut conn, acc, csv, true).unwrap();
        assert_eq!(res.imported, 3);
        assert_eq!(res.skipped_duplicates, 0);
        assert!(res.preview.iter().all(|r| !r.duplicate));
    }

    #[test]
    fn import_inserts_rows_and_creates_categories() {
        let mut conn = db::open_in_memory().unwrap();
        let acc = seed_account(&conn, "Checking");
        let csv = "date,amount,description,category\n\
                   2026-01-05,-42.90,Grocery store,Groceries\n\
                   2026-01-06,1500.00,Salary,Income\n";

        let result = import_csv_into(&mut conn, acc, csv, false).unwrap();
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

        let first = import_csv_into(&mut conn, acc, csv, false).unwrap();
        assert_eq!(first.imported, 1);

        let second = import_csv_into(&mut conn, acc, csv, false).unwrap();
        assert_eq!(second.imported, 0);
        assert_eq!(second.skipped_duplicates, 1);
    }

    #[test]
    fn dry_run_writes_nothing_but_previews_every_row() {
        let mut conn = db::open_in_memory().unwrap();
        let acc = seed_account(&conn, "Checking");
        let csv = "date,amount,description,category\n\
                   2026-01-05,-42.90,Grocery store,Groceries\n\
                   2026-01-06,1500.00,Salary,A Brand New Category\n";

        let res = import_csv_into(&mut conn, acc, csv, true).unwrap();
        assert_eq!(res.imported, 2);
        assert_eq!(res.skipped_duplicates, 0);
        assert_eq!(res.preview.len(), 2);

        // Nothing was written: no transactions and no new category.
        let tx_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tx_count, 0);
        let brand_new: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM categories WHERE name = 'A Brand New Category'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(brand_new, 0);

        // Groceries is a seeded default, so it isn't flagged new; the second row's
        // category would be created.
        assert_eq!(res.preview[0].category.as_deref(), Some("Groceries"));
        assert!(!res.preview[0].new_category);
        assert_eq!(
            res.preview[1].category.as_deref(),
            Some("A Brand New Category")
        );
        assert!(res.preview[1].new_category);
    }

    #[test]
    fn dry_run_flags_existing_and_in_file_duplicates() {
        let mut conn = db::open_in_memory().unwrap();
        let acc = seed_account(&conn, "Checking");
        let csv = "date,amount,description\n2026-01-05,-42.90,Grocery store\n";
        import_csv_into(&mut conn, acc, csv, false).unwrap();

        // Re-previewing the already-imported row, plus a repeat within the same file.
        let csv2 = "date,amount,description\n\
                    2026-01-05,-42.90,Grocery store\n\
                    2026-02-01,-9.99,New one\n\
                    2026-02-01,-9.99,New one\n";
        let res = import_csv_into(&mut conn, acc, csv2, true).unwrap();
        assert_eq!(res.imported, 1);
        assert_eq!(res.skipped_duplicates, 2);
        // Row 0 duplicates the DB; row 3 duplicates row 2 within the file.
        assert!(res.preview[0].duplicate);
        assert!(!res.preview[1].duplicate);
        assert!(res.preview[2].duplicate);
    }

    #[test]
    fn dry_run_surfaces_learned_classification() {
        let mut conn = db::open_in_memory().unwrap();
        let acc = seed_account(&conn, "Checking");
        // Import an ING row, then teach a rule for its counterparty.
        import_csv_into(&mut conn, acc, ING, false).unwrap();
        let cat = get_or_create_category(&conn, "Salary").unwrap();
        upsert_rule(&conn, "BMW Car IT GmbH", cat).unwrap();

        // A fresh (non-duplicate) ING row from the same payee previews as auto-classified.
        let ing2 = "Umsatzanzeige;x\n\n\
            Buchung;Wertstellungsdatum;Auftraggeber/Empfänger;Buchungstext;Verwendungszweck;Saldo;Währung;Betrag;Währung\n\
            20.06.2026;20.06.2026;BMW Car IT GmbH;Gehalt/Rente;July pay;100,00;EUR;5.000,00;EUR\n";
        let res = import_csv_into(&mut conn, acc, ing2, true).unwrap();
        assert_eq!(res.preview.len(), 1);
        assert_eq!(res.preview[0].category.as_deref(), Some("Salary"));
        assert!(res.preview[0].auto_classified);
        assert!(!res.preview[0].duplicate);
    }

    // A comdirect Visa export where several genuinely distinct charges share a
    // date, amount, and merchant — distinguished only by their Referenz. Keying
    // duplicates on the reference must keep all of them.
    const VISA_REPEATS: &str = "\
;\n\
\"Umsätze Visa-Karte (Kreditkarte) ..9255\";\"Zeitraum\";\n\
\"Neuer Kontostand\";\"0,00 EUR\";\n\
\n\
\"Buchungstag\";\"Umsatztag\";\"Vorgang\";\"Referenz\";\"Buchungstext\";\"Umsatz in EUR\";\n\
\"16.05.2026\";\"16.05.2026\";\"Kartenumsatz\";\"140803816503\";\" NYX HAPPYGAMESSL \";\"-1,00\";\n\
\"16.05.2026\";\"16.05.2026\";\"Kartenumsatz\";\"140787949803\";\" NYX HAPPYGAMESSL \";\"-1,00\";\n\
\"16.05.2026\";\"16.05.2026\";\"Kartenumsatz\";\"140785418703\";\" NYX HAPPYGAMESSL \";\"-1,00\";\n";

    #[test]
    fn import_keeps_distinct_charges_with_different_references() {
        let mut conn = db::open_in_memory().unwrap();
        let acc = seed_account(&conn, "Visa");

        // All three are distinct despite identical date/amount/merchant.
        let res = import_csv_into(&mut conn, acc, VISA_REPEATS, false).unwrap();
        assert_eq!(res.imported, 3);
        assert_eq!(res.skipped_duplicates, 0);
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 3);

        // Re-importing the same file is still idempotent — the references match.
        let again = import_csv_into(&mut conn, acc, VISA_REPEATS, false).unwrap();
        assert_eq!(again.imported, 0);
        assert_eq!(again.skipped_duplicates, 3);
    }

    #[test]
    fn dry_run_previews_reference_repeats_as_new() {
        let mut conn = db::open_in_memory().unwrap();
        let acc = seed_account(&conn, "Visa");
        let res = import_csv_into(&mut conn, acc, VISA_REPEATS, true).unwrap();
        assert_eq!(res.imported, 3);
        assert_eq!(res.skipped_duplicates, 0);
        assert!(res.preview.iter().all(|r| !r.duplicate));
    }

    #[test]
    fn import_dedupes_repeated_reference_within_one_file() {
        let mut conn = db::open_in_memory().unwrap();
        let acc = seed_account(&conn, "Visa");
        // The same Referenz twice in one file is a true duplicate.
        let csv = "\
\"Buchungstag\";\"Umsatztag\";\"Vorgang\";\"Referenz\";\"Buchungstext\";\"Umsatz in EUR\";\n\
\"16.05.2026\";\"16.05.2026\";\"Kartenumsatz\";\"140803816503\";\" NYX HAPPYGAMESSL \";\"-1,00\";\n\
\"16.05.2026\";\"16.05.2026\";\"Kartenumsatz\";\"140803816503\";\" NYX HAPPYGAMESSL \";\"-1,00\";\n";
        let res = import_csv_into(&mut conn, acc, csv, false).unwrap();
        assert_eq!(res.imported, 1);
        assert_eq!(res.skipped_duplicates, 1);
    }

    #[test]
    fn import_reports_bad_rows_but_keeps_going() {
        let mut conn = db::open_in_memory().unwrap();
        let acc = seed_account(&conn, "Checking");
        let csv = "date,amount,description\n\
                   not-a-date,-1.00,Bad date\n\
                   2026-01-06,oops,Bad amount\n\
                   2026-01-07,-9.99,Good row\n";

        let result = import_csv_into(&mut conn, acc, csv, false).unwrap();
        assert_eq!(result.imported, 1);
        assert_eq!(result.errors.len(), 2);
    }

    #[test]
    fn import_requires_expected_columns() {
        let mut conn = db::open_in_memory().unwrap();
        let acc = seed_account(&conn, "Checking");
        let err = import_csv_into(&mut conn, acc, "foo,bar\n1,2\n", false).unwrap_err();
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
        let res = import_csv_into(&mut conn, acc, ING, false).unwrap();
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

        import_csv_into(&mut conn, acc, ING, false).unwrap();
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
        import_csv_into(&mut conn, acc, ING, false).unwrap();

        // Hand-categorize and verify one row.
        let savings = cat(&conn, "Savings");
        conn.execute(
            "UPDATE transactions SET is_verified = 1, category_id = ?1
             WHERE description LIKE 'Entgelt%'",
            params![savings],
        )
        .unwrap();

        // Re-importing the same file changes nothing.
        let res = import_csv_into(&mut conn, acc, ING, false).unwrap();
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
    fn opening_balance_folds_into_the_total_but_not_income_or_expense() {
        let conn = db::open_in_memory().unwrap();
        let acc = seed_account(&conn, "Checking");
        conn.execute(
            "UPDATE accounts SET opening_balance = 100000 WHERE id = ?1",
            params![acc],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transactions (account_id, date, amount) VALUES (?1, '2026-01-01', 5000)",
            params![acc],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO transactions (account_id, date, amount) VALUES (?1, '2026-01-02', -2000)",
            params![acc],
        )
        .unwrap();

        let s = compute_summary(&conn).unwrap();
        // Opening balance rides on the total...
        assert_eq!(s.total_balance, 100000 + 5000 - 2000);
        // ...but income and expenses only reflect the transactions.
        assert_eq!(s.income, 5000);
        assert_eq!(s.expenses, -2000);
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
        let hits = query_transactions(&conn, None, Some("50%"), None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].description, "50% off sale");

        let plain = query_transactions(&conn, None, Some("price"), None).unwrap();
        assert_eq!(plain.len(), 1);
    }

    #[test]
    fn query_transactions_filters_by_account() {
        let conn = db::open_in_memory().unwrap();
        let a = seed_account(&conn, "A");
        let b = seed_account(&conn, "B");
        seed_tx(&conn, a, "in a");
        seed_tx(&conn, b, "in b");

        assert_eq!(
            query_transactions(&conn, Some(a), None, None)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            query_transactions(&conn, None, None, None).unwrap().len(),
            2
        );
        // Blank/whitespace search is ignored, not treated as a filter.
        assert_eq!(
            query_transactions(&conn, None, Some("  "), None)
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn update_transaction_moves_the_row_to_another_account() {
        let conn = db::open_in_memory().unwrap();
        let from = seed_account(&conn, "Checking");
        let to = seed_account(&conn, "Savings");
        conn.execute(
            "INSERT INTO transactions (account_id, date, amount, description)
             VALUES (?1, '2026-01-01', -100, 'Coffee')",
            params![from],
        )
        .unwrap();
        let id = conn.last_insert_rowid();

        // Editing the account (plus other fields) actually re-homes the row.
        update_transaction_into(&conn, id, to, "2026-02-02", -250, "Latte", None).unwrap();
        let (acc, date, amount, desc): (i64, String, i64, String) = conn
            .query_row(
                "SELECT account_id, date, amount, description FROM transactions WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            (acc, date.as_str(), amount, desc.as_str()),
            (to, "2026-02-02", -250, "Latte")
        );

        // A move to a non-existent account is rejected, not silently applied.
        assert!(
            update_transaction_into(&conn, id, 9999, "2026-02-02", -250, "Latte", None).is_err()
        );
    }

    #[test]
    fn query_transactions_caps_rows_at_the_limit() {
        let conn = db::open_in_memory().unwrap();
        let acc = seed_account(&conn, "Checking");
        for i in 0..5 {
            conn.execute(
                "INSERT INTO transactions (account_id, date, amount, description)
                 VALUES (?1, ?2, -100, 'tx')",
                params![acc, format!("2026-01-0{}", i + 1)],
            )
            .unwrap();
        }
        // Newest first, capped to the limit.
        let rows = query_transactions(&conn, None, None, Some(2)).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].date, "2026-01-05");
        assert_eq!(rows[1].date, "2026-01-04");
        // No limit returns everything.
        assert_eq!(
            query_transactions(&conn, None, None, None).unwrap().len(),
            5
        );
    }
}
