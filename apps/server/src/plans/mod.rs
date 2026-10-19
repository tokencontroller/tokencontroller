//! Plans and estimates (docs: Account > Plans and estimates): estimates as
//! distributions from any estimator, accepting one as the plan for its tag
//! set, and shares of a project's plan per person.
//!
//! Owner: the service platform work. Keep this area's code and routes in
//! this folder.

mod classifier;

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};
use tokencontroller_core::state::State as Fold;
use tokencontroller_core::{
    Accept, BASIS_POINTS, Band, Capability, DateRange, Distribution, Estimate, EventId, Filter,
    Money, Record, Share, TagValue, Tags, price::date_of,
};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;
use crate::error::ApiError;
use crate::events::json::{self, Amount, TagsSchema};
use crate::events::{self, Log, access};
use crate::sign_in::Member;

pub use classifier::{Classifier, DeviationLabelled};

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(add_estimate))
        .routes(routes!(get_estimate))
        .routes(routes!(accept_estimate))
        .routes(routes!(list_shares, add_share))
}

/// One band of a distribution as the API writes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct BandView {
    /// The band's upper end in US dollars, or `null` for the last band.
    #[schema(value_type = Option<String>, example = "250.00")]
    pub up_to: Option<Amount>,
    /// The chance of the band, as a fraction such as `0.31`.
    #[schema(value_type = String, example = "0.31")]
    pub p: Amount,
}

/// Ask for an estimate, or type one.
#[derive(Debug, Deserialize, ToSchema)]
pub struct NewEstimate {
    /// The description: a prompt, a spec, a feature or acceptance criteria.
    pub work: String,
    /// The tag set the estimate is for, such as `{"feature": "KD-1"}`.
    #[serde(default)]
    #[schema(value_type = TagsSchema)]
    pub tags: BTreeMap<String, String>,
    /// Overhead inside the amount, such as `15`.
    #[serde(default)]
    pub markup_percent: Option<u32>,
    /// Anything that helps the estimator, such as the cost of earlier work.
    #[serde(default)]
    pub context: Option<String>,
    /// Type the median yourself instead of asking the classifier.
    #[serde(default)]
    #[schema(value_type = Option<String>, example = "500.00")]
    pub median: Option<Amount>,
    /// Type the bands yourself. They add up to 1.
    #[serde(default)]
    pub distribution: Option<Vec<BandView>>,
    /// The first day of the work, `YYYY-MM-DD`. Default today.
    #[serde(default)]
    pub from: Option<String>,
    /// The last day of the work, `YYYY-MM-DD`. Default 30 days after
    /// `from`.
    #[serde(default)]
    pub to: Option<String>,
}

/// An estimate as the API shows it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct EstimateView {
    /// The estimate's event id, such as `est_4k2q9w`.
    pub id: String,
    pub work: String,
    #[schema(value_type = TagsSchema)]
    pub tags: BTreeMap<String, String>,
    pub currency: &'static str,
    /// The median in US dollars, markup inside.
    pub median: String,
    pub distribution: Vec<BandView>,
    pub markup_percent: u32,
    /// Who made it: `classifier`, or the person's email.
    pub estimator: String,
    #[serde(with = "crate::dates::day")]
    #[schema(value_type = String, format = Date)]
    pub from: Date,
    #[serde(with = "crate::dates::day")]
    #[schema(value_type = String, format = Date)]
    pub to: Date,
    /// The plan amount: the median with the markup inside.
    pub amount: String,
    /// Whether this estimate is the accepted plan for its tag set.
    pub accepted: bool,
    /// The cost of its tag set over its range so far.
    pub cost: Option<String>,
    /// Plan minus cost, where accepted. Positive is favourable.
    pub deviation: Option<String>,
    /// `scope`, `trouble` or `estimate_itself`, from the classifier.
    pub deviation_label: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub recorded_at: OffsetDateTime,
}

/// An accept as the API shows it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Accepted {
    /// The accept event's id.
    pub id: String,
    pub estimate: EstimateView,
}

/// A person's share of a project's plan.
#[derive(Debug, Deserialize, ToSchema)]
pub struct NewShare {
    /// The person, by email.
    pub person: String,
    /// The project's key.
    pub project: String,
    /// Such as `0.6`.
    #[schema(value_type = String, example = "0.6")]
    pub fraction: Amount,
    /// `YYYY-MM-DD`.
    pub from: String,
    /// `YYYY-MM-DD`; open-ended when left out.
    #[serde(default)]
    pub to: Option<String>,
}

