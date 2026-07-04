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
    pub category_id: Option<i64>,
    pub category_name: Option<String>,
    pub is_internal_transfer: bool,
    pub created_at: String,
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
