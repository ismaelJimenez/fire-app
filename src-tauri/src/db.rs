use rusqlite::Connection;
use std::path::Path;

/// Open (or create) the SQLite database at `path`, enable foreign keys,
/// and run the schema migrations.
pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    migrate(&conn)?;
    Ok(conn)
}

/// Idempotent schema creation. Amounts are stored as integer cents to avoid
/// floating-point rounding errors.
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS accounts (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            name       TEXT    NOT NULL UNIQUE,
            created_at TEXT    NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS categories (
            id   INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT    NOT NULL UNIQUE COLLATE NOCASE
        );

        CREATE TABLE IF NOT EXISTS transactions (
            id                   INTEGER PRIMARY KEY AUTOINCREMENT,
            account_id           INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
            date                 TEXT    NOT NULL,               -- ISO 8601 YYYY-MM-DD
            amount               INTEGER NOT NULL,               -- cents; negative = expense
            description          TEXT    NOT NULL DEFAULT '',
            category_id          INTEGER REFERENCES categories(id) ON DELETE SET NULL,
            is_internal_transfer INTEGER NOT NULL DEFAULT 0,
            created_at           TEXT    NOT NULL DEFAULT (datetime('now'))
        );

        CREATE INDEX IF NOT EXISTS idx_tx_account ON transactions(account_id);
        CREATE INDEX IF NOT EXISTS idx_tx_date    ON transactions(date);
        "#,
    )?;

    // Seed a handful of sensible default categories on first run.
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM categories", [], |r| r.get(0))?;
    if count == 0 {
        for name in [
            "Groceries",
            "Rent",
            "Utilities",
            "Transport",
            "Dining",
            "Shopping",
            "Health",
            "Entertainment",
            "Income",
            "Savings",
        ] {
            conn.execute("INSERT INTO categories (name) VALUES (?1)", [name])?;
        }
    }

    Ok(())
}