/// A share as the API shows it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ShareView {
    pub id: String,
    pub person: String,
    pub project: String,
    pub fraction: String,
    #[serde(with = "crate::dates::day")]
    #[schema(value_type = String, format = Date)]
    pub from: Date,
    #[serde(with = "crate::dates::option_day")]
    #[schema(value_type = Option<String>, format = Date)]
    pub to: Option<Date>,
    /// The sum of the project's shares on the share's first day, where it
    /// is over 1. Shown, never corrected.
    pub over: Option<String>,
}

fn basis_points(fraction: &Amount) -> Result<u32, ApiError> {
    let fraction = fraction.decimal()?;
    let scaled = fraction
        .checked_mul(rust_decimal::Decimal::from(BASIS_POINTS))
        .ok_or_else(|| ApiError::BadRequest("the fraction does not fit".to_owned()))?;
    if scaled.fract() != rust_decimal::Decimal::ZERO {
        return Err(ApiError::BadRequest(format!(
            "a fraction has at most 4 decimals, not {fraction}"
        )));
    }
    u32::try_from(scaled.normalize())
        .map_err(|_| ApiError::BadRequest(format!("{fraction} is not a fraction")))
}

fn fraction_text(basis_points: u64) -> String {
    rust_decimal::Decimal::new(i64::try_from(basis_points).unwrap_or(i64::MAX), 4)
        .normalize()
        .to_string()
}

fn bands(views: &[BandView]) -> Result<Vec<Band>, ApiError> {
    let mut bands = Vec::with_capacity(views.len());
    for band in views {
        bands.push(Band {
            up_to: band
                .up_to
                .as_ref()
                .map(|amount| amount.money(tokencontroller_core::Currency::USD))
                .transpose()?,
            basis_points: basis_points(&band.p)?,
        });
    }
    Ok(bands)
}

fn band_views(bands: &[Band]) -> Vec<BandView> {
    bands
        .iter()
        .map(|band| BandView {
            up_to: band.up_to.map(|amount| Amount::Text(json::money(amount))),
            p: Amount::Text(fraction_text(u64::from(band.basis_points))),
        })
        .collect()
}

/// The view of an estimate with its figures as of the fold.
///
/// # Errors
///
/// [`ApiError::Internal`] when an amount does not fit.
pub fn view(
    fold: &Fold,
    id: &EventId,
    recorded_at: OffsetDateTime,
    estimate: &Estimate,
    label: Option<&str>,
) -> Result<EstimateView, ApiError> {
    let overflow =
        |error: tokencontroller_core::MoneyError| ApiError::Internal(anyhow::anyhow!("{error}"));
    let accepted = fold
        .accepted(&estimate.tags)
        .is_some_and(|accepted| accepted.id == *id);
    let filter = Filter(estimate.tags.clone());
    let cost = if estimate.tags.is_empty() {
        None
    } else {
        Some(fold.cost(&filter, estimate.range).map_err(overflow)?.amount)
    };
    let amount = estimate.amount().map_err(overflow)?;
    let deviation = match (accepted, cost) {
        (true, Some(cost)) => Some(amount.checked_sub(cost).map_err(overflow)?),
        _ => None,
    };
    Ok(EstimateView {
        id: id.to_string(),
        work: estimate.description.clone(),
        tags: json::tags_map(&estimate.tags),
        currency: "USD",
        median: json::money(estimate.distribution.median),
        distribution: band_views(&estimate.distribution.bands),
        markup_percent: estimate.markup_basis_points.checked_div(100).unwrap_or(0),
        estimator: estimate.estimator.clone(),
        from: estimate.range.from,
        to: estimate.range.to,
        amount: json::money(amount),
        accepted,
        cost: cost.map(json::money),
        deviation: deviation.map(json::money),
        deviation_label: label.map(str::to_owned),
        recorded_at,
    })
}

fn find_estimate<'a>(
    fold: &'a Fold,
    id: &str,
) -> Result<&'a tokencontroller_core::Recorded<Estimate>, ApiError> {
    let id = EventId::new(id).map_err(|_| ApiError::NotFound)?;
    fold.estimates()
        .find(|estimate| estimate.id == id)
        .ok_or(ApiError::NotFound)
}

