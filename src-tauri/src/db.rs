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
    "Other",
    "Transfer",
];

/// Create the schema and seed default categories. Amounts are stored as integer
/// cents to avoid floating-point rounding errors.
///
/// Idempotent (`CREATE ... IF NOT EXISTS`, seed-if-empty, add-column-if-missing),
/// so it is safe to run on every open. The `CREATE TABLE` statements are the full
/// current shape; columns added after a table shipped are backfilled in place via
/// [`add_column_if_missing`] so existing databases pick them up.
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
            -- Starting balance in cents, for accounts whose imported history is
            -- only partial: it anchors the displayed balance to a known figure
            -- but is never counted as income/expense.
            opening_balance INTEGER NOT NULL DEFAULT 0,
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
            -- A stable per-transaction reference from the bank export (e.g.
            -- comdirect's Referenz), when one exists. Used as the duplicate key so
            -- distinct same-day/same-amount charges aren't merged. Empty otherwise.
            import_ref         TEXT    NOT NULL DEFAULT '',
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

    // Evolve databases created before `opening_balance` existed: `CREATE TABLE
    // IF NOT EXISTS` above leaves an existing table untouched, so add the column
    // here if it is missing. Idempotent alongside the rest of `init_schema`.
    add_column_if_missing(
        conn,
        "accounts",
        "opening_balance",
        "INTEGER NOT NULL DEFAULT 0",
    )?;

    // Databases created before per-transaction references existed pick up the
    // column here; existing rows keep the empty default and dedupe on their
    // date/amount/description as before.
    add_column_if_missing(
        conn,
        "transactions",
        "import_ref",
        "TEXT NOT NULL DEFAULT ''",
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

    // One-time data migrations, gated by the schema version so they run once.
    // NOTE: existing databases from an earlier migration scheme already carry a
    // non-zero `user_version` (observed up to 13), so this target must sit above
    // those — a `< 1` gate would silently skip the migration on them.
    let version: i64 = conn.query_row("SELECT * FROM pragma_user_version", [], |r| r.get(0))?;
    if version < CANONICAL_MERCHANT_MIGRATION {
        // Merchant canonicalization ([`importers::canonical_merchant`]) shipped after
        // rows were already imported, so their counterparties still carry per-purchase
        // tokens (e.g. every Amazon charge under a different string). Rewrite existing
        // counterparties and rule concepts to the canonical form so a single learned
        // rule covers them, then classify history that was previously scattered.
        backfill_canonical_counterparties(conn)?;
        conn.pragma_update(None, "user_version", CANONICAL_MERCHANT_MIGRATION)?;
    }
    if version < OTHER_CATEGORY_MIGRATION {
        // "Other" joined the default set after databases had already been seeded
        // (the seed above only runs on an empty categories table). Back-fill it so
        // existing databases pick it up too. `INSERT OR IGNORE` makes this a no-op
        // when the row already exists — e.g. a fresh DB, or a user who added it by
        // hand — and the name's case-insensitive UNIQUE constraint enforces that.
        conn.execute("INSERT OR IGNORE INTO categories (name) VALUES ('Other')", [])?;
        conn.pragma_update(None, "user_version", OTHER_CATEGORY_MIGRATION)?;
    }

    Ok(())
}

/// `user_version` marking that the "Other" default category has been back-filled
/// into pre-existing databases. Sits one above [`CANONICAL_MERCHANT_MIGRATION`].
const OTHER_CATEGORY_MIGRATION: i64 = 15;

/// `user_version` marking that the merchant-canonicalization backfill has run.
/// Chosen above the legacy values older databases already carry (see the gate in
/// [`init_schema`]).
const CANONICAL_MERCHANT_MIGRATION: i64 = 14;

/// Rewrite already-stored counterparties and classification-rule concepts to the
/// canonical merchant form, then apply each rule to still-uncategorized history.
///
/// Idempotent in effect (canonicalization is a pure function), but run only once via
/// the `user_version` gate. Rules whose canonical concept collides with an existing
/// rule are dropped in favor of the one already there.
fn backfill_canonical_counterparties(conn: &Connection) -> rusqlite::Result<()> {
    use crate::importers::canonical_merchant;

    // Transactions: re-canonicalize each counterparty in place.
    let tx_rows: Vec<(i64, String)> = {
        let mut stmt = conn.prepare("SELECT id, counterparty FROM transactions")?;
        let mapped = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        mapped.collect::<rusqlite::Result<_>>()?
    };
    for (id, cp) in tx_rows {
        let canon = canonical_merchant(&cp);
        if canon != cp {
            conn.execute(
                "UPDATE transactions SET counterparty = ?1 WHERE id = ?2",
                rusqlite::params![canon, id],
            )?;
        }
    }

    // Rules: same rewrite, but `concept` is unique — on a collision keep the rule
    // already holding the canonical concept and drop this now-redundant one.
    let rule_rows: Vec<(i64, String)> = {
        let mut stmt = conn.prepare("SELECT id, concept FROM classification_rules")?;
        let mapped = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        mapped.collect::<rusqlite::Result<_>>()?
    };
    for (id, concept) in rule_rows {
        let canon = canonical_merchant(&concept);
        if canon == concept {
            continue;
        }
        let renamed = conn.execute(
            "UPDATE classification_rules SET concept = ?1 WHERE id = ?2",
            rusqlite::params![canon, id],
        );
        if renamed.is_err() {
            conn.execute(
                "DELETE FROM classification_rules WHERE id = ?1",
                rusqlite::params![id],
            )?;
        }
    }

    // Apply every (now canonical) rule to unverified rows that are still
    // uncategorized. Fills gaps only — a user's manual category is never overwritten.
    conn.execute(
        "UPDATE transactions
         SET category_id = (
                 SELECT category_id FROM classification_rules
                 WHERE concept = transactions.counterparty COLLATE NOCASE
             ),
             is_auto_classified = 1
         WHERE is_verified = 0
           AND category_id IS NULL
           AND counterparty IN (SELECT concept FROM classification_rules)",
        [],
    )?;

    Ok(())
}

/// Add `column` to `table` if it is not already present, so a database created
/// before the column existed picks it up on open. No-op once the column exists.
fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    decl: &str,
) -> rusqlite::Result<()> {
    let exists: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = ?1"),
        [column],
        |r| r.get(0),
    )?;
    if exists == 0 {
        // `table`/`column`/`decl` are internal constants, never user input.
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"),
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
    fn backfill_canonicalizes_counterparties_without_touching_descriptions() {
        let conn = open_in_memory().unwrap();
        conn.execute("INSERT INTO accounts (name) VALUES ('Visa')", [])
            .unwrap();
        let acc = conn.last_insert_rowid();
        let shopping: i64 = conn
            .query_row(
                "SELECT id FROM categories WHERE name = 'Shopping'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        // Simulate pre-canonicalization data: two Amazon charges under different raw
        // merchant strings — one categorized by hand (which taught a rule keyed on
        // its raw counterparty), one still uncategorized.
        conn.execute(
            "INSERT INTO transactions
                (account_id, date, amount, description, counterparty, category_id, is_auto_classified, is_verified)
             VALUES (?1, '2026-01-08', -2099, 'AMZN Mktp DE ZC38L06T4', 'AMZN Mktp DE ZC38L06T4', ?2, 0, 0)",
            rusqlite::params![acc, shopping],
        ).unwrap();
        conn.execute(
            "INSERT INTO transactions
                (account_id, date, amount, description, counterparty, category_id, is_auto_classified, is_verified)
             VALUES (?1, '2026-01-09', -1299, 'Amazon.de O59H84AO5', 'Amazon.de O59H84AO5', NULL, 0, 0)",
            rusqlite::params![acc],
        ).unwrap();
        conn.execute(
            "INSERT INTO classification_rules (concept, category_id) VALUES ('AMZN Mktp DE ZC38L06T4', ?1)",
            rusqlite::params![shopping],
        ).unwrap();

        backfill_canonical_counterparties(&conn).unwrap();

        // Counterparties fold to the shared concept; descriptions are left untouched.
        let rows: Vec<(String, String)> = {
            let mut stmt = conn
                .prepare("SELECT description, counterparty FROM transactions ORDER BY id")
                .unwrap();
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        assert_eq!(
            rows[0],
            ("AMZN Mktp DE ZC38L06T4".to_string(), "Amazon".to_string())
        );
        assert_eq!(
            rows[1],
            ("Amazon.de O59H84AO5".to_string(), "Amazon".to_string())
        );

        // The rule folded to "Amazon", and the previously-uncategorized sibling is
        // now classified from it.
        let concept: String = conn
            .query_row("SELECT concept FROM classification_rules", [], |r| r.get(0))
            .unwrap();
        assert_eq!(concept, "Amazon");
        let cat2: Option<i64> = conn
            .query_row(
                "SELECT category_id FROM transactions WHERE amount = -1299",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cat2, Some(shopping));
    }

    #[test]
    fn fresh_database_seeds_the_default_categories() {
        let conn = open_in_memory().unwrap();
        assert!(has_column(&conn, "accounts", "parent_id"));
        assert!(has_column(&conn, "accounts", "opening_balance"));
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
    fn other_category_is_backfilled_into_an_existing_database() {
        // An existing database seeded before "Other" shipped: categories are
        // present (so the seed-if-empty step won't run) but "Other" is absent, and
        // the user_version predates the back-fill migration.
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        conn.execute_batch(
            "CREATE TABLE categories (
                 id          INTEGER PRIMARY KEY AUTOINCREMENT,
                 name        TEXT    NOT NULL UNIQUE COLLATE NOCASE,
                 is_transfer INTEGER NOT NULL DEFAULT 0
             );
             INSERT INTO categories (name) VALUES ('Groceries'), ('Transfer');
             PRAGMA user_version = 14;",
        )
        .unwrap();

        init_schema(&conn).unwrap();

        // The migration adds "Other" exactly once and bumps the version.
        let others: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM categories WHERE name = 'Other'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(others, 1);
        let version: i64 = conn
            .query_row("SELECT * FROM pragma_user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, OTHER_CATEGORY_MIGRATION);

        // Re-running is a no-op: no duplicate "Other".
        init_schema(&conn).unwrap();
        let others: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM categories WHERE name = 'Other'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(others, 1);
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
    fn opening_balance_column_is_added_to_a_legacy_database() {
        // Simulate a database created before `opening_balance` existed: an
        // accounts table without the column.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE accounts (
                 id         INTEGER PRIMARY KEY AUTOINCREMENT,
                 name       TEXT    NOT NULL,
                 parent_id  INTEGER,
                 created_at TEXT    NOT NULL DEFAULT (datetime('now'))
             );
             INSERT INTO accounts (name) VALUES ('Legacy');",
        )
        .unwrap();
        assert!(!has_column(&conn, "accounts", "opening_balance"));

        // Running the schema init backfills the column, defaulting existing rows
        // to a zero starting balance.
        init_schema(&conn).unwrap();
        assert!(has_column(&conn, "accounts", "opening_balance"));
        let opening: i64 = conn
            .query_row(
                "SELECT opening_balance FROM accounts WHERE name = 'Legacy'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(opening, 0);
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
