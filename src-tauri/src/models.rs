use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Account {
    pub id: i64,
    pub name: String,
    /// Parent account this was split from, or `None` for a top-level account.
    pub parent_id: Option<i64>,
    pub created_at: String,
    /// Sum of all transaction amounts for the account, in cents.
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
#[derive(Debug, Serialize, Deserialize)]
pub struct ImportResult {
    pub imported: usize,
    pub skipped_duplicates: usize,
    pub errors: Vec<String>,
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
