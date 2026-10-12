//! Token forecasts (docs: Account > Token budgets and forecasts): a
//! suggestion for a ticket from similar past work. The ticket's title and
//! description are compared with the summaries of past token entries in
//! closed periods; the tickets those token entries belong to are shown with
//! a confidence and their actual cost, and their median cost is proposed. A
//! token forecast becomes a token budget only when a person accepts it.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use tokencontroller_core::{Currency, Money, MoneyError};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::actual::{self, Actuals};
use super::embeddings::{EmbedError, Embedder, HttpEmbedder, cut};
use super::inherited::Inherited;
use super::similarity::{Embedding, median};
use super::token_budget::{self, Applies, Draft, TokenBudgetView};
use crate::AppState;
use crate::bills::amount::{format_amount, parse_amount};
use crate::error::ApiError;
use crate::organizations::Role;
use crate::reports::filter::Filter;
use crate::reports::lines::StatusView;
use crate::reports::scope::Scope;
use crate::sign_in::{Caller, Credential};

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(create_token_forecast))
        .routes(routes!(accept_token_forecast))
        .routes(routes!(get_settings, set_settings))
}

/// The lowest confidence, in percent, a past ticket needs to count as a
/// match.
pub const MIN_CONFIDENCE: u8 = 30;

/// The most matches a token forecast keeps.
pub const MAX_MATCHES: usize = 5;

/// How many summaries one token forecast embeds on the way, when they were
/// not embedded yet.
pub const EMBED_PER_FORECAST: usize = 32;

/// How many summaries go to the model in one call.
const EMBED_BATCH: usize = 64;

/// The most past token entries one token forecast compares with, newest
/// first.
const MAX_COMPARED: i64 = 5_000;

/// Why a token forecast cannot be made on an instance without a model.
pub const NO_MODEL: &str = "Token forecasts are off on this instance: it has no embeddings model configured. \
     Whoever runs it can set TC_EMBEDDINGS_URL and TC_EMBEDDINGS_MODEL (see Self-hosting > Configuration). \
     Token budgets can still be typed.";

/// Why a token forecast cannot be made for an organization that switched
/// them off.
pub const SWITCHED_OFF: &str = "Token forecasts are switched off for this organization. \
     A controller can switch them on again in the token forecast settings. Token budgets can still be typed.";

