//! Price table versions in the database, and the daily refresh.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context as _;
use sqlx::PgPool;
use time::OffsetDateTime;

use super::{LITELLM_URL, MODELS_DEV_URL, ModelPrice, fetch, list_value, lookup, table_digest};
use crate::AppState;
use crate::ingest::store::Pricing;

/// The job that refreshes the price table.
pub const JOB: &str = "price-tables";

/// Prices refresh daily (docs: Reference > Price tables).
pub const JOB_EVERY: Duration = Duration::from_hours(24);

/// One stored version of the price table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceTableVersion {
    pub id: i64,
    pub created_at: OffsetDateTime,
    pub prices: Vec<ModelPrice>,
    /// Sources the refresh that made this version could not read.
    pub failed_sources: Vec<String>,
}

impl PriceTableVersion {
    /// Prices model calls with this version.
    #[must_use]
    pub fn pricing(self: &Arc<Self>) -> Pricing {
        let table = Arc::clone(self);
        Pricing::new(Some(self.id), move |model, tokens| {
            lookup(&table.prices, model).and_then(|price| list_value(tokens, price).ok().flatten())
        })
    }
}

/// The last version read, so a request does not parse the whole table
/// again while it has not changed.
static CURRENT: Mutex<Option<Arc<PriceTableVersion>>> = Mutex::new(None);

/// The price table in force: the newest version, or `None` before the
/// first refresh.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn current(db: &PgPool) -> Result<Option<Arc<PriceTableVersion>>, sqlx::Error> {
    let Some(id) = sqlx::query_scalar!("SELECT max(id) FROM price_table_versions")
        .fetch_one(db)
        .await?
    else {
        return Ok(None);
    };
    let cached = CURRENT.lock().ok().and_then(|current| current.clone());
    if let Some(cached) = cached.filter(|cached| cached.id == id) {
        return Ok(Some(cached));
    }
    let version = version(db, id).await?.map(Arc::new);
    if let (Some(version), Ok(mut current)) = (&version, CURRENT.lock()) {
        *current = Some(Arc::clone(version));
    }
    Ok(version)
}

/// Prices with the price table in force, or nothing before the first
/// refresh.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn current_pricing(db: &PgPool) -> Result<Pricing, sqlx::Error> {
    Ok(current(db)
        .await?
        .map_or_else(Pricing::none, |version| version.pricing()))
}

/// One version by its ID, such as the one a closed period keeps.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn version(db: &PgPool, id: i64) -> Result<Option<PriceTableVersion>, sqlx::Error> {
    let row = sqlx::query!(
        "SELECT id, created_at, prices, failed_sources FROM price_table_versions WHERE id = $1",
        id
    )
    .fetch_optional(db)
    .await?;
    Ok(row.map(|row| PriceTableVersion {
        id: row.id,
        created_at: row.created_at,
        prices: serde_json::from_value(row.prices).unwrap_or_default(),
        failed_sources: row.failed_sources,
    }))
}

/// What a refresh did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refreshed {
    /// The prices changed: a new version.
    NewVersion { id: i64, models: usize },
    /// The prices are the same as the version in force.
    Unchanged { id: i64 },
}

/// Stores `prices` as a new version unless they are the same as the
/// version in force.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn save(
    db: &PgPool,
    prices: &[ModelPrice],
    failed_sources: &[String],
) -> Result<Refreshed, sqlx::Error> {
    let digest = table_digest(prices);
    let latest =
        sqlx::query!("SELECT id, digest FROM price_table_versions ORDER BY id DESC LIMIT 1")
            .fetch_optional(db)
            .await?;
    if let Some(latest) = latest.filter(|latest| latest.digest == digest) {
        return Ok(Refreshed::Unchanged { id: latest.id });
    }
    let id = sqlx::query_scalar!(
        "INSERT INTO price_table_versions (digest, failed_sources, prices)
         VALUES ($1, $2, $3) RETURNING id",
        digest,
        failed_sources,
        serde_json::to_value(prices).unwrap_or_default()
    )
    .fetch_one(db)
    .await?;
    Ok(Refreshed::NewVersion {
        id,
        models: prices.len(),
    })
}

/// The daily refresh: fetches both sources and stores a new version when
/// the prices changed. Token entries of open periods whose model was
/// unknown get their list value from the new version.
///
/// # Errors
///
/// Fails when neither source can be read or a query fails.
pub async fn refresh(
    db: &PgPool,
    client: &reqwest::Client,
    litellm_url: &str,
    models_dev_url: &str,
) -> anyhow::Result<String> {
    let fetched = fetch(client, litellm_url, models_dev_url).await?;
    let failed: Vec<String> = fetched
        .failed_sources
        .iter()
        .map(ToString::to_string)
        .collect();
    let note = match save(db, &fetched.prices, &failed).await? {
        Refreshed::Unchanged { id } => format!("prices unchanged (version {id})"),
        Refreshed::NewVersion { id, models } => {
            let pricing = current_pricing(db).await?;
            let repriced = crate::ingest::store::reprice_unknown(db, &pricing).await?;
            format!(
                "new price table version {id} with {models} models; {repriced} token entries with an unknown model priced"
            )
        }
    };
    if failed.is_empty() {
        Ok(note)
    } else {
        Ok(format!("{note}; could not read: {}", failed.join("; ")))
    }
}

/// The `price-tables` job: [`refresh`] from `LiteLLM` and models.dev.
///
/// # Errors
///
/// As [`refresh`].
pub async fn run_job(state: AppState) -> anyhow::Result<String> {
    let client = crate::integrations::http::client().context("could not build the HTTP client")?;
    refresh(&state.db, &client, LITELLM_URL, MODELS_DEV_URL).await
}
