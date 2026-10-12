//! Storing what the doors bring in: model calls as messages, token entries
//! summed from them, and their splits with every tag copied (ADR 0006, ADR
//! 0008, ADR 0017).
//!
//! Every function here takes a connection, so a caller can store token
//! entries inside its own transaction, such as a token sheet submit.

mod entries;
mod place;
mod rebuild;
mod telemetry;

use std::sync::Arc;

use serde::Serialize;
use tokencontroller_core::{CostKind, Currency, Money, TokenCounts};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::ApiError;

pub use entries::{store_checked, store_token_entries};
pub use telemetry::{TelemetryOutcome, record_telemetry};

/// The door a token entry or message came through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Door {
    Telemetry,
    /// The token entries endpoint, with a service token: automatic.
    Endpoint,
    /// A token sheet submit from the CLI, the mod or the web app.
    TokenSheet,
    /// Manual entry in the web app: lands on the person's token sheet.
    Manual,
    /// CSV import in the web app: lands on the person's token sheet.
    Csv,
}

impl Door {
    /// The name stored in `token_entries.door`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Telemetry => "telemetry",
            Self::Endpoint => "endpoint",
            Self::TokenSheet => "token_sheet",
            Self::Manual => "manual",
            Self::Csv => "csv",
        }
    }

    /// The status a token entry gets when it arrives through this door.
    /// Automatic token entries start at submitted; token sheet submits are
    /// submitted; manual entries and imports wait on the token sheet.
    #[must_use]
    pub const fn status(self) -> &'static str {
        match self {
            Self::Telemetry | Self::Endpoint | Self::TokenSheet => "submitted",
            Self::Manual | Self::Csv => "draft",
        }
    }

    /// Whether token entries through this door sit on a token sheet.
    #[must_use]
    pub const fn on_token_sheet(self) -> bool {
        matches!(self, Self::TokenSheet | Self::Manual | Self::Csv)
    }
}

/// Prices `tokens` of a model: its list value in US dollars, or `None` for
/// an unknown model.
pub type PriceFn = dyn Fn(&str, TokenCounts) -> Option<Money> + Send + Sync;

/// Prices model calls with one price table version. Token entries with no
/// reported cost get their list value from it; a model it does not know
/// stays unknown, never zero.
#[derive(Clone)]
pub struct Pricing {
    /// The price table version, or `None` when no table has been loaded.
    pub version_id: Option<i64>,
    price: Arc<PriceFn>,
}

impl std::fmt::Debug for Pricing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pricing")
            .field("version_id", &self.version_id)
            .finish_non_exhaustive()
    }
}

impl Pricing {
    /// Prices nothing: every model without a reported cost is unknown.
    #[must_use]
    pub fn none() -> Self {
        Self {
            version_id: None,
            price: Arc::new(|_, _| None),
        }
    }

    /// Prices with `price`, which returns the list value of `tokens` of a
    /// model in US dollars, or `None` for an unknown model.
    #[must_use]
    pub fn new<F>(version_id: Option<i64>, price: F) -> Self
    where
        F: Fn(&str, TokenCounts) -> Option<Money> + Send + Sync + 'static,
    {
        Self {
            version_id,
            price: Arc::new(price),
        }
    }

    /// The list value of one model call: the reported cost where there is
    /// one, otherwise the tokens priced.
    #[must_use]
    pub fn list_value(
        &self,
        model: &str,
        tokens: TokenCounts,
        reported: Option<Money>,
    ) -> Option<Money> {
        reported.or_else(|| (self.price)(model, tokens))
    }
}

/// What storing one token entry gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEntry {
    /// The sender's own ID, when it sent one.
    pub local_id: Option<String>,
    pub id: Uuid,
    pub status: String,
    /// The list value, or `None` for an unknown model.
    pub list_value: Option<Money>,
    pub currency: Currency,
    /// The cost kind of its largest split.
    pub cost_kind: CostKind,
    /// Tag names that were sent but are not on the organization's list, so
    /// they were left out.
    pub dropped_tags: Vec<String>,
}

/// A stored token entry as the API answers with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct StoredEntryBody {
    pub local_id: Option<String>,
    pub id: Uuid,
    /// `draft` or `submitted`.
    pub status: String,
    /// A decimal string such as `"0.03"`, or `null` for an unknown model.
    pub list_value: Option<String>,
    pub currency: String,
    /// `direct`, `project_overhead` or `organization_overhead`.
    pub cost_kind: &'static str,
    /// Tag names that were sent but are not on the organization's list, so
    /// they were left out.
    pub dropped_tags: Vec<String>,
}