/// Why summaries could not be embedded.
#[derive(Debug, thiserror::Error)]
pub enum ForecastError {
    #[error(transparent)]
    Embed(#[from] EmbedError),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

impl From<ForecastError> for ApiError {
    fn from(error: ForecastError) -> Self {
        match error {
            ForecastError::Embed(embed) => {
                Self::Conflict(format!("No token forecast this time: {embed}."))
            }
            ForecastError::Database(database) => Self::Database(database),
        }
    }
}

/// Whether an organization uses token forecasts: on unless a controller
/// switched them off.
///
/// # Errors
///
/// A database error.
pub async fn enabled(db: &PgPool, organization_id: Uuid) -> Result<bool, sqlx::Error> {
    let enabled = sqlx::query_scalar!(
        "SELECT enabled FROM forecast_settings WHERE organization_id = $1",
        organization_id,
    )
    .fetch_optional(db)
    .await?;
    Ok(enabled.unwrap_or(true))
}

/// Embeds up to `limit` summaries of locked token entries that have no
/// embedding from `embedder`'s model yet, newest first, in one organization
/// or in every organization that uses token forecasts. A background job
/// calls it; each token forecast also embeds a few on the way. Returns how
/// many were embedded.
///
/// # Errors
///
/// [`ForecastError::Embed`] when the model fails, or a database error.
/// Summaries embedded before a failure stay stored.
pub async fn embed_pending<E: Embedder>(
    db: &PgPool,
    embedder: &E,
    organization_id: Option<Uuid>,
    limit: usize,
) -> Result<usize, ForecastError> {
    let rows = sqlx::query!(
        r#"SELECT e.id, e.organization_id, e.summary
           FROM token_entries e
           LEFT JOIN forecast_settings f ON f.organization_id = e.organization_id
           WHERE e.status = 'locked' AND btrim(e.summary) <> ''
             AND ($1::uuid IS NULL OR e.organization_id = $1)
             AND coalesce(f.enabled, true)
             AND NOT EXISTS (
                 SELECT 1 FROM summary_embeddings s WHERE s.token_entry_id = e.id AND s.model = $2
             )
           ORDER BY e.started_at DESC, e.id
           LIMIT $3"#,
        organization_id,
        embedder.model(),
        i64::try_from(limit).unwrap_or(i64::MAX),
    )
    .fetch_all(db)
    .await?;
    let mut stored_count: usize = 0;
    for chunk in rows.chunks(EMBED_BATCH) {
        let texts: Vec<String> = chunk.iter().map(|row| cut(row.summary.trim())).collect();
        let embeddings = embedder.embed(&texts).await?;
        if embeddings.len() != chunk.len() {
            return Err(EmbedError::Count {
                sent: chunk.len(),
                got: embeddings.len(),
            }
            .into());
        }
        let mut tx = db.begin().await?;
        for (row, embedding) in chunk.iter().zip(&embeddings) {
            sqlx::query!(
                r#"INSERT INTO summary_embeddings (token_entry_id, organization_id, model, millionths)
                   VALUES ($1, $2, $3, $4)
                   ON CONFLICT (token_entry_id) DO UPDATE
                       SET model = EXCLUDED.model, millionths = EXCLUDED.millionths, created_at = now()"#,
                row.id,
                row.organization_id,
                embedder.model(),
                embedding.millionths(),
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        stored_count = stored_count.saturating_add(chunk.len());
    }
    Ok(stored_count)
}

/// The best-matching past tickets: each ticket's best confidence, at or
/// above [`MIN_CONFIDENCE`], the most alike first and at most
/// [`MAX_MATCHES`]. The ticket being forecast never matches itself.
#[must_use]
pub fn best_matches<I>(candidates: I, exclude: &str) -> Vec<(String, u8)>
where
    I: IntoIterator<Item = (String, u8)>,
{
    let mut best: BTreeMap<String, u8> = BTreeMap::new();
    for (ticket, confidence) in candidates {
        if ticket == exclude || confidence < MIN_CONFIDENCE {
            continue;
        }
        let kept = best.entry(ticket).or_insert(confidence);
        *kept = (*kept).max(confidence);
    }
    let mut matches: Vec<(String, u8)> = best.into_iter().collect();
    matches.sort_by(|(a_ticket, a), (b_ticket, b)| b.cmp(a).then_with(|| a_ticket.cmp(b_ticket)));
    matches.truncate(MAX_MATCHES);
    matches
}

/// One past ticket a token forecast matched, as stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredMatch {
    pub ticket: String,
    pub title: Option<String>,
    pub confidence: u8,
    pub actual_nanos: i64,
    pub token_entries: usize,
    pub unknown_model_entries: usize,
}

impl StoredMatch {
    /// Its actual cost, when every token entry on it has a known cost.
    fn known_cost(&self, currency: Currency) -> Option<Money> {
        (self.unknown_model_entries == 0).then(|| Money::from_nanos(self.actual_nanos, currency))
    }
}

/// The median cost of the matches whose cost is fully known.
///
/// # Errors
///
/// [`MoneyError::Overflow`] when the sum of the two middle costs does not
/// fit.
pub fn proposal<'a, I>(matches: I, currency: Currency) -> Result<Option<Money>, MoneyError>
where
    I: IntoIterator<Item = &'a StoredMatch>,
{
    let costs: Vec<Money> = matches
        .into_iter()
        .filter_map(|found| found.known_cost(currency))
        .collect();
    median(&costs, currency)
}

/// One past ticket a token forecast matched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ForecastMatch {
    #[schema(example = "KD-180")]
    pub ticket: String,
    pub title: Option<String>,
    /// How alike its token entries' summaries are to the ticket, in percent.
    #[schema(example = 82)]
    pub confidence: u8,
    /// Its actual cost over all time, in the reporting currency.
    #[schema(example = "41.87")]
    pub actual: String,
    pub token_entries: usize,
    /// Token entries on a model missing from the price table: their cost is
    /// not known, so this ticket is left out of the proposal.
    pub unknown_model_entries: usize,
}

/// A token forecast for one ticket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TokenForecastView {
    pub id: Uuid,
    #[schema(example = "KD-214")]
    pub ticket: String,
    pub title: Option<String>,
    #[schema(example = "EUR")]
    pub currency: String,
    /// The median actual cost of the matches with a known cost; `null` when
    /// there is none.
    #[schema(example = "38.20")]
    pub proposed: Option<String>,
    /// The most alike first.
    pub matches: Vec<ForecastMatch>,
    /// The embeddings model it compared with.
    pub model: String,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub created_at: OffsetDateTime,
}

fn ticket_filter(key: &str) -> Filter {
    Filter(std::iter::once(("ticket".to_owned(), key.to_owned())).collect())
}

/// Checks that `scope` may set a token budget on the ticket.
async fn require_covers_ticket(
    db: &PgPool,
    organization_id: Uuid,
    scope: &Scope,
    key: &str,
) -> Result<(), ApiError> {
    let filter = ticket_filter(key);
    let inherited = Inherited::load(db, organization_id, &[&filter]).await?;
    if scope.covers(&inherited.expand(&filter)) {
        Ok(())
    } else {
        Err(ApiError::Forbidden)
    }
}

/// The text of a ticket a token forecast compares: its title and
/// description.
fn ticket_text(title: Option<&str>, description: Option<&str>) -> String {
    [title, description]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// How alike each embedded summary of a locked token entry is to `text`,
/// in percent, by token entry.
async fn alike<E: Embedder>(
    db: &PgPool,
    embedder: &E,
    organization_id: Uuid,
    text: &str,
) -> Result<BTreeMap<Uuid, u8>, ForecastError> {
    embed_pending(db, embedder, Some(organization_id), EMBED_PER_FORECAST).await?;
    let target = embedder
        .embed(&[cut(text)])
        .await?
        .into_iter()
        .next()
        .ok_or(EmbedError::Count { sent: 1, got: 0 })?;
    let stored = sqlx::query!(
        r#"SELECT s.token_entry_id, s.millionths
           FROM summary_embeddings s JOIN token_entries e ON e.id = s.token_entry_id
           WHERE s.organization_id = $1 AND s.model = $2 AND e.status = 'locked'
           ORDER BY e.started_at DESC, e.id
           LIMIT $3"#,
        organization_id,
        embedder.model(),
        MAX_COMPARED,
    )
    .fetch_all(db)
    .await?;
    Ok(stored
        .into_iter()
        .filter_map(|row| {
            let embedding = Embedding::from_millionths(row.millionths).ok()?;
            Some((row.token_entry_id, embedding.similarity(&target)?))
        })
        .collect())
}

/// The best-matching past tickets with their actual cost over all time.
async fn matches_for(
    db: &PgPool,
    organization_id: Uuid,
    scope: &Scope,
    alike: &BTreeMap<Uuid, u8>,
    key: &str,
) -> Result<(Currency, Vec<StoredMatch>), ApiError> {
    let periods = actual::all_periods(db, organization_id, OffsetDateTime::now_utc()).await?;
    let actuals = Actuals::load(db, organization_id, &periods, StatusView::All, scope).await?;
    let candidates = actuals.lines().filter_map(|line| {
        let confidence = alike.get(&line.line.entry_id)?;
        Some((line.line.tags.get("ticket")?.clone(), *confidence))
    });
    let best = best_matches(candidates, key);
    let keys: Vec<String> = best.iter().map(|(key, _)| key.clone()).collect();
    let titles = titles(db, organization_id, &keys).await?;
    let mut matches = Vec::new();
    for (key, confidence) in best {
        let actual = actuals
            .of(&ticket_filter(&key), None)
            .map_err(|error| ApiError::Internal(error.into()))?;
        matches.push(StoredMatch {
            title: titles.get(&key).cloned(),
            ticket: key,
            confidence,
            actual_nanos: actual.value.nanos(),
            token_entries: actual.token_entries,
            unknown_model_entries: actual.unknown_model_entries,
        });
    }
    Ok((actuals.currency, matches))
}

/// Makes and stores a token forecast for the ticket `key`, comparing with
/// the past token entries `scope` sees. `scope` must cover the ticket, as
/// for setting a token budget on it.
///
/// # Errors
///
/// [`ApiError::Conflict`] when the organization switched token forecasts
/// off, the ticket has no text to compare or the model fails;
/// [`ApiError::NotFound`] for no such ticket; [`ApiError::Forbidden`]
/// outside `scope`; or a database error.
pub async fn create<E: Embedder>(
    db: &PgPool,
    embedder: &E,
    organization_id: Uuid,
    created_by: Uuid,
    scope: &Scope,
    key: &str,
) -> Result<TokenForecastView, ApiError> {
    if !enabled(db, organization_id).await? {
        return Err(ApiError::Conflict(SWITCHED_OFF.to_owned()));
    }
    let ticket = sqlx::query!(
        r#"SELECT id, key, title, description FROM tickets
           WHERE organization_id = $1 AND key = $2 ORDER BY id LIMIT 1"#,
        organization_id,
        key.trim(),
    )
    .fetch_optional(db)
    .await?
    .ok_or(ApiError::NotFound)?;
    require_covers_ticket(db, organization_id, scope, &ticket.key).await?;
    let text = ticket_text(ticket.title.as_deref(), ticket.description.as_deref());
    if text.is_empty() {
        return Err(ApiError::Conflict(format!(
            "{} has no title or description to compare with past work: add one, or type a token budget",
            ticket.key
        )));
    }
    let alike = alike(db, embedder, organization_id, &text).await?;
    let (currency, matches) = matches_for(db, organization_id, scope, &alike, &ticket.key).await?;
    let proposed =
        proposal(&matches, currency).map_err(|error| ApiError::Internal(error.into()))?;
    let stored =
        serde_json::to_value(&matches).map_err(|error| ApiError::Internal(error.into()))?;
    let row = sqlx::query!(
        r#"INSERT INTO token_forecasts
               (organization_id, ticket_id, proposed_nanos, currency, matches, model, created_by)
           VALUES ($1, $2, $3, $4, $5, $6, $7)
           RETURNING id, created_at"#,
        organization_id,
        ticket.id,
        proposed.map(Money::nanos),
        currency.code(),
        stored,
        embedder.model(),
        created_by,
    )
    .fetch_one(db)
    .await?;
    Ok(TokenForecastView {
        id: row.id,
        ticket: ticket.key,
        title: ticket.title,
        currency: currency.code(),
        proposed: proposed.map(format_amount),
        matches: matches
            .iter()
            .map(|found| ForecastMatch {
                ticket: found.ticket.clone(),
                title: found.title.clone(),
                confidence: found.confidence,
                actual: format_amount(Money::from_nanos(found.actual_nanos, currency)),
                token_entries: found.token_entries,
                unknown_model_entries: found.unknown_model_entries,
            })
            .collect(),
        model: embedder.model().to_owned(),
        created_at: row.created_at,
    })
}

/// The titles of some tickets, by key.
async fn titles(
    db: &PgPool,
    organization_id: Uuid,
    keys: &[String],
) -> Result<BTreeMap<String, String>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT DISTINCT ON (key) key, title AS "title!"
           FROM tickets WHERE organization_id = $1 AND key = ANY($2) AND title IS NOT NULL
           ORDER BY key, id"#,
        organization_id,
        keys,
    )
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(|row| (row.key, row.title)).collect())
}

