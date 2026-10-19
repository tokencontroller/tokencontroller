//! The cutoff (docs: Account > Cutoff): the one date per organization up
//! to which figures are frozen. Org managers move it; moving it back is an
//! event like any other.
//!
//! Owner: the service platform work. Keep this area's code and routes in
//! this folder.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use time::{Date, OffsetDateTime};
use tokencontroller_core::Record;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;
use crate::error::ApiError;
use crate::events::{self, Log, access};
use crate::sign_in::Member;

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(get_cutoff, set_cutoff))
}

/// The cutoff as the API shows it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct CutoffView {
    /// The cutoff date, `YYYY-MM-DD`, or `null` when none was set: every
    /// figure is live.
    #[serde(with = "crate::dates::option_day")]
    #[schema(value_type = Option<String>, format = Date)]
    pub cutoff: Option<Date>,
    /// The moment the frozen figures are as of: the start of the cutoff
    /// date in UTC.
    #[serde(with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub frozen_as_of: Option<OffsetDateTime>,
    /// The event that set it.
    pub event_id: Option<String>,
    /// Who moved it last.
    pub moved_by: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    #[schema(value_type = Option<String>, format = DateTime)]
    pub moved_at: Option<OffsetDateTime>,
}

/// A new cutoff.
#[derive(Debug, Deserialize, ToSchema)]
pub struct NewCutoff {
    /// The last frozen day, `YYYY-MM-DD`.
    pub cutoff: String,
}

/// The cutoff in force as of the fold, with the event that set it.
#[must_use]
pub fn view(log: &Log, as_of: OffsetDateTime) -> CutoffView {
    let reversed: std::collections::BTreeSet<&tokencontroller_core::EventId> = log
        .events
        .iter()
        .filter(|event| event.recorded_at <= as_of)
        .filter_map(|event| match &event.record {
            Record::Reversal(id) => Some(id),
            _ => None,
        })
        .collect();
    let last = log.events.iter().rev().find(|event| {
        event.recorded_at <= as_of
            && matches!(event.record, Record::Cutoff(_))
            && !reversed.contains(&event.id)
    });
    let cutoff = last.and_then(|event| match event.record {
        Record::Cutoff(date) => Some(date),
        _ => None,
    });
    CutoffView {
        cutoff,
        frozen_as_of: cutoff.map(|date| date.midnight().assume_utc()),
        event_id: last.map(|event| event.id.to_string()),
        moved_by: last.and_then(|event| event.recorded_by.as_ref().map(ToString::to_string)),
        moved_at: last.map(|event| event.recorded_at),
    }
}

/// The organization's cutoff.
#[utoipa::path(
    get,
    path = "/cutoff",
    tag = "cutoff",
    responses((status = OK, body = CutoffView)),
)]
async fn get_cutoff(
    State(state): State<AppState>,
    member: Member,
) -> Result<Json<CutoffView>, ApiError> {
    let log = Log::load(&state, member.organization_id).await?;
    Ok(Json(view(&log, OffsetDateTime::now_utc())))
}

/// Move the cutoff. Org managers only. Moving it back is allowed.
#[utoipa::path(
    post,
    path = "/cutoff",
    tag = "cutoff",
    request_body = NewCutoff,
    responses(
        (status = CREATED, body = CutoffView),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = FORBIDDEN, body = crate::error::ErrorBody),
    ),
)]
async fn set_cutoff(
    State(state): State<AppState>,
    member: Member,
    Json(request): Json<NewCutoff>,
) -> Result<(StatusCode, Json<CutoffView>), ApiError> {
    member.require_session()?;
    let date = crate::dates::parse(&request.cutoff)?;
    let today = tokencontroller_core::price::date_of(OffsetDateTime::now_utc());
    if date > today {
        return Err(ApiError::BadRequest(
            "the cutoff cannot be in the future".to_owned(),
        ));
    }
    let log = Log::load(&state, member.organization_id).await?;
    let fold = log.fold(OffsetDateTime::now_utc())?;
    let person = member.person()?;
    access::require(
        &fold,
        &person,
        tokencontroller_core::Capability::Cutoff,
        &tokencontroller_core::Filter::default(),
    )?;
    let event = events::event(Record::Cutoff(date))?.by(person);
    events::write(&state, &log, std::slice::from_ref(&event)).await?;
    crate::organizations::record_audit_event(
        &state,
        member.organization_id,
        Some(member.person_id),
        "cutoff.moved",
        None,
    );
    let log = Log::load(&state, member.organization_id).await?;
    Ok((
        StatusCode::CREATED,
        Json(view(&log, OffsetDateTime::now_utc())),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::{date, datetime};
    use tokencontroller_core::Event;

    #[test]
    fn the_last_cutoff_not_reversed_is_in_force() {
        let log = Log {
            organization_id: uuid::Uuid::nil(),
            events: vec![
                Event::new(
                    "cut_1",
                    datetime!(2026-04-01 00:00 UTC),
                    Record::Cutoff(date!(2026 - 03 - 31)),
                )
                .unwrap(),
                Event::new(
                    "cut_2",
                    datetime!(2026-04-02 00:00 UTC),
                    Record::Cutoff(date!(2026 - 04 - 01)),
                )
                .unwrap(),
                Event::new(
                    "rev_1",
                    datetime!(2026-04-03 00:00 UTC),
                    Record::Reversal(tokencontroller_core::EventId::new("cut_2").unwrap()),
                )
                .unwrap(),
            ],
        };
        let now = view(&log, datetime!(2026-04-04 00:00 UTC));
        assert_eq!(now.cutoff, Some(date!(2026 - 03 - 31)));
        assert_eq!(now.event_id.as_deref(), Some("cut_1"));
        assert_eq!(now.frozen_as_of, Some(datetime!(2026-03-31 00:00 UTC)));
        let before = view(&log, datetime!(2026-03-01 00:00 UTC));
        assert!(before.cutoff.is_none());
        assert!(before.moved_by.is_none());
    }
}
