use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Account {
    pub id: i64,
    pub name: String,
    /// Parent account this was split from, or `None` for a top-level account.
    pub parent_id: Option<i64>,
    pub created_at: String,
    /// Starting balance in cents, for accounts whose imported history is partial.
    /// Included in `balance`; excluded from income/expense totals.
    pub opening_balance: i64,
    /// Opening balance plus the sum of all transaction amounts, in cents.
    pub balance: i64,
    /// Number of transactions in the account.
    pub tx_count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Category {
    pub id: i64,
    pub name: String,
    /// Built-in role: transactions here are transfers between the user's own
    /// accounts and are excluded from income/expense totals. At most one category
    /// carries this, and it cannot be deleted.
    pub is_transfer: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Transaction {
    pub id: i64,
    pub account_id: i64,
    pub account_name: String,
    pub date: String,
    /// Amount in cents; negative = expense, positive = income.
    pub amount: i64,
    pub description: String,
    /// Payee/counterparty ("concept") that drives auto-classification. May be empty
    /// for manually entered transactions.
    pub counterparty: String,
    pub category_id: Option<i64>,
    pub category_name: Option<String>,
    /// The user has reviewed this row; it is locked from rule-based re-categorization.
    pub is_verified: bool,
    /// The category was applied by a learned rule rather than set by hand.
    pub is_auto_classified: bool,
    pub created_at: String,
}

/// A learned "concept → category" mapping used to auto-classify transactions.
#[derive(Debug, Serialize, Deserialize)]
pub struct ClassificationRule {
    pub id: i64,
    pub concept: String,
    pub category_id: i64,
    pub category_name: String,
}

/// Result of a CSV import run.
///
/// `preview` is only populated on a dry run: it lists every parsed row with the
/// outcome it *would* have, so the user can review before committing. A real
/// import leaves it empty.
#[derive(Debug, Serialize, Deserialize)]
pub struct ImportResult {
    pub imported: usize,
    pub skipped_duplicates: usize,
    pub errors: Vec<String>,
    #[serde(default)]
    pub preview: Vec<ImportPreviewRow>,
}

/// One parsed row as it would land, computed by a dry run without writing anything.
#[derive(Debug, Serialize, Deserialize)]
pub struct ImportPreviewRow {
    pub date: String,
    /// Amount in cents; negative = expense.
    pub amount: i64,
    pub description: String,
    pub counterparty: String,
    /// The category this row would receive (explicit, or from a learned rule).
    pub category: Option<String>,
    /// True when the category came from a learned classification rule.
    pub auto_classified: bool,
    /// True when importing would create `category` as a brand-new category.
    pub new_category: bool,
    /// True when an identical transaction already exists (row would be skipped).
    pub duplicate: bool,
}

/// Aggregate figures for the dashboard.
#[derive(Debug, Serialize, Deserialize)]
pub struct Summary {
    pub total_balance: i64,
    pub income: i64,
    pub expenses: i64,
    pub account_count: i64,
    pub transaction_count: i64,
}