/// Ask any estimator for an estimate: the classifier from `work` and the
/// history of every tag value, or yourself by sending `median` and, if you
/// like, `distribution`. Any token can ask. Needs plan on the tag set
/// when one is given.
#[utoipa::path(
    post,
    path = "/estimates",
    tag = "plans",
    request_body = NewEstimate,
    responses(
        (status = CREATED, body = EstimateView),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = FORBIDDEN, body = crate::error::ErrorBody),
        (status = CONFLICT, description = "No classifier is set up; send a median", body = crate::error::ErrorBody),
    ),
)]
async fn add_estimate(
    State(state): State<AppState>,
    member: Member,
    Json(request): Json<NewEstimate>,
) -> Result<(StatusCode, Json<EstimateView>), ApiError> {
    let work = request.work.trim();
    if work.is_empty() || work.chars().count() > 50_000 {
        return Err(ApiError::BadRequest(
            "work describes the work in 1 to 50,000 characters".to_owned(),
        ));
    }
    let tags = json::tags(&request.tags)?;
    crate::tags::check_names(&state.db, member.organization_id, &tags).await?;
    let markup = request.markup_percent.unwrap_or(0);
    if markup > 1_000 {
        return Err(ApiError::BadRequest(
            "the markup is at most 1,000 percent".to_owned(),
        ));
    }
    let today = date_of(OffsetDateTime::now_utc());
    let from = crate::dates::parse_optional(request.from.as_deref())?.unwrap_or(today);
    let to = match crate::dates::parse_optional(request.to.as_deref())? {
        Some(to) => to,
        None => from.saturating_add(time::Duration::days(29)),
    };
    let range = crate::dates::range_of(from, to)?;
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    let person = member.person()?;
    if let Some(first) = crate::tags::unknown_values(&fold, &tags).first() {
        return Err(ApiError::BadRequest(format!(
            "{first} is not a value on record; add it first"
        )));
    }
    if !tags.is_empty() {
        access::require(&fold, &person, Capability::Plan, &Filter(tags.clone()))?;
    }
    let (distribution, estimator) = match (&request.median, &request.distribution) {
        (Some(median), bands_given) => {
            let median = median.money(tokencontroller_core::Currency::USD)?;
            if median.nanos().is_negative() {
                return Err(ApiError::BadRequest("a median is not negative".to_owned()));
            }
            (
                Distribution {
                    median,
                    bands: bands(bands_given.as_deref().unwrap_or_default())?,
                },
                person.to_string(),
            )
        }
        (None, Some(_)) => {
            return Err(ApiError::BadRequest(
                "a typed distribution needs its median".to_owned(),
            ));
        }
        (None, None) => {
            let Some(classifier) = Classifier::from_config(&state.config) else {
                return Err(ApiError::Conflict(
                    "this instance has no classifier; send a median, or set TC_CLASSIFIER_URL and TC_CLASSIFIER_MODEL".to_owned(),
                ));
            };
            let history = fold
                .history()
                .map_err(|error| ApiError::Internal(anyhow::anyhow!("{error}")))?;
            (
                classifier
                    .estimate(work, &tags, request.context.as_deref(), &history)
                    .await?,
                "classifier".to_owned(),
            )
        }
    };
    let estimate = Estimate {
        description: work.to_owned(),
        tags,
        distribution,
        markup_basis_points: markup.saturating_mul(100),
        estimator,
        range,
    };
    estimate
        .check()
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    let event = events::event(Record::Estimate(estimate.clone()))?.by(person);
    events::write(&state, &log, std::slice::from_ref(&event)).await?;
    let fold = Log {
        organization_id: member.organization_id,
        events: log
            .events
            .iter()
            .cloned()
            .chain(std::iter::once(event.clone()))
            .collect(),
    }
    .fold(OffsetDateTime::now_utc().saturating_add(time::Duration::seconds(1)))?;
    Ok((
        StatusCode::CREATED,
        Json(view(&fold, &event.id, event.recorded_at, &estimate, None)?),
    ))
}

/// An estimate with the cost of its tag set and the labelled deviation.
#[utoipa::path(
    get,
    path = "/estimates/{id}",
    tag = "plans",
    params(("id" = String, Path, description = "The estimate's id")),
    responses(
        (status = OK, body = EstimateView),
        (status = NOT_FOUND, body = crate::error::ErrorBody),
    ),
)]
async fn get_estimate(
    State(state): State<AppState>,
    member: Member,
    Path(id): Path<String>,
) -> Result<Json<EstimateView>, ApiError> {
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    let person = member.person()?;
    let recorded = find_estimate(&fold, &id)?;
    if !recorded.value.tags.is_empty() && !access::can_view(&fold, &person, &recorded.value.tags) {
        return Err(ApiError::NotFound);
    }
    let mut estimate = view(
        &fold,
        &recorded.id,
        recorded.recorded_at,
        &recorded.value,
        None,
    )?;
    if let (Some(deviation), Some(classifier)) = (
        estimate.deviation.as_deref(),
        Classifier::from_config(&state.config),
    ) {
        match classifier
            .label_deviation(
                &recorded.value,
                estimate.cost.as_deref().unwrap_or("0"),
                deviation,
            )
            .await
        {
            Ok(label) => estimate.deviation_label = Some(label.as_str().to_owned()),
            Err(error) => tracing::warn!("deviation label: {error}"),
        }
    }
    Ok(Json(estimate))
}