/// How to accept a token forecast.
#[derive(Debug, Clone, Default, Deserialize, ToSchema)]
pub struct AcceptInput {
    /// Keep only these matches, by ticket key: the token budget is their
    /// median cost. Left out: the proposal as it is.
    #[schema(example = json!(["KD-180", "KD-191"]))]
    pub tickets: Option<Vec<String>>,
    /// Type the amount instead, in the reporting currency, such as `45.00`.
    #[schema(example = "45.00")]
    pub amount: Option<String>,
}

/// The amount a token forecast is accepted with: the typed amount, else the
/// median of the picked matches, else the proposal.
fn accepted_amount(
    input: &AcceptInput,
    proposed: Option<Money>,
    matches: &[StoredMatch],
    currency: Currency,
) -> Result<Money, ApiError> {
    let typed = input
        .amount
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty());
    let amount = if let Some(text) = typed {
        parse_amount(text, currency).map_err(|error| ApiError::BadRequest(error.to_string()))?
    } else if let Some(keys) = &input.tickets {
        if let Some(stranger) = keys
            .iter()
            .find(|key| !matches.iter().any(|found| &found.ticket == *key))
        {
            return Err(ApiError::BadRequest(format!(
                "{stranger} is not one of this token forecast's matches"
            )));
        }
        let picked = matches.iter().filter(|found| keys.contains(&found.ticket));
        proposal(picked, currency)
            .map_err(|error| ApiError::Internal(error.into()))?
            .ok_or_else(|| {
                ApiError::Conflict(
                    "none of the picked matches has a known cost: pick others, or type an amount"
                        .to_owned(),
                )
            })?
    } else {
        proposed.ok_or_else(|| {
            ApiError::Conflict(
                "this token forecast proposes no amount: no match has a known cost. Type an amount instead".to_owned(),
            )
        })?
    };
    if amount.nanos() <= 0 {
        return Err(ApiError::Conflict(
            "a token budget needs an amount above zero: type one".to_owned(),
        ));
    }
    Ok(amount)
}

