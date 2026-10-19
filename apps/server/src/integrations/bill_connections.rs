//! Bill sources an organization connects, and the pulls that keep their bills
//! current (docs: Integrations > Anthropic Enterprise, Anthropic Console,
//! `OpenAI`).
//!
//! An org manager adds an admin key under _Bills_. The `bill-pulls` job
//! pulls the current and the previous month of each connection once a day,
//! or at once when someone asks, and hands each bill to the bills area. The
//! bills area writes a new bill event only when the figures changed, so
//! pulling the same bill again changes nothing.

use std::collections::BTreeSet;
use std::time::Duration;

use anyhow::Context as _;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::bills::{
    BillLine, BillMonth, PullError, PulledBill, anthropic_console, anthropic_enterprise, openai,
};
use super::{ConnectionStatus, secrets};
use crate::AppState;
use crate::error::{ApiError, ErrorBody};
use crate::events::{Log, access};
use crate::sign_in::{Caller, Member};

/// The job that pulls bills.
pub const JOB: &str = "bill-pulls";

/// How often the job looks for connections that are due. A pull someone
/// asks for starts at the runner's next tick instead.
pub const JOB_EVERY: Duration = Duration::from_mins(15);

/// How often each connection pulls, in hours.
const PULL_EVERY_HOURS: i32 = 24;

/// Connections one run pulls at most; the rest wait for the next run.
const PER_RUN: i64 = 20;

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_bill_connections, add_bill_connection))
        .routes(routes!(remove_bill_connection))
        .routes(routes!(pull_now))
}

/// The provider API a bill connection pulls from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum BillApi {
    /// The Anthropic Enterprise Analytics API.
    #[serde(rename = "anthropic_enterprise")]
    AnthropicEnterprise,
    /// The Anthropic Usage and Cost Admin API.
    #[serde(rename = "anthropic_console")]
    AnthropicConsole,
    /// The `OpenAI` Costs API.
    #[serde(rename = "openai")]
    OpenAi,
}

impl BillApi {
    /// The name stored in `bill_connections.provider`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AnthropicEnterprise => "anthropic_enterprise",
            Self::AnthropicConsole => "anthropic_console",
            Self::OpenAi => "openai",
        }
    }

    /// The API for a stored name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [
            Self::AnthropicEnterprise,
            Self::AnthropicConsole,
            Self::OpenAi,
        ]
        .into_iter()
        .find(|api| api.as_str() == name)
    }

    /// The name people know it by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::AnthropicEnterprise => "Anthropic Enterprise",
            Self::AnthropicConsole => "Anthropic Console",
            Self::OpenAi => "OpenAI",
        }
    }
}

/// Where the provider APIs are: the providers' own addresses in production,
/// a fake server in tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseUrls {
    pub anthropic: String,
    pub openai: String,
}

impl BaseUrls {
    /// The providers' own addresses.
    #[must_use]
    pub fn production() -> Self {
        Self {
            anthropic: anthropic_console::BASE_URL.to_owned(),
            openai: openai::BASE_URL.to_owned(),
        }
    }

    /// Every provider at one address, for tests.
    #[must_use]
    pub fn all_at(base_url: &str) -> Self {
        Self {
            anthropic: base_url.to_owned(),
            openai: base_url.to_owned(),
        }
    }
}

/// A connected bill source, as the web app shows it. The key is never sent
/// back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct BillConnectionView {
    pub id: Uuid,
    pub provider: BillApi,
    /// The key's last characters, to tell keys apart.
    pub key_hint: String,
    /// When the last pull succeeded.
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_pull_at: Option<OffsetDateTime>,
    pub status: ConnectionStatus,
    /// Why the last pull failed.
    pub message: Option<String>,
}

/// A bill source to connect.
#[derive(Deserialize, ToSchema)]
pub struct NewBillConnection {
    pub provider: BillApi,
    /// An admin key of the provider's organization: an Enterprise Analytics
    /// API key, a Console admin key or an `OpenAI` admin key.
    pub key: String,
}

async fn views(
    db: &PgPool,
    organization_id: Uuid,
    only: Option<Uuid>,
) -> Result<Vec<BillConnectionView>, sqlx::Error> {
    let rows = sqlx::query!(
        "SELECT id, provider, key_hint, last_pull_at, last_error FROM bill_connections
         WHERE organization_id = $1 AND ($2::uuid IS NULL OR id = $2)
         ORDER BY created_at, id",
        organization_id,
        only
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            Some(BillConnectionView {
                id: row.id,
                provider: BillApi::from_name(&row.provider)?,
                key_hint: row.key_hint,
                last_pull_at: row.last_pull_at,
                status: ConnectionStatus::of(row.last_error.as_ref()),
                message: row.last_error,
            })
        })
        .collect())
}

