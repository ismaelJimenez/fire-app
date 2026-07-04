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
const MIGRATIONS: &[fn(&Connection) -> rusqlite::Result<()>] = &[
    migrate_v1_baseline,
    migrate_v2_classification,
    migrate_v3_fees_category,
    migrate_v4_salary_category,
    migrate_v5_dividends_category,
    migrate_v6_transfer_category,
    migrate_v7_transfer_category_flag,
    migrate_v8_subscriptions_category,
];

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
            "Fees",
            "Salary",
            "Dividends",
            "Subscriptions",
            "Income",
            "Savings",
            "Transfer",
        ] {
            conn.execute("INSERT INTO categories (name) VALUES (?1)", [name])?;
        }
    }

    Ok(())
}

/// v2: concept-based auto-classification and manual verification.
///
/// - `transactions` gains a `counterparty` (the "concept" that drives
///   classification), a `is_verified` review flag, and `is_auto_classified` so the
///   UI can tell learned categories from ones the user set by hand.
/// - `classification_rules` remembers, per concept, which category to apply. The
///   concept is unique (case-insensitively) and a rule disappears when its category
///   is deleted.
///
/// Idempotent like the baseline: the column adds are guarded so re-running over an
/// already-migrated database is a no-op.
fn migrate_v2_classification(conn: &Connection) -> rusqlite::Result<()> {
    let has_column = |name: &str| -> rusqlite::Result<bool> {
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('transactions') WHERE name = ?1",
            [name],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    };

    if !has_column("counterparty")? {
        conn.execute(
            "ALTER TABLE transactions ADD COLUMN counterparty TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    if !has_column("is_verified")? {
        conn.execute(
            "ALTER TABLE transactions ADD COLUMN is_verified INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    if !has_column("is_auto_classified")? {
        conn.execute(
            "ALTER TABLE transactions ADD COLUMN is_auto_classified INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS classification_rules (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            -- The counterparty/concept this rule matches, unique case-insensitively.
            concept     TEXT    NOT NULL UNIQUE COLLATE NOCASE,
            category_id INTEGER NOT NULL REFERENCES categories(id) ON DELETE CASCADE,
            created_at  TEXT    NOT NULL DEFAULT (datetime('now'))
        );

        CREATE INDEX IF NOT EXISTS idx_tx_counterparty ON transactions(counterparty);
        "#,
    )?;

    Ok(())
}

/// v3: add the "Fees" default category to databases seeded before it existed.
///
/// `INSERT OR IGNORE` leans on the case-insensitive UNIQUE on `categories.name`,
/// so it's a no-op when the user (or the first-run seed) already has a "Fees"
/// category. Because migrations run once per database, a user who later deletes
/// "Fees" won't see it reappear.
fn migrate_v3_fees_category(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO categories (name) VALUES ('Fees')",
        [],
    )?;
    Ok(())
}

/// v4: add the "Salary" default category to databases seeded before it existed.
///
/// Same idempotent `INSERT OR IGNORE` approach as [`migrate_v3_fees_category`]:
/// a no-op when a "Salary" category already exists, and it won't reappear if the
/// user deletes it later.
fn migrate_v4_salary_category(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO categories (name) VALUES ('Salary')",
        [],
    )?;
    Ok(())
}

/// v5: add the "Dividends" default category to databases seeded before it existed.
///
/// Same idempotent `INSERT OR IGNORE` approach as [`migrate_v3_fees_category`]:
/// a no-op when a "Dividends" category already exists, and it won't reappear if
/// the user deletes it later.
fn migrate_v5_dividends_category(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO categories (name) VALUES ('Dividends')",
        [],
    )?;
    Ok(())
}

/// v8: add the "Subscriptions" default category (streaming, SaaS, AI tools such
/// as Netflix, ChatGPT, Claude…) to databases seeded before it existed.
///
/// Same idempotent `INSERT OR IGNORE` approach as [`migrate_v3_fees_category`]:
/// a no-op when a "Subscriptions" category already exists, and it won't reappear
/// if the user deletes it later.
fn migrate_v8_subscriptions_category(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO categories (name) VALUES ('Subscriptions')",
        [],
    )?;
    Ok(())
}