/// Accepts a token forecast as a token budget on its ticket for all time.
///
/// # Errors
///
/// [`ApiError::NotFound`] for no such token forecast,
/// [`ApiError::Forbidden`] outside `scope`, [`ApiError::Conflict`] when it
/// was accepted already or there is no amount, [`ApiError::BadRequest`]
/// for an amount or match that is not valid, or a database error.
pub async fn accept(
    db: &PgPool,
    organization_id: Uuid,
    person_id: Uuid,
    scope: &Scope,
    id: Uuid,
    input: &AcceptInput,
) -> Result<TokenBudgetView, ApiError> {
    let forecast = sqlx::query!(
        r#"SELECT f.currency, f.proposed_nanos, f.matches, t.key
           FROM token_forecasts f JOIN tickets t ON t.id = f.ticket_id
           WHERE f.id = $1 AND f.organization_id = $2"#,
        id,
        organization_id,
    )
    .fetch_optional(db)
    .await?
    .ok_or(ApiError::NotFound)?;
    require_covers_ticket(db, organization_id, scope, &forecast.key).await?;
    let currency =
        Currency::new(&forecast.currency).map_err(|error| ApiError::Internal(error.into()))?;
    let matches: Vec<StoredMatch> = serde_json::from_value(forecast.matches)
        .map_err(|error| ApiError::Internal(error.into()))?;
    let proposed = forecast
        .proposed_nanos
        .map(|nanos| Money::from_nanos(nanos, currency));
    let amount = accepted_amount(input, proposed, &matches, currency)?;
    let draft = Draft {
        name: None,
        filter: ticket_filter(&forecast.key),
        applies: Applies::AllTime,
        amount,
    };
    let budget_id = token_budget::insert(db, organization_id, &draft, Some(id), person_id)
        .await
        .map_err(|error| match error {
            sqlx::Error::Database(ref database) if database.is_unique_violation() => {
                ApiError::Conflict(
                    "this token forecast is already accepted as a token budget".to_owned(),
                )
            }
            other => ApiError::Database(other),
        })?;
    token_budget::view(db, organization_id, budget_id).await
}

