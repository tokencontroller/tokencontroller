//! Splits (docs: Rules > Splits): the assignment a person writes on a
//! session or one message, a list of fractions and tag sets that add up
//! to 1. A new split supersedes the earlier one on the same target; the
//! earlier one stays in the log.
//!
//! Owner: the service platform work. Keep this area's code, routes and
//! queries in this folder.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tokencontroller_core::state::State as Fold;
use tokencontroller_core::{
    Capability, Event, MessageId, Record, SessionId, Split, SplitPart, SplitTarget, TagValue,
};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;
use crate::error::ApiError;
use crate::events::json::{self, Amount, TagsSchema};
use crate::events::{self, Log, access, prefix};
use crate::sign_in::Member;

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(add_split))
}

/// One part of a split as the API takes and shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PartInput {
    /// The part's fraction, as a number or a decimal string. The fractions
    /// of a split add up to 1. One part may leave it out: it is 1.
    #[serde(default)]
    #[schema(value_type = Option<String>, example = "0.6")]
    pub fraction: Option<Amount>,
    #[serde(default)]
    #[schema(value_type = TagsSchema)]
    pub tags: BTreeMap<String, String>,
}

/// A split on a session, or on one message of it.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct NewSplit {
    pub session_id: String,
    /// Left out, the split is on the whole session.
    #[serde(default)]
    pub message_id: Option<String>,
    pub parts: Vec<PartInput>,
    /// When the split is dated; default now. Before the cutoff needs
    /// adjust.
    #[serde(default, with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub dated: Option<OffsetDateTime>,
}

/// A split as the API shows it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SplitView {
    /// The split's event id.
    pub id: String,
    pub session_id: String,
    pub message_id: Option<String>,
    pub parts: Vec<PartView>,
    /// The split this one superseded, if any.
    pub supersedes: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String, format = DateTime)]
    pub recorded_at: OffsetDateTime,
}

/// One part of a split as shown: its fraction as a decimal string.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PartView {
    pub fraction: String,
    #[schema(value_type = TagsSchema)]
    pub tags: BTreeMap<String, String>,
}

/// A fraction as a decimal string: `weight / total`, up to six decimals.
#[must_use]
pub fn fraction_text(weight: u64, total: u64) -> String {
    if total == 0 {
        return "0".to_owned();
    }
    Decimal::from(weight)
        .checked_div(Decimal::from(total))
        .map_or_else(
            || "0".to_owned(),
            |fraction| fraction.round_dp(6).normalize().to_string(),
        )
}

impl SplitView {
    /// The view of a split event.
    #[must_use]
    pub fn of(event: &Event, split: &Split) -> Self {
        let total: u64 = split
            .weights()
            .iter()
            .fold(0_u64, |sum, weight| sum.saturating_add(*weight));
        let (session_id, message_id) = match &split.target {
            SplitTarget::Session(id) => (id.to_string(), None),
            SplitTarget::Message {
                session_id,
                message_id,
            } => (session_id.to_string(), Some(message_id.to_string())),
        };
        Self {
            id: event.id.to_string(),
            session_id,
            message_id,
            parts: split
                .parts
                .iter()
                .map(|part| PartView {
                    fraction: fraction_text(part.weight, total),
                    tags: json::tags_map(&part.tags),
                })
                .collect(),
            supersedes: event.supersedes.as_ref().map(ToString::to_string),
            recorded_at: event.recorded_at,
        }
    }
}

/// The live split event on a target.
#[must_use]
pub fn live_split_event<'a>(log: &'a Log, target: &SplitTarget) -> Option<&'a Event> {
    let reversed = log.reversed();
    let mut superseded = std::collections::BTreeSet::new();
    for event in log.events.iter().rev() {
        if reversed.contains(&event.id) || superseded.contains(&event.id) {
            continue;
        }
        if let Some(older) = &event.supersedes {
            superseded.insert(older.clone());
        }
        if let Record::Split(split) = &event.record
            && split.target == *target
        {
            return Some(event);
        }
    }
    None
}