/// Make an estimate the plan for its tag set. Accepting another one later
/// supersedes it. Needs plan on the tag set.
#[utoipa::path(
    post,
    path = "/estimates/{id}/accept",
    tag = "plans",
    params(("id" = String, Path, description = "The estimate's id")),
    responses(
        (status = CREATED, body = Accepted),
        (status = BAD_REQUEST, description = "The estimate has no tags", body = crate::error::ErrorBody),
        (status = FORBIDDEN, body = crate::error::ErrorBody),
        (status = NOT_FOUND, body = crate::error::ErrorBody),
    ),
)]
async fn accept_estimate(
    State(state): State<AppState>,
    member: Member,
    Path(id): Path<String>,
) -> Result<(StatusCode, Json<Accepted>), ApiError> {
    member.require_session()?;
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    let person = member.person()?;
    let recorded = find_estimate(&fold, &id)?.clone();
    if recorded.value.tags.is_empty() {
        return Err(ApiError::BadRequest(
            "an estimate without tags cannot be the plan; ask for one with the tag set".to_owned(),
        ));
    }
    access::require(
        &fold,
        &person,
        Capability::Plan,
        &Filter(recorded.value.tags.clone()),
    )?;
    let older = fold.accepted(&recorded.value.tags).and_then(|accepted| {
        log.events.iter().rev().find(|event| {
            matches!(&event.record, Record::Accept(accept) if accept.estimate == accepted.id)
                && log.is_live(&event.id)
        })
    });
    let now = OffsetDateTime::now_utc();
    let at = older.map_or(now, |older| Log::after(older, now));
    let mut event = tokencontroller_core::Event::new(
        &events::new_id(events::prefix::ACCEPT),
        at,
        Record::Accept(Accept {
            estimate: recorded.id.clone(),
        }),
    )
    .map_err(|error| ApiError::Internal(error.into()))?
    .by(person);
    if let Some(older) = older {
        event = event.superseding(older.id.clone());
    }
    events::write(&state, &log, std::slice::from_ref(&event)).await?;
    let fold = Log {
        organization_id: member.organization_id,
        events: log
            .events
            .iter()
            .cloned()
            .chain(std::iter::once(event.clone()))
            .collect(),
    }
    .fold(now.saturating_add(time::Duration::seconds(1)))?;
    Ok((
        StatusCode::CREATED,
        Json(Accepted {
            id: event.id.to_string(),
            estimate: view(
                &fold,
                &recorded.id,
                recorded.recorded_at,
                &recorded.value,
                None,
            )?,
        }),
    ))
}

