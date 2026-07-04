use rusqlite::Connection;
use std::path::Path;

/// Open (or create) the SQLite database at `path`, enable foreign keys,
/// and apply the schema.
pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    init_schema(&conn)?;
    Ok(conn)
}

/// Open an in-memory database with the schema applied. Used by tests to get an
/// isolated, disposable connection without touching the filesystem.
#[cfg(test)]
pub fn open_in_memory() -> rusqlite::Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    init_schema(&conn)?;
    Ok(conn)
}

/// The default categories seeded into a fresh database, in display-independent
/// order. Add or remove entries here to change the built-in set. "Transfer" is a
/// special role (see [`init_schema`]) that money moved between the user's own
/// accounts is categorized as, and which the dashboard excludes from totals.
const DEFAULT_CATEGORIES: &[&str] = &[
    "Groceries",
    "Rent",
    "Utilities",
    "Transport",
    "Dining",
    "Shopping",
    "Health",
    "Entertainment",
    "Fees",
    "Salary",
    "Dividends",
    "Subscriptions",
    "Travel",
    "Benefits",
    "Eating Out",
    "Taxes",
    "Home & DIY",
    "Income",
    "Savings",
    "Transfer",
];

/// Create the schema and seed default categories. Amounts are stored as integer
/// cents to avoid floating-point rounding errors.
///
/// Idempotent (`CREATE ... IF NOT EXISTS`, seed-if-empty), so it is safe to run on
/// every open. There are no field databases predating this schema, so it is the
/// full shape rather than an accumulation of migrations.
fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS accounts (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            -- Unique per parent (see ux_accounts_name_parent below), so two
            -- subaccounts under different accounts may share a name.
            name       TEXT    NOT NULL,
            -- NULL = a top-level account; otherwise the parent it was split from.
            parent_id  INTEGER REFERENCES accounts(id) ON DELETE CASCADE,
            created_at TEXT    NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS categories (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            name        TEXT    NOT NULL UNIQUE COLLATE NOCASE,
            -- Role flag for the built-in "Transfer" category. Summaries and the UI
            -- identify transfers by this flag, so the category can be renamed
            -- freely without breaking totals.
            is_transfer INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS transactions (
            id                 INTEGER PRIMARY KEY AUTOINCREMENT,
            account_id         INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
            date               TEXT    NOT NULL,               -- ISO 8601 YYYY-MM-DD
            amount             INTEGER NOT NULL,               -- cents; negative = expense
            description        TEXT    NOT NULL DEFAULT '',
            category_id        INTEGER REFERENCES categories(id) ON DELETE SET NULL,
            -- The counterparty/"concept" that drives auto-classification.
            counterparty       TEXT    NOT NULL DEFAULT '',
            -- Manual review flag.
            is_verified        INTEGER NOT NULL DEFAULT 0,
            -- Category was applied by a learned rule rather than set by hand.
            is_auto_classified INTEGER NOT NULL DEFAULT 0,
            created_at         TEXT    NOT NULL DEFAULT (datetime('now'))
        );

        -- Remembers, per concept, which category to apply. The concept is unique
        -- case-insensitively and a rule disappears when its category is deleted.
        CREATE TABLE IF NOT EXISTS classification_rules (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            concept     TEXT    NOT NULL UNIQUE COLLATE NOCASE,
            category_id INTEGER NOT NULL REFERENCES categories(id) ON DELETE CASCADE,
            created_at  TEXT    NOT NULL DEFAULT (datetime('now'))
        );

        CREATE INDEX IF NOT EXISTS idx_tx_account      ON transactions(account_id);
        CREATE INDEX IF NOT EXISTS idx_tx_date         ON transactions(date);
        CREATE INDEX IF NOT EXISTS idx_tx_counterparty ON transactions(counterparty);

        -- Uniqueness scoped to the parent: top-level accounts are keyed under 0, so
        -- their names stay unique among each other while subaccounts only collide
        -- with their own siblings.
        CREATE UNIQUE INDEX IF NOT EXISTS ux_accounts_name_parent
            ON accounts(name, COALESCE(parent_id, 0));
        CREATE INDEX IF NOT EXISTS idx_accounts_parent ON accounts(parent_id);

        -- At most one category may carry the transfer role.
        CREATE UNIQUE INDEX IF NOT EXISTS ux_categories_transfer
            ON categories(is_transfer) WHERE is_transfer = 1;

        -- Backstop the app-level guard: even a stray DELETE can't remove the
        -- transfer category and silently change the dashboard totals.
        CREATE TRIGGER IF NOT EXISTS trg_protect_transfer_category
            BEFORE DELETE ON categories
            WHEN OLD.is_transfer = 1
            BEGIN
                SELECT RAISE(ABORT, 'The Transfer category is built in and cannot be deleted.');
            END;
        "#,
    )?;

    // Seed the default categories on first run, then flag the transfer role.
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM categories", [], |r| r.get(0))?;
    if count == 0 {
        for name in DEFAULT_CATEGORIES {
            conn.execute("INSERT INTO categories (name) VALUES (?1)", [name])?;
        }
        conn.execute(
            "UPDATE categories SET is_transfer = 1 WHERE name = 'Transfer'",
            [],
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_column(conn: &Connection, table: &str, column: &str) -> bool {
        let n: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = ?1"),
                [column],
                |r| r.get(0),
            )
            .unwrap();
        n > 0
    }

    #[test]
    fn fresh_database_seeds_the_default_categories() {
        let conn = open_in_memory().unwrap();
        assert!(has_column(&conn, "accounts", "parent_id"));
        // Default categories are seeded exactly once.
        let cats: i64 = conn
            .query_row("SELECT COUNT(*) FROM categories", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cats, DEFAULT_CATEGORIES.len() as i64);
        // A sampling of defaults across the set is present.
        let named: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM categories \
                 WHERE name IN ('Fees', 'Salary', 'Dividends', 'Travel', 'Home & DIY', 'Transfer')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(named, 6);
        // The transfer role is carried by exactly the built-in "Transfer" row.
        let flagged: (i64, String) = conn
            .query_row(
                "SELECT COUNT(*), COALESCE(MAX(name), '') FROM categories WHERE is_transfer = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(flagged, (1, "Transfer".to_string()));
    }

    #[test]
    fn transfer_role_is_a_singleton_and_survives_a_rename() {
        let conn = open_in_memory().unwrap();

        // Renaming the category keeps the role, so transfers stay identifiable.
        conn.execute(
            "UPDATE categories SET name = 'Umbuchung' WHERE is_transfer = 1",
            [],
        )
        .unwrap();
        let name: String = conn
            .query_row(
                "SELECT name FROM categories WHERE is_transfer = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(name, "Umbuchung");

        // A second transfer category is rejected by the partial unique index.
        let dup = conn.execute(
            "INSERT INTO categories (name, is_transfer) VALUES ('Another', 1)",
            [],
        );
        assert!(dup.is_err(), "only one category may be the transfer role");

        // Deleting the transfer category is blocked at the data layer.
        let del = conn.execute("DELETE FROM categories WHERE is_transfer = 1", []);
        assert!(del.is_err(), "the transfer category must not be deletable");
        // A plain category is still deletable.
        conn.execute("INSERT INTO categories (name) VALUES ('Scratch')", [])
            .unwrap();
        conn.execute("DELETE FROM categories WHERE name = 'Scratch'", [])
            .unwrap();
    }

    #[test]
    fn per_parent_account_uniqueness_holds() {
        let conn = open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO accounts (name) VALUES ('Checking'), ('Savings')",
            [],
        )
        .unwrap();

        // Two subaccounts under different parents may share a name...
        conn.execute(
            "INSERT INTO accounts (name, parent_id) VALUES ('Sub', 1), ('Sub', 2)",
            [],
        )
        .unwrap();
        // ...but siblings under the same parent may not.
        let dup = conn.execute(
            "INSERT INTO accounts (name, parent_id) VALUES ('Sub', 1)",
            [],
        );
        assert!(dup.is_err(), "per-parent uniqueness should hold");
    }

    #[test]
    fn init_schema_is_idempotent() {
        let conn = open_in_memory().unwrap();
        // Running again is a no-op and must not error or reseed categories.
        init_schema(&conn).unwrap();
        init_schema(&conn).unwrap();
        let cats: i64 = conn
            .query_row("SELECT COUNT(*) FROM categories", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cats, DEFAULT_CATEGORIES.len() as i64);
    }
}