/// Make a token forecast for a ticket from similar past work: its title
/// and description are compared with the summaries of past token entries,
/// and the median actual cost of the best-matching past tickets is
/// proposed. Nothing is planned until a person accepts it.
#[utoipa::path(
    post,
    path = "/tickets/{key}/token-forecast",
    tag = "token_budgets",
    params(("key" = String, Path, description = "The ticket key, such as KD-214")),
    responses(
        (status = CREATED, description = "The token forecast", body = TokenForecastView),
        (status = CONFLICT, description = "Token forecasts are off on this instance or for this organization, the ticket has no text, or the model failed; the message says which", body = crate::error::ErrorBody),
        (status = FORBIDDEN, description = "Outside the filters you are assigned", body = crate::error::ErrorBody),
        (status = NOT_FOUND, description = "No such ticket", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn create_token_forecast(
    State(state): State<AppState>,
    caller: Caller,
    Path(key): Path<String>,
) -> Result<(StatusCode, Json<TokenForecastView>), ApiError> {
    caller.require_role(Role::PeopleManager)?;
    caller.require_credential(&[Credential::Session])?;
    let config = state
        .config
        .embeddings
        .as_ref()
        .ok_or_else(|| ApiError::Conflict(NO_MODEL.to_owned()))?;
    let embedder = HttpEmbedder::new(config).map_err(ForecastError::from)?;
    let scope = Scope::of(&state.db, &caller).await?;
    let forecast = create(
        &state.db,
        &embedder,
        caller.organization_id,
        caller.person_id,
        &scope,
        &key,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(forecast)))
}

/// Accept a token forecast: it becomes a token budget on its ticket for
/// all time. Keep only some matches, or type the amount instead.
#[utoipa::path(
    post,
    path = "/token-forecasts/{id}/accept",
    tag = "token_budgets",
    params(("id" = Uuid, Path, description = "The token forecast")),
    request_body = AcceptInput,
    responses(
        (status = CREATED, description = "The token budget", body = TokenBudgetView),
        (status = BAD_REQUEST, description = "The amount or a match is not valid", body = crate::error::ErrorBody),
        (status = CONFLICT, description = "Already accepted, or there is no amount to accept", body = crate::error::ErrorBody),
        (status = FORBIDDEN, description = "Outside the filters you are assigned", body = crate::error::ErrorBody),
        (status = NOT_FOUND, description = "No such token forecast", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn accept_token_forecast(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
    Json(input): Json<AcceptInput>,
) -> Result<(StatusCode, Json<TokenBudgetView>), ApiError> {
    caller.require_role(Role::PeopleManager)?;
    caller.require_credential(&[Credential::Session])?;
    let scope = Scope::of(&state.db, &caller).await?;
    let budget = accept(
        &state.db,
        caller.organization_id,
        caller.person_id,
        &scope,
        id,
        &input,
    )
    .await?;
    Ok((StatusCode::CREATED, Json(budget)))
}

/// Whether token forecasts are on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TokenForecastSettings {
    /// Whether the organization uses token forecasts: on unless a
    /// controller switched them off.
    pub enabled: bool,
    /// Whether this instance has an embeddings model configured. Without
    /// one, token forecasts are off whatever `enabled` says.
    pub available: bool,
    /// The embeddings model, such as `text-embedding-3-small`.
    pub model: Option<String>,
    /// Why token forecasts are off, in plain words; `null` while they are
    /// on.
    pub reason: Option<String>,
}

/// Switches token forecasts on or off for the organization.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct TokenForecastSettingsInput {
    pub enabled: bool,
}

fn settings(enabled: bool, model: Option<&str>) -> TokenForecastSettings {
    let reason = match (model, enabled) {
        (None, _) => Some(NO_MODEL.to_owned()),
        (Some(_), false) => Some(SWITCHED_OFF.to_owned()),
        (Some(_), true) => None,
    };
    TokenForecastSettings {
        enabled,
        available: model.is_some(),
        model: model.map(str::to_owned),
        reason,
    }
}

fn configured_model(state: &AppState) -> Option<&str> {
    state
        .config
        .embeddings
        .as_ref()
        .map(|config| config.model.as_str())
}

/// Whether token forecasts are on for the organization and available on
/// this instance.
#[utoipa::path(
    get,
    path = "/token-forecasts/settings",
    tag = "token_budgets",
    responses((status = OK, description = "The token forecast settings", body = TokenForecastSettings)),
)]
pub(crate) async fn get_settings(
    State(state): State<AppState>,
    caller: Caller,
) -> Result<Json<TokenForecastSettings>, ApiError> {
    caller.require_role(Role::PeopleManager)?;
    let enabled = enabled(&state.db, caller.organization_id).await?;
    Ok(Json(settings(enabled, configured_model(&state))))
}