/// A person's share of a project's plan between two dates. Needs plan on
/// the project. Shares may add up to more than 1; the sum is shown.
#[utoipa::path(
    post,
    path = "/shares",
    tag = "plans",
    request_body = NewShare,
    responses(
        (status = CREATED, body = ShareView),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = FORBIDDEN, body = crate::error::ErrorBody),
    ),
)]
async fn add_share(
    State(state): State<AppState>,
    member: Member,
    Json(request): Json<NewShare>,
) -> Result<(StatusCode, Json<ShareView>), ApiError> {
    member.require_session()?;
    let holder =
        crate::organizations::member_by_email(&state.db, member.organization_id, &request.person)
            .await?
            .ok_or_else(|| ApiError::BadRequest(format!("{} is not a member", request.person)))?;
    let person = crate::organizations::person_of(&state.db, member.organization_id, holder).await?;
    let project = json::tag_value("project", request.project.trim())?;
    let fraction = basis_points(&request.fraction)?;
    if fraction == 0 {
        return Err(ApiError::BadRequest("a share is above 0".to_owned()));
    }
    let from = crate::dates::parse(&request.from)?;
    let to = crate::dates::parse_optional(request.to.as_deref())?;
    if to.is_some_and(|to| to < from) {
        return Err(ApiError::BadRequest("to is before from".to_owned()));
    }
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    let caller = member.person()?;
    if fold
        .values()
        .tag_value(&tokencontroller_core::TagName::project(), &project)
        .is_none()
    {
        return Err(ApiError::BadRequest(format!(
            "project {project} is not a value on record"
        )));
    }
    let mut tags = Tags::new();
    tags.insert(tokencontroller_core::TagName::project(), project.clone());
    access::require(&fold, &caller, Capability::Plan, &Filter(tags))?;
    let share = Share {
        person: person.clone(),
        project: project.clone(),
        fraction_basis_points: fraction,
        from,
        to,
    };
    let event = events::event(Record::Share(share.clone()))?.by(caller);
    events::write(&state, &log, std::slice::from_ref(&event)).await?;
    let mut shares: Vec<Share> = fold.shares().to_vec();
    shares.push(share);
    let over = tokencontroller_core::plan::share_sums(&shares, from)
        .into_iter()
        .find(|sum| sum.project == project && sum.is_over())
        .map(|sum| fraction_text(sum.basis_points));
    Ok((
        StatusCode::CREATED,
        Json(ShareView {
            id: event.id.to_string(),
            person: person.to_string(),
            project: project.to_string(),
            fraction: fraction_text(u64::from(fraction)),
            from,
            to,
            over,
        }),
    ))
}

/// The shares in force whose project the person may view, oldest first.
#[must_use]
pub fn share_views(log: &Log, fold: &Fold, person: &TagValue) -> Vec<ShareView> {
    let shares = fold.shares();
    let mut views = Vec::new();
    for event in &log.events {
        let Record::Share(share) = &event.record else {
            continue;
        };
        if event.recorded_at > fold.as_of() || !log.is_live(&event.id) {
            continue;
        }
        let mut tags = Tags::new();
        tags.insert(
            tokencontroller_core::TagName::project(),
            share.project.clone(),
        );
        // A person sees their own shares, and anyone who views the project
        // sees all of its shares.
        if share.person != *person && !access::can_view(fold, person, &tags) {
            continue;
        }
        let over = tokencontroller_core::plan::share_sums(shares, share.from)
            .into_iter()
            .find(|sum| sum.project == share.project && sum.is_over())
            .map(|sum| fraction_text(sum.basis_points));
        views.push(ShareView {
            id: event.id.to_string(),
            person: share.person.to_string(),
            project: share.project.to_string(),
            fraction: fraction_text(u64::from(share.fraction_basis_points)),
            from: share.from,
            to: share.to,
            over,
        });
    }
    views
}

/// The shares in force whose project the caller may view.
#[utoipa::path(
    get,
    path = "/shares",
    tag = "plans",
    params(crate::tags::AsOfQuery),
    responses((status = OK, body = Vec<ShareView>)),
)]
async fn list_shares(
    State(state): State<AppState>,
    member: Member,
    Query(query): Query<crate::tags::AsOfQuery>,
) -> Result<Json<Vec<ShareView>>, ApiError> {
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(crate::dates::as_of(query.as_of.as_deref())?)?;
    Ok(Json(share_views(&log, &fold, &member.person()?)))
}

/// The plan of a person over a window, for reports.
///
/// # Errors
///
/// [`ApiError::Internal`] when an amount does not fit.
pub fn person_plan(fold: &Fold, person: &TagValue, window: DateRange) -> Result<Money, ApiError> {
    fold.person_plan(person, window)
        .map_err(|error| ApiError::Internal(anyhow::anyhow!("{error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fractions_read_as_basis_points() {
        assert_eq!(
            basis_points(&Amount::Text("0.6".to_owned())).unwrap(),
            6_000
        );
        assert_eq!(basis_points(&Amount::Text("1".to_owned())).unwrap(), 10_000);
        assert!(basis_points(&Amount::Text("0.33333".to_owned())).is_err());
        assert_eq!(fraction_text(6_000), "0.6");
        assert_eq!(fraction_text(10_000), "1");
        let bands = bands(&[
            BandView {
                up_to: Some(Amount::Text("250".to_owned())),
                p: Amount::Text("0.04".to_owned()),
            },
            BandView {
                up_to: None,
                p: Amount::Text("0.96".to_owned()),
            },
        ])
        .unwrap();
        assert_eq!(bands[0].basis_points, 400);
        assert!(bands[1].up_to.is_none());
        assert_eq!(band_views(&bands)[0].p, Amount::Text("0.04".to_owned()));
    }
}