/// v6: replace the standalone `is_internal_transfer` flag with a first-class
/// "Transfer" category. Money moved between the user's own accounts is now
/// modelled as a transaction categorized "Transfer", which the dashboard summary
/// leaves out of income/expense totals.
///
/// Seeds the category and moves any legacy transactions that were flagged as
/// internal transfers (but never categorized) into it, so nothing that used to be
/// excluded from totals silently starts counting. The `is_internal_transfer`
/// column is left in place — harmless and avoids a table rebuild — but is no
/// longer read. Idempotent: re-running only ever re-touches already-migrated rows.
fn migrate_v6_transfer_category(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO categories (name) VALUES ('Transfer')",
        [],
    )?;
    conn.execute(
        "UPDATE transactions
            SET category_id = (SELECT id FROM categories WHERE name = 'Transfer')
          WHERE is_internal_transfer = 1 AND category_id IS NULL",
        [],
    )?;
    Ok(())
}

/// v7: make "the transfer category" a durable property instead of a magic name.
///
/// Adds a `categories.is_transfer` role flag and points it at the built-in
/// Transfer category. Summaries and the UI identify transfers by this flag, so the
/// category can be renamed freely without breaking totals. A partial unique index
/// keeps it a singleton, and `delete_category` refuses to remove the flagged row,
/// so the totals can never be silently changed by deleting it.
///
/// Idempotent: the column add is guarded, the category insert is `OR IGNORE`, and
/// the index/flag updates converge to the same state on every run.
fn migrate_v7_transfer_category_flag(conn: &Connection) -> rusqlite::Result<()> {
    let has_flag: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('categories') WHERE name = 'is_transfer'",
        [],
        |r| r.get(0),
    )?;
    if has_flag == 0 {
        conn.execute(
            "ALTER TABLE categories ADD COLUMN is_transfer INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }

    // Guarantee the built-in category exists even if a user deleted it under an
    // earlier version, then flag exactly that row.
    conn.execute(
        "INSERT OR IGNORE INTO categories (name) VALUES ('Transfer')",
        [],
    )?;
    conn.execute(
        "UPDATE categories SET is_transfer = 1 WHERE name = 'Transfer'",
        [],
    )?;

    // At most one category may carry the flag.
    conn.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS ux_categories_transfer
             ON categories(is_transfer) WHERE is_transfer = 1",
        [],
    )?;

    // Backstop the app-level guard: even a stray DELETE can't remove the transfer
    // category and silently change the dashboard totals.
    conn.execute(
        "CREATE TRIGGER IF NOT EXISTS trg_protect_transfer_category
             BEFORE DELETE ON categories
             WHEN OLD.is_transfer = 1
             BEGIN
                 SELECT RAISE(ABORT, 'The Transfer category is built in and cannot be deleted.');
             END",
        [],
    )?;
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
        assert_eq!(cats, 15);
        // The newer defaults are all present on a fresh database.
        let named: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM categories \
                 WHERE name IN ('Fees', 'Salary', 'Dividends', 'Transfer')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(named, 4);
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
        assert_eq!(cats, 15);
    }

    #[test]
    fn v6_moves_legacy_flagged_transfers_into_the_transfer_category() {
        // A database migrated only through v5, with a transaction flagged as an
        // internal transfer the old way and left uncategorized.
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        migrate_v1_baseline(&conn).unwrap();
        migrate_v2_classification(&conn).unwrap();
        conn.execute("INSERT INTO accounts (name) VALUES ('Checking')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO transactions (account_id, date, amount, is_internal_transfer)
             VALUES (1, '2026-01-01', -50000, 1)",
            [],
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 5).unwrap();

        migrate(&conn).unwrap();

        // The flagged row now carries the Transfer category.
        let cat: Option<String> = conn
            .query_row(
                "SELECT c.name FROM transactions t
                 LEFT JOIN categories c ON c.id = t.category_id
                 WHERE t.id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cat.as_deref(), Some("Transfer"));
    }

    #[test]
    fn v3_backfills_fees_for_legacy_databases_without_duplicating() {
        // A database migrated up to v2 (before "Fees" was a default): pretend the
        // user already had their own categories and none of them is "Fees".
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        migrate_v1_baseline(&conn).unwrap();
        conn.execute("DELETE FROM categories", []).unwrap();
        conn.execute(
            "INSERT INTO categories (name) VALUES ('Groceries'), ('Rent')",
            [],
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();

        migrate(&conn).unwrap();

        let fees: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM categories WHERE name = 'Fees'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fees, 1, "v3 should add exactly one Fees category");

        // Running again must not create a second "Fees".
        migrate_v3_fees_category(&conn).unwrap();
        let fees_again: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM categories WHERE name = 'Fees'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fees_again, 1, "backfill must be idempotent");
    }
}
