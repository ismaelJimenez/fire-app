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

/// Open an in-memory database with the schema applied. Used by tests to get an
/// isolated, disposable connection without touching the filesystem.
#[cfg(test)]
pub fn open_in_memory() -> rusqlite::Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn)?;
    Ok(conn)
}

/// Ordered schema migrations. Each step is applied once, in order, to any
/// database whose `user_version` is below its (1-based) position; `user_version`
/// is then advanced so the step never runs again.
///
/// Only ever *append* to this list — never edit or reorder an existing step, or
/// databases in the field will diverge from fresh ones.
const MIGRATIONS: &[fn(&Connection) -> rusqlite::Result<()>] = &[migrate_v1_baseline];

/// Apply any migrations the database hasn't seen yet.
///
/// Databases created before versioning existed report `user_version = 0` and so
/// run the v1 baseline, which is deliberately idempotent (`CREATE ... IF NOT
/// EXISTS`, guarded `ALTER`s, seed-if-empty) and safe to run over existing data.
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let mut version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    while (version as usize) < MIGRATIONS.len() {
        MIGRATIONS[version as usize](conn)?;
        version += 1;
        conn.pragma_update(None, "user_version", version)?;
    }
    Ok(())
}

/// Baseline schema. Amounts are stored as integer cents to avoid floating-point
/// rounding errors. Idempotent so it can also bring pre-versioning databases up
/// to the current shape.
fn migrate_v1_baseline(conn: &Connection) -> rusqlite::Result<()> {
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

    // Add the subaccount column to databases created before it existed. Adding a
    // column is the only ALTER SQLite needs here, and it is safe to run every start.
    let has_parent: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('accounts') WHERE name = 'parent_id'",
        [],
        |r| r.get(0),
    )?;
    if has_parent == 0 {
        conn.execute(
            "ALTER TABLE accounts ADD COLUMN parent_id INTEGER REFERENCES accounts(id) ON DELETE CASCADE",
            [],
        )?;
    }

    // Older databases enforced a global UNIQUE on accounts.name. Names are now
    // unique only within a parent, which a column-level constraint cannot express,
    // so rebuild the table without it. This only loosens the rule, so any existing
    // rows carry over cleanly.
    let accounts_sql: String = conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'accounts'",
        [],
        |r| r.get(0),
    )?;
    if accounts_sql.to_uppercase().contains("UNIQUE") {
        // foreign_keys must be toggled outside a transaction; run the rebuild as
        // plain autocommitting statements.
        conn.execute_batch(
            r#"
            PRAGMA foreign_keys = OFF;
            CREATE TABLE accounts_new (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                name       TEXT    NOT NULL,
                parent_id  INTEGER REFERENCES accounts(id) ON DELETE CASCADE,
                created_at TEXT    NOT NULL DEFAULT (datetime('now'))
            );
            INSERT INTO accounts_new (id, name, parent_id, created_at)
                SELECT id, name, parent_id, created_at FROM accounts;
            DROP TABLE accounts;
            ALTER TABLE accounts_new RENAME TO accounts;
            PRAGMA foreign_keys = ON;
            "#,
        )?;
    }

    // Uniqueness scoped to the parent: top-level accounts are keyed under 0, so
    // their names stay unique among each other while subaccounts only collide with
    // their own siblings.
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS ux_accounts_name_parent
             ON accounts(name, COALESCE(parent_id, 0))",
        [],
    )?;
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_accounts_parent ON accounts(parent_id)",
        [],
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

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn has_column(conn: &Connection, table: &str, column: &str) -> bool {
        let n: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = ?1"),
                params![column],
                |r| r.get(0),
            )
            .unwrap();
        n > 0
    }

    fn user_version(conn: &Connection) -> i64 {
        conn.query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn fresh_database_lands_on_the_latest_version() {
        let conn = open_in_memory().unwrap();
        assert_eq!(user_version(&conn), MIGRATIONS.len() as i64);
        assert!(has_column(&conn, "accounts", "parent_id"));
        // Default categories are seeded exactly once.
        let cats: i64 = conn
            .query_row("SELECT COUNT(*) FROM categories", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cats, 10);
    }

    #[test]
    fn migrating_a_legacy_database_preserves_rows_and_drops_global_unique() {
        // Reproduce a pre-versioning schema: UNIQUE on name, no parent_id.
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE accounts (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                name       TEXT    NOT NULL UNIQUE,
                created_at TEXT    NOT NULL DEFAULT (datetime('now'))
            );
            INSERT INTO accounts (name) VALUES ('Checking'), ('Savings');
            "#,
        )
        .unwrap();
        assert_eq!(user_version(&conn), 0);

        migrate(&conn).unwrap();

        // Schema was upgraded in place.
        assert!(has_column(&conn, "accounts", "parent_id"));
        assert_eq!(user_version(&conn), MIGRATIONS.len() as i64);
        // Existing rows carried over untouched.
        let names: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT name FROM accounts ORDER BY name")
                .unwrap();
            let rows = stmt.query_map([], |r| r.get(0)).unwrap();
            rows.map(|r| r.unwrap()).collect()
        };
        assert_eq!(names, vec!["Checking", "Savings"]);

        // The global UNIQUE is gone: two subaccounts under different parents may
        // now share a name...
        conn.execute(
            "INSERT INTO accounts (name, parent_id) VALUES ('Sub', 1), ('Sub', 2)",
            [],
        )
        .unwrap();
        // ...but siblings under the same parent still may not.
        let dup = conn.execute(
            "INSERT INTO accounts (name, parent_id) VALUES ('Sub', 1)",
            [],
        );
        assert!(dup.is_err(), "per-parent uniqueness should still hold");

        // Foreign keys survive the table rebuild.
        let fk: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk, 1);
    }

    #[test]
    fn migrate_is_idempotent() {
        let conn = open_in_memory().unwrap();
        let before = user_version(&conn);
        // Running again is a no-op and must not error or reseed categories.
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();
        assert_eq!(user_version(&conn), before);
        let cats: i64 = conn
            .query_row("SELECT COUNT(*) FROM categories", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cats, 10);
    }
}