/// Switch token forecasts on or off for the organization. Token budgets
/// work either way.
#[utoipa::path(
    put,
    path = "/token-forecasts/settings",
    tag = "token_budgets",
    request_body = TokenForecastSettingsInput,
    responses(
        (status = OK, description = "The token forecast settings", body = TokenForecastSettings),
        (status = FORBIDDEN, description = "Only a controller can switch token forecasts", body = crate::error::ErrorBody),
    ),
)]
pub(crate) async fn set_settings(
    State(state): State<AppState>,
    caller: Caller,
    Json(input): Json<TokenForecastSettingsInput>,
) -> Result<Json<TokenForecastSettings>, ApiError> {
    caller.require_role(Role::Controller)?;
    caller.require_credential(&[Credential::Session])?;
    sqlx::query!(
        r#"INSERT INTO forecast_settings (organization_id, enabled) VALUES ($1, $2)
           ON CONFLICT (organization_id) DO UPDATE SET enabled = EXCLUDED.enabled"#,
        caller.organization_id,
        input.enabled,
    )
    .execute(&state.db)
    .await?;
    Ok(Json(settings(input.enabled, configured_model(&state))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eur(cents: i64) -> Money {
        Money::from_minor_units(cents, Currency::EUR).unwrap()
    }

    fn found(ticket: &str, cents: i64, unknown: usize) -> StoredMatch {
        StoredMatch {
            ticket: ticket.to_owned(),
            title: None,
            confidence: 50,
            actual_nanos: eur(cents).nanos(),
            token_entries: 2,
            unknown_model_entries: unknown,
        }
    }

    fn candidates(pairs: &[(&str, u8)]) -> Vec<(String, u8)> {
        pairs
            .iter()
            .map(|(ticket, c)| ((*ticket).to_owned(), *c))
            .collect()
    }

    #[test]
    fn each_ticket_counts_with_its_best_confidence() {
        let best = best_matches(
            candidates(&[("KD-1", 40), ("KD-1", 75), ("KD-2", 60)]),
            "KD-9",
        );
        assert_eq!(best, candidates(&[("KD-1", 75), ("KD-2", 60)]));
    }

    #[test]
    fn the_threshold_is_inclusive() {
        let below = MIN_CONFIDENCE.checked_sub(1).unwrap();
        let best = best_matches(
            candidates(&[("KD-1", MIN_CONFIDENCE), ("KD-2", below)]),
            "KD-9",
        );
        assert_eq!(best, candidates(&[("KD-1", MIN_CONFIDENCE)]));
    }

    #[test]
    fn the_ticket_never_matches_itself_and_only_the_best_are_kept() {
        let many = candidates(&[
            ("KD-9", 100),
            ("KD-1", 90),
            ("KD-2", 80),
            ("KD-3", 70),
            ("KD-4", 70),
            ("KD-5", 60),
            ("KD-6", 50),
        ]);
        let best = best_matches(many, "KD-9");
        assert_eq!(best.len(), MAX_MATCHES);
        assert_eq!(
            best,
            candidates(&[
                ("KD-1", 90),
                ("KD-2", 80),
                ("KD-3", 70),
                ("KD-4", 70),
                ("KD-5", 60)
            ])
        );
        assert!(best_matches(Vec::new(), "KD-9").is_empty());
    }

    #[test]
    fn the_proposal_is_the_median_of_known_costs() {
        let matches = [
            found("KD-1", 1_000, 0),
            found("KD-2", 3_000, 0),
            found("KD-3", 2_000, 0),
        ];
        assert_eq!(proposal(&matches, Currency::EUR).unwrap(), Some(eur(2_000)));
        let with_unknown = [
            found("KD-1", 1_000, 0),
            found("KD-2", 9_000, 1),
            found("KD-3", 2_000, 0),
        ];
        assert_eq!(
            proposal(&with_unknown, Currency::EUR).unwrap(),
            Some(eur(1_500))
        );
        assert_eq!(
            proposal(&[found("KD-2", 9_000, 1)], Currency::EUR).unwrap(),
            None
        );
        assert_eq!(proposal(&[], Currency::EUR).unwrap(), None);
    }

    #[test]
    fn accepting_takes_the_typed_amount_the_picked_matches_or_the_proposal() {
        let matches = [
            found("KD-1", 1_000, 0),
            found("KD-2", 3_000, 0),
            found("KD-3", 5_000, 1),
        ];
        let proposed = Some(eur(2_000));
        let as_is = AcceptInput::default();
        assert_eq!(
            accepted_amount(&as_is, proposed, &matches, Currency::EUR).unwrap(),
            eur(2_000)
        );
        let picked = AcceptInput {
            tickets: Some(vec!["KD-2".to_owned()]),
            amount: None,
        };
        assert_eq!(
            accepted_amount(&picked, proposed, &matches, Currency::EUR).unwrap(),
            eur(3_000)
        );
        let typed = AcceptInput {
            tickets: Some(vec!["KD-2".to_owned()]),
            amount: Some("45.50".to_owned()),
        };
        assert_eq!(
            accepted_amount(&typed, proposed, &matches, Currency::EUR).unwrap(),
            eur(4_550)
        );
    }

    #[test]
    fn accepting_needs_an_amount_above_zero() {
        let matches = [found("KD-1", 0, 0), found("KD-3", 5_000, 1)];
        let unknown = AcceptInput {
            tickets: Some(vec!["KD-3".to_owned()]),
            amount: None,
        };
        assert!(matches!(
            accepted_amount(&unknown, None, &matches, Currency::EUR),
            Err(ApiError::Conflict(_))
        ));
        let stranger = AcceptInput {
            tickets: Some(vec!["GLOBEX-7".to_owned()]),
            amount: None,
        };
        assert!(matches!(
            accepted_amount(&stranger, None, &matches, Currency::EUR),
            Err(ApiError::BadRequest(_))
        ));
        assert!(matches!(
            accepted_amount(&AcceptInput::default(), None, &matches, Currency::EUR),
            Err(ApiError::Conflict(_))
        ));
        assert!(matches!(
            accepted_amount(
                &AcceptInput::default(),
                Some(eur(0)),
                &matches,
                Currency::EUR
            ),
            Err(ApiError::Conflict(_))
        ));
        let zero = AcceptInput {
            tickets: None,
            amount: Some("0".to_owned()),
        };
        assert!(matches!(
            accepted_amount(&zero, Some(eur(100)), &matches, Currency::EUR),
            Err(ApiError::Conflict(_))
        ));
        let smallest = AcceptInput {
            tickets: None,
            amount: Some("0.000000001".to_owned()),
        };
        assert_eq!(
            accepted_amount(&smallest, None, &matches, Currency::EUR).unwrap(),
            Money::from_nanos(1, Currency::EUR)
        );
        let wrong = AcceptInput {
            tickets: None,
            amount: Some("lots".to_owned()),
        };
        assert!(matches!(
            accepted_amount(&wrong, None, &matches, Currency::EUR),
            Err(ApiError::BadRequest(_))
        ));
    }

    #[test]
    fn settings_say_why_token_forecasts_are_off() {
        let on = settings(true, Some("text-embedding-3-small"));
        assert!(on.enabled && on.available);
        assert_eq!(on.reason, None);
        let switched_off = settings(false, Some("text-embedding-3-small"));
        assert_eq!(switched_off.reason.as_deref(), Some(SWITCHED_OFF));
        let no_model = settings(true, None);
        assert!(!no_model.available);
        assert_eq!(no_model.reason.as_deref(), Some(NO_MODEL));
        assert!(NO_MODEL.contains("TC_EMBEDDINGS_URL"));
    }

    #[test]
    fn the_ticket_text_is_its_title_and_description() {
        assert_eq!(
            ticket_text(Some(" Cart "), Some("Fix totals")),
            "Cart\n\nFix totals"
        );
        assert_eq!(ticket_text(Some("Cart"), None), "Cart");
        assert_eq!(ticket_text(None, Some("  ")), "");
        assert_eq!(ticket_text(None, None), "");
    }

    #[test]
    fn matches_round_trip_through_json() {
        let matches = vec![found("KD-1", 1_000, 1)];
        let json = serde_json::to_value(&matches).unwrap();
        let back: Vec<StoredMatch> = serde_json::from_value(json).unwrap();
        assert_eq!(back, matches);
    }
}