impl From<&StoredEntry> for StoredEntryBody {
    fn from(entry: &StoredEntry) -> Self {
        Self {
            local_id: entry.local_id.clone(),
            id: entry.id,
            status: entry.status.clone(),
            list_value: entry.list_value.map(money_text),
            currency: entry.currency.code(),
            cost_kind: cost_kind_name(entry.cost_kind),
            dropped_tags: entry.dropped_tags.clone(),
        }
    }
}

/// An amount as the API writes it: a decimal string in the currency's minor
/// unit, such as `"41.87"`.
#[must_use]
pub fn money_text(money: Money) -> String {
    money
        .currency()
        .format_minor_units(money.round_to_minor_units())
}

/// The stable name of a cost kind, as the API and exports write it.
#[must_use]
pub const fn cost_kind_name(kind: CostKind) -> &'static str {
    match kind {
        CostKind::Direct => "direct",
        CostKind::ProjectOverhead => "project_overhead",
        CostKind::OrganizationOverhead => "organization_overhead",
    }
}

/// Why storing failed.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// A token entry breaks the schema; `index` is its place in the batch.
    #[error("token entry {index}: {message}")]
    Invalid { index: usize, message: String },
    /// The token entry's period is closed.
    #[error("token entry {index} is in a closed period: post an adjustment instead")]
    Locked { index: usize },
    #[error("an amount is too large to add up")]
    Overflow,
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Invalid { .. } => Self::BadRequest(error.to_string()),
            StoreError::Locked { .. } => Self::Conflict(error.to_string()),
            StoreError::Overflow => Self::Internal(anyhow::Error::new(error)),
            StoreError::Database(error) => Self::Database(error),
        }
    }
}

impl From<tokencontroller_core::MoneyError> for StoreError {
    fn from(_: tokencontroller_core::MoneyError) -> Self {
        Self::Overflow
    }
}

/// Token entries that arrived since `since`, for the status endpoint's
/// "token entries arriving" signal.
///
/// # Errors
///
/// Fails if the query fails.
pub async fn arrivals_since(
    db: &sqlx::PgPool,
    since: time::OffsetDateTime,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "count!" FROM token_entries WHERE updated_at >= $1"#,
        since
    )
    .fetch_one(db)
    .await
}

fn to_i64(count: u64) -> Result<i64, StoreError> {
    i64::try_from(count).map_err(|_| StoreError::Overflow)
}

fn to_u64(count: i64) -> u64 {
    u64::try_from(count).unwrap_or_default()
}

/// Prices the calls whose model the price table did not know, in token
/// entries of open periods, and rebuilds those token entries. Runs after a
/// price table refresh made a new version. Returns how many token entries
/// changed.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn reprice_unknown(db: &sqlx::PgPool, pricing: &Pricing) -> Result<u64, StoreError> {
    let calls = sqlx::query!(
        r#"SELECT m.organization_id, m.session_id, m.message_id, m.model, m.token_entry_id AS "token_entry_id!",
                  m.input_tokens, m.output_tokens, m.cache_write_tokens, m.cache_read_tokens
           FROM messages m JOIN token_entries e ON e.id = m.token_entry_id
           WHERE m.list_value_nanos IS NULL AND m.reported_cost_nanos IS NULL
             AND e.status <> 'locked'
           ORDER BY m.organization_id, m.token_entry_id
           LIMIT 100000"#
    )
    .fetch_all(db)
    .await?;
    let mut changed: std::collections::BTreeMap<Uuid, std::collections::BTreeSet<Uuid>> =
        std::collections::BTreeMap::new();
    let mut tx = db.begin().await?;
    for call in calls {
        let tokens = TokenCounts {
            input: to_u64(call.input_tokens),
            output: to_u64(call.output_tokens),
            cache_write: to_u64(call.cache_write_tokens),
            cache_read: to_u64(call.cache_read_tokens),
        };
        let Some(value) = pricing.list_value(&call.model, tokens, None) else {
            continue;
        };
        sqlx::query!(
            "UPDATE messages SET list_value_nanos = $4
             WHERE organization_id = $1 AND session_id = $2 AND message_id = $3",
            call.organization_id,
            call.session_id,
            call.message_id,
            value.nanos()
        )
        .execute(&mut *tx)
        .await?;
        changed
            .entry(call.organization_id)
            .or_default()
            .insert(call.token_entry_id);
    }
    let mut count = 0_u64;
    for (organization_id, entries) in changed {
        let context = place::org_context(&mut tx, organization_id).await?;
        for entry_id in entries {
            sqlx::query!(
                "UPDATE token_entries SET price_table_version_id = $2 WHERE id = $1",
                entry_id,
                pricing.version_id
            )
            .execute(&mut *tx)
            .await?;
            rebuild::rebuild(&mut tx, organization_id, entry_id, &context).await?;
            count = count.saturating_add(1);
        }
    }
    tx.commit().await?;
    Ok(count)
}