/// The event of a split, superseding the live split on the same target.
///
/// # Errors
///
/// [`ApiError::BadRequest`] when the split has no weight.
pub fn event_for(
    log: &Log,
    split: Split,
    person: &TagValue,
    at: OffsetDateTime,
) -> Result<Event, ApiError> {
    split
        .check()
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    let older = live_split_event(log, &split.target);
    let at = older.map_or(at, |older| Log::after(older, at));
    let mut event = Event::new(&events::new_id(prefix::SPLIT), at, Record::Split(split))
        .map_err(|error| ApiError::Internal(error.into()))?
        .by(person.clone());
    if let Some(older) = older {
        event = event.superseding(older.id.clone());
    }
    Ok(event)
}

/// Reads parts into split parts with whole-number weights.
///
/// # Errors
///
/// [`ApiError::BadRequest`] when the fractions do not add up to 1.
pub fn parts(input: &[PartInput]) -> Result<Vec<SplitPart>, ApiError> {
    if input.is_empty() {
        return Err(ApiError::BadRequest(
            "a split needs at least one part".to_owned(),
        ));
    }
    let scale = Decimal::from(1_000_000_u64);
    let mut parts = Vec::with_capacity(input.len());
    let mut total = Decimal::ZERO;
    for part in input {
        let fraction = match &part.fraction {
            Some(amount) => amount.decimal()?,
            None if input.len() == 1 => Decimal::ONE,
            None => {
                return Err(ApiError::BadRequest(
                    "every part of a split with several parts needs a fraction".to_owned(),
                ));
            }
        };
        if fraction <= Decimal::ZERO || fraction > Decimal::ONE {
            return Err(ApiError::BadRequest(format!(
                "a fraction is above 0 and at most 1, not {fraction}"
            )));
        }
        total = total
            .checked_add(fraction)
            .ok_or_else(|| ApiError::BadRequest("the fractions do not add up".to_owned()))?;
        let weight = fraction
            .checked_mul(scale)
            .ok_or_else(|| ApiError::BadRequest("the fraction does not fit".to_owned()))?;
        if weight.fract() != Decimal::ZERO {
            return Err(ApiError::BadRequest(format!(
                "a fraction has at most 6 decimals, not {fraction}"
            )));
        }
        let weight = u64::try_from(weight.normalize())
            .map_err(|_| ApiError::BadRequest(format!("{fraction} is not a fraction")))?;
        parts.push(SplitPart {
            weight,
            tags: json::tags(&part.tags)?,
        });
    }
    if total != Decimal::ONE {
        return Err(ApiError::BadRequest(format!(
            "the fractions add up to {total}, not 1"
        )));
    }
    Ok(parts)
}

/// Whether the caller may split the target: their own messages, or
/// assign on the tags of one of its lines.
///
/// # Errors
///
/// [`ApiError::NotFound`] when the target has no messages;
/// [`ApiError::NoCapability`] when the caller may not.
pub fn require_assign(
    fold: &Fold,
    person: &TagValue,
    target: &SplitTarget,
) -> Result<(), ApiError> {
    let lines: Vec<_> = fold
        .lines()
        .iter()
        .filter(|line| match target {
            SplitTarget::Session(id) => line.session_id == *id,
            SplitTarget::Message {
                session_id,
                message_id,
            } => line.session_id == *session_id && line.message_id == *message_id,
        })
        .collect();
    if lines.is_empty() {
        return Err(ApiError::NotFound);
    }
    if lines.iter().all(|line| line.person == *person) {
        return Ok(());
    }
    if lines
        .iter()
        .any(|line| access::covers(fold, person, Capability::Assign, &line.tags))
    {
        return Ok(());
    }
    Err(ApiError::NoCapability(
        "this needs assign on a filter the session is in".to_owned(),
    ))
}