async fn view(
    db: &PgPool,
    organization_id: Uuid,
    id: Uuid,
) -> Result<BillConnectionView, ApiError> {
    views(db, organization_id, Some(id))
        .await?
        .into_iter()
        .next()
        .ok_or(ApiError::NotFound)
}

/// The bill sources the organization connected. Org managers only.
#[utoipa::path(
    get,
    path = "/bill-connections",
    tag = "integrations",
    responses((status = OK, body = Vec<BillConnectionView>)),
)]
async fn list_bill_connections(
    State(state): State<AppState>,
    member: Member,
) -> Result<Json<Vec<BillConnectionView>>, ApiError> {
    require_org_manager(&state, &member).await?;
    Ok(Json(views(&state.db, member.organization_id, None).await?))
}

/// Connect a bill source with an admin key. Org managers only. The first
/// pull starts right away and loads the current and the previous month.
#[utoipa::path(
    post,
    path = "/bill-connections",
    tag = "integrations",
    request_body = NewBillConnection,
    responses(
        (status = CREATED, body = BillConnectionView),
        (status = BAD_REQUEST, description = "The key is missing", body = ErrorBody),
    ),
)]
async fn add_bill_connection(
    State(state): State<AppState>,
    caller: Caller,
    Json(body): Json<NewBillConnection>,
) -> Result<(StatusCode, Json<BillConnectionView>), ApiError> {
    caller.require_session()?;
    require_org_manager(&state, &caller).await?;
    let key = body.key.trim().to_owned();
    if key.is_empty() {
        return Err(ApiError::BadRequest(format!(
            "Add an admin key from {}.",
            body.provider.name()
        )));
    }
    let key_hint = secrets::hint(&key);
    let sealed = secrets::seal(&state.config.secret_key, &key).context("could not seal the key")?;
    let id = sqlx::query_scalar!(
        "INSERT INTO bill_connections
             (organization_id, provider, sealed_key, key_hint, created_by, pull_requested_at)
         VALUES ($1, $2, $3, $4, $5, now()) RETURNING id",
        caller.organization_id,
        body.provider.as_str(),
        sealed,
        key_hint,
        caller.person_id
    )
    .fetch_one(&state.db)
    .await?;
    crate::jobs::request_now(&state.db, JOB).await?;
    Ok((
        StatusCode::CREATED,
        Json(view(&state.db, caller.organization_id, id).await?),
    ))
}

/// Disconnect a bill source. Org managers only. Its bills stay, and stop
/// updating.
#[utoipa::path(
    delete,
    path = "/bill-connections/{id}",
    tag = "integrations",
    params(("id" = Uuid, Path, description = "The bill connection")),
    responses(
        (status = NO_CONTENT, description = "Disconnected"),
        (status = NOT_FOUND, body = ErrorBody),
    ),
)]
async fn remove_bill_connection(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    caller.require_session()?;
    require_org_manager(&state, &caller).await?;
    let deleted = sqlx::query!(
        "DELETE FROM bill_connections WHERE organization_id = $1 AND id = $2",
        caller.organization_id,
        id
    )
    .execute(&state.db)
    .await?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Pull a bill source's bills now instead of at its next daily pull.
/// Org managers only.
#[utoipa::path(
    post,
    path = "/bill-connections/{id}/pull",
    tag = "integrations",
    params(("id" = Uuid, Path, description = "The bill connection")),
    responses(
        (status = ACCEPTED, description = "The pull starts within a minute", body = BillConnectionView),
        (status = NOT_FOUND, body = ErrorBody),
    ),
)]
async fn pull_now(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<BillConnectionView>), ApiError> {
    caller.require_session()?;
    require_org_manager(&state, &caller).await?;
    let asked = sqlx::query!(
        "UPDATE bill_connections SET pull_requested_at = now()
         WHERE organization_id = $1 AND id = $2",
        caller.organization_id,
        id
    )
    .execute(&state.db)
    .await?;
    if asked.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    crate::jobs::request_now(&state.db, JOB).await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(view(&state.db, caller.organization_id, id).await?),
    ))
}