/// Split a session, or one message of it. Your own, or with assign on a
/// filter the session is in. Supersedes the earlier split on the same
/// target.
#[utoipa::path(
    post,
    path = "/splits",
    tag = "splits",
    request_body = NewSplit,
    responses(
        (status = CREATED, body = SplitView),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = FORBIDDEN, body = crate::error::ErrorBody),
        (status = NOT_FOUND, description = "No such session or message", body = crate::error::ErrorBody),
    ),
)]
async fn add_split(
    State(state): State<AppState>,
    member: Member,
    Json(request): Json<NewSplit>,
) -> Result<(StatusCode, Json<SplitView>), ApiError> {
    member.require_person()?;
    let session_id = SessionId::new(request.session_id.trim())
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    let target = match request.message_id.as_deref().map(str::trim) {
        Some(id) if !id.is_empty() => SplitTarget::Message {
            session_id,
            message_id: MessageId::new(id)
                .map_err(|error| ApiError::BadRequest(error.to_string()))?,
        },
        _ => SplitTarget::Session(session_id),
    };
    let parts = parts(&request.parts)?;
    for part in &parts {
        crate::tags::check_names(&state.db, member.organization_id, &part.tags).await?;
    }
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    let person = member.person()?;
    require_assign(&fold, &person, &target)?;
    let at = access::dated(&fold, &person, request.dated)?;
    let split = Split { target, parts };
    let event = event_for(&log, split.clone(), &person, at)?;
    events::write(&state, &log, std::slice::from_ref(&event)).await?;
    Ok((StatusCode::CREATED, Json(SplitView::of(&event, &split))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(fraction: Option<&str>, tags: &[(&str, &str)]) -> PartInput {
        PartInput {
            fraction: fraction.map(|text| Amount::Text(text.to_owned())),
            tags: tags
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
        }
    }

    #[test]
    fn fractions_become_weights_that_add_up() {
        let parts = parts(&[
            part(Some("0.6"), &[("feature", "KD-1")]),
            part(Some("0.4"), &[("feature", "KD-2")]),
        ])
        .unwrap();
        assert_eq!(parts[0].weight, 600_000);
        assert_eq!(parts[1].weight, 400_000);
        assert_eq!(fraction_text(600_000, 1_000_000), "0.6");
        assert_eq!(fraction_text(1, 3), "0.333333");
        assert_eq!(fraction_text(0, 0), "0");
        assert_eq!(
            super::parts(&[part(None, &[])]).unwrap()[0].weight,
            1_000_000
        );
        assert!(super::parts(&[part(Some("0.5"), &[]), part(Some("0.4"), &[])]).is_err());
        assert!(super::parts(&[part(Some("1.5"), &[])]).is_err());
        assert!(super::parts(&[]).is_err());
        assert!(super::parts(&[part(None, &[]), part(None, &[])]).is_err());
    }

    #[test]
    fn a_split_supersedes_the_live_one_on_its_target() {
        let mara = TagValue::new("mara").unwrap();
        let target = SplitTarget::Session(SessionId::new("S1").unwrap());
        let mut log = Log {
            organization_id: uuid::Uuid::nil(),
            events: Vec::new(),
        };
        let at = time::macros::datetime!(2026-03-01 10:00 UTC);
        let first = event_for(
            &log,
            Split::whole(target.clone(), tokencontroller_core::Tags::new()),
            &mara,
            at,
        )
        .unwrap();
        assert!(first.supersedes.is_none());
        log.events.push(first.clone());
        let second = event_for(
            &log,
            Split::whole(target.clone(), tokencontroller_core::Tags::new()),
            &mara,
            at - time::Duration::days(1),
        )
        .unwrap();
        assert_eq!(second.supersedes.as_ref(), Some(&first.id));
        assert!(
            second.recorded_at > first.recorded_at,
            "dated after what it supersedes"
        );
        log.events.push(second.clone());
        assert_eq!(live_split_event(&log, &target).unwrap().id, second.id);
        let split = match &second.record {
            Record::Split(split) => Some(split),
            _ => None,
        };
        let view = SplitView::of(&second, split.unwrap());
        assert_eq!(view.parts[0].fraction, "1");
        assert!(view.message_id.is_none());
        assert!(log.check_with(&[]).is_ok());
    }
}