/// Refuses callers who are not org managers as of now.
async fn require_org_manager(state: &AppState, caller: &Caller) -> Result<(), ApiError> {
    let person = caller.person()?;
    let log = Log::load(state, caller.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    access::require_org_manager(&fold, &person)
}

/// Why one connection did not pull.
#[derive(Debug, thiserror::Error)]
enum PullFailure {
    #[error("{0}")]
    Pull(#[from] PullError),
    #[error("{0}")]
    Secret(#[from] secrets::SecretError),
    #[error("{0}")]
    Bill(String),
    #[error("{0}")]
    Database(#[from] sqlx::Error),
    #[error("{0}")]
    Api(#[from] ApiError),
}

impl PullFailure {
    /// Whether the failure is on the side of Token Controller or the
    /// provider as a whole, rather than one organization's key.
    const fn is_outage(&self) -> bool {
        match self {
            Self::Pull(error) => matches!(
                error,
                PullError::Network(_) | PullError::Provider { .. } | PullError::Format(_)
            ),
            Self::Database(_) | Self::Api(_) => true,
            Self::Secret(_) | Self::Bill(_) => false,
        }
    }
}

/// The `bill-pulls` job, against the providers' own addresses.
///
/// # Errors
///
/// As [`pull_due`].
pub async fn run_job(state: AppState) -> anyhow::Result<String> {
    pull_due(state, BaseUrls::production(), OffsetDateTime::now_utc()).await
}

/// Pulls each connection that is due, at most [`PER_RUN`] of them: the
/// month `now` falls in and the one before. A connection the provider asks
/// to wait is tried again at the next run.
///
/// # Errors
///
/// Fails when the database fails, or when every connection it tried failed
/// for a reason that is not one organization's own, so the status page
/// shows the outage.
pub async fn pull_due(
    state: AppState,
    urls: BaseUrls,
    now: OffsetDateTime,
) -> anyhow::Result<String> {
    let client = super::http::client().context("could not build the HTTP client")?;
    let due = sqlx::query!(
        "UPDATE bill_connections SET last_pull_started_at = now()
         WHERE id IN (
             SELECT id FROM bill_connections
             WHERE last_pull_started_at IS NULL
                OR last_pull_started_at <= now() - make_interval(hours => $1)
                OR pull_requested_at >= last_pull_started_at
             ORDER BY last_pull_started_at NULLS FIRST
             LIMIT $2)
         RETURNING id, organization_id, provider, sealed_key",
        PULL_EVERY_HOURS,
        PER_RUN
    )
    .fetch_all(&state.db)
    .await?;
    let mut pulled = 0_usize;
    let mut changed = 0_usize;
    let mut outages = Vec::new();
    for connection in &due {
        let result = match BillApi::from_name(&connection.provider) {
            Some(api) => {
                pull_connection(
                    &state,
                    &client,
                    &urls,
                    Due {
                        organization_id: connection.organization_id,
                        api,
                        sealed_key: &connection.sealed_key,
                    },
                    now,
                )
                .await
            }
            None => Ok(0),
        };
        let error = result.as_ref().err().map(ToString::to_string);
        let again = matches!(
            result,
            Err(PullFailure::Pull(PullError::RateLimited { .. }))
        );
        sqlx::query!(
            "UPDATE bill_connections SET
                 last_pull_at = CASE WHEN $2::text IS NULL THEN now() ELSE last_pull_at END,
                 last_error = $2,
                 pull_requested_at = CASE WHEN $3 THEN now() ELSE pull_requested_at END
             WHERE id = $1",
            connection.id,
            error,
            again
        )
        .execute(&state.db)
        .await?;
        match result {
            Ok(count) => {
                pulled = pulled.saturating_add(1);
                changed = changed.saturating_add(count);
            }
            Err(failure) if failure.is_outage() => outages.push(failure.to_string()),
            Err(_) => {}
        }
    }
    if !due.is_empty() && outages.len() == due.len() {
        anyhow::bail!(
            "no bill source could be pulled: {}",
            outages
                .into_iter()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
    Ok(format!(
        "{pulled} of {} bill sources pulled, {changed} bills changed",
        due.len()
    ))
}

/// One connection the job pulls.
struct Due<'a> {
    organization_id: Uuid,
    api: BillApi,
    sealed_key: &'a str,
}

/// Pulls the current and the previous month of one connection and records
/// each bill. Returns how many bills changed.
async fn pull_connection(
    state: &AppState,
    client: &reqwest::Client,
    urls: &BaseUrls,
    connection: Due<'_>,
    now: OffsetDateTime,
) -> Result<usize, PullFailure> {
    let key = secrets::open(&state.config.secret_key, connection.sealed_key)?;
    let current = BillMonth::containing(now);
    let months = current.previous().into_iter().chain([current]);
    let mut changed = 0_usize;
    for month in months {
        let bill = pull(client, urls, connection.api, &key, month, now.date()).await?;
        if record(state, connection.organization_id, &bill).await? {
            changed = changed.saturating_add(1);
        }
    }
    Ok(changed)
}

async fn pull(
    client: &reqwest::Client,
    urls: &BaseUrls,
    api: BillApi,
    key: &str,
    period: BillMonth,
    today: Date,
) -> Result<PulledBill, PullError> {
    match api {
        BillApi::AnthropicEnterprise => {
            anthropic_enterprise::pull(client, &urls.anthropic, key, period, today).await
        }
        BillApi::AnthropicConsole => {
            anthropic_console::pull(client, &urls.anthropic, key, period).await
        }
        BillApi::OpenAi => openai::pull(client, &urls.openai, key, period).await,
    }
}

/// A bill line as the bills area stores it, with its amount in full: a
/// line can be a fraction of a cent.
pub(crate) fn line_json(line: &BillLine) -> serde_json::Value {
    serde_json::json!({
        "date": line.date.to_string(),
        "workspace": line.workspace,
        "api_key_id": line.api_key_id,
        "model": line.model,
        "description": line.description,
        "amount": decimal_text(line.amount),
        "currency": line.amount.currency().code(),
    })
}

/// An amount in full, as a decimal: a line can be a fraction of a cent.
fn decimal_text(amount: tokencontroller_core::Money) -> String {
    rust_decimal::Decimal::new(amount.nanos(), 9)
        .normalize()
        .to_string()
}

/// A pulled or uploaded bill as the bills area takes it: the month's days
/// as the range, the net as the amount, the provider's charging written
/// on the first line.
///
/// # Errors
///
/// A plain sentence for an exchange rate the bills area cannot hold.
pub(super) fn new_bill(
    bill: &PulledBill,
    source: crate::bills::BillSource,
    recorded_by: Option<tokencontroller_core::TagValue>,
) -> Result<crate::bills::NewBill, String> {
    let range = bill
        .period()
        .range()
        .ok_or_else(|| format!("{} is not a month with days", bill.period()))?;
    let exchange_rate = bill
        .exchange_rate()
        .map(|rate| crate::bills::amount::RateNumber::parse(&rate.normalize().to_string()))
        .transpose()
        .map_err(|error| error.to_string())?;
    let mut lines = vec![serde_json::json!({
        "description": "figures",
        "charging": bill.cost_source().as_str(),
        "gross": decimal_text(bill.gross()),
        "discount": decimal_text(bill.discount()),
        "credit": decimal_text(bill.credit()),
        "net": decimal_text(bill.net()),
        "currency": bill.currency().code(),
    })];
    lines.extend(bill.lines().iter().map(line_json));
    Ok(crate::bills::NewBill {
        provider: bill.provider_name().to_owned(),
        range,
        amount: bill.net(),
        exchange_rate,
        source,
        lines,
        recorded_by,
        is_final: bill.is_final(),
    })
}

/// Hands a pulled bill to the bills area. Returns whether a new bill event
/// was stored.
async fn record(
    state: &AppState,
    organization_id: Uuid,
    bill: &PulledBill,
) -> Result<bool, PullFailure> {
    let stored = match new_bill(bill, crate::bills::BillSource::Pulled, None) {
        Ok(new) => crate::bills::record_bill(state, organization_id, new)
            .await?
            .map_err(|error| error.to_string()),
        Err(error) => Err(error),
    };
    match stored {
        Ok(recorded) => Ok(recorded.changed),
        Err(error) => Err(PullFailure::Bill(format!(
            "The {} bill for {} could not be stored: {error}",
            bill.provider_name(),
            bill.period()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_names_round_trip() {
        for api in [
            BillApi::AnthropicEnterprise,
            BillApi::AnthropicConsole,
            BillApi::OpenAi,
        ] {
            assert_eq!(BillApi::from_name(api.as_str()), Some(api));
            assert_eq!(
                serde_json::to_value(api).unwrap(),
                serde_json::json!(api.as_str())
            );
        }
        assert_eq!(BillApi::from_name("bedrock"), None);
    }
}
