//! Late token sheets (docs: Account > Cadence and token sheets, late). A
//! token sheet whose cadence period is over and that has not been
//! submitted is late. The only consequence is a reminder. Only people who
//! submit token sheets at all are expected to: automations never do. An
//! empty token sheet says there was nothing to report.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use super::cadence::CadencePeriod;
use super::cadences::cadence_for;
use crate::AppState;
use crate::close::period::{Period, PeriodDates};
use crate::error::ApiError;
use crate::organizations::Role;
use crate::reports::filter::Filter;
use crate::reports::scope::Scope;
use crate::sign_in::Caller;

/// How far back late token sheets are looked for, in cadence periods.
const MAX_PERIODS_BACK: usize = 60;

pub(super) fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(list_late))
}

/// One late token sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct LateSheet {
    pub person_id: Uuid,
    pub person: String,
    pub email: String,
    #[serde(with = "crate::close::period::day")]
    #[schema(value_type = String, format = Date)]
    pub period_start: Date,
    #[serde(with = "crate::close::period::day")]
    #[schema(value_type = String, format = Date)]
    pub period_end: Date,
}

/// Someone who submits token sheets, with the cadence periods of theirs.
struct SheetUser {
    name: String,
    email: String,
    sheets: Vec<(Date, Date)>,
}

/// Whether any of `sheets` covers part of `period`.
fn covered(period: CadencePeriod, sheets: &[(Date, Date)]) -> bool {
    sheets
        .iter()
        .any(|(starts_on, ends_on)| *starts_on <= period.ends_on && *ends_on >= period.starts_on)
}

/// Every late token sheet of an organization, oldest first. With a
/// `period`, only cadence periods that overlap that month.
///
/// # Errors
///
/// A database error.
pub async fn late_sheets(
    db: &PgPool,
    organization_id: Uuid,
    period: Option<Period>,
    today: Date,
) -> Result<Vec<LateSheet>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT ts.person_id, p.name, p.email, ts.starts_on, ts.ends_on
           FROM token_sheets ts JOIN people p ON p.id = ts.person_id
           WHERE ts.organization_id = $1 AND p.removed_at IS NULL
           ORDER BY ts.person_id, ts.starts_on"#,
        organization_id,
    )
    .fetch_all(db)
    .await?;
    let mut people: BTreeMap<Uuid, SheetUser> = BTreeMap::new();
    for row in rows {
        people
            .entry(row.person_id)
            .or_insert_with(|| SheetUser {
                name: row.name,
                email: row.email,
                sheets: Vec::new(),
            })
            .sheets
            .push((row.starts_on, row.ends_on));
    }
    let mut late = Vec::new();
    for (
        person_id,
        SheetUser {
            name,
            email,
            sheets,
        },
    ) in people
    {
        let Some(cadence) = cadence_for(db, organization_id, person_id, today).await? else {
            continue;
        };
        let Some(first) = sheets.iter().map(|(starts_on, _)| *starts_on).min() else {
            continue;
        };
        let from = match period {
            Some(period) => first.max(period.first_day()),
            None => first,
        };
        let Ok(ended) = cadence.ended_between(from, today) else {
            continue;
        };
        let skip = ended.len().saturating_sub(MAX_PERIODS_BACK);
        for cadence_period in ended.into_iter().skip(skip) {
            if period.is_some_and(|period| cadence_period.starts_on > period.last_day()) {
                continue;
            }
            if !covered(cadence_period, &sheets) {
                late.push(LateSheet {
                    person_id,
                    person: name.clone(),
                    email: email.clone(),
                    period_start: cadence_period.starts_on,
                    period_end: cadence_period.ends_on,
                });
            }
        }
    }
    late.sort_by(|a, b| {
        a.period_start
            .cmp(&b.period_start)
            .then_with(|| a.person.cmp(&b.person))
    });
    Ok(late)
}

/// The cadence period running for a person, as the CLI and the Claude Code
/// mod show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct CurrentSheet {
    #[serde(with = "crate::close::period::day")]
    #[schema(value_type = String, format = Date)]
    pub period_start: Date,
    #[serde(with = "crate::close::period::day")]
    #[schema(value_type = String, format = Date)]
    pub period_end: Date,
    /// Whether an earlier cadence period still waits for its token sheet.
    pub late: bool,
}

/// A token entry sent back to its agent manager.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Rejected {
    pub token_entry_id: Uuid,
    /// The local tool's ID for the session, so it can show the reason on the
    /// draft.
    pub local_id: Option<String>,
    pub reason: String,
}

/// What the local tools need to know about a person's token sheets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct SheetContext {
    /// `null` when no cadence applies.
    pub cadence: Option<CurrentSheet>,
    pub rejected: Vec<Rejected>,
}

/// The token sheet context of one person, for `GET /me/context`.
///
/// # Errors
///
/// A database error.
pub async fn context_for(
    db: &PgPool,
    organization_id: Uuid,
    person_id: Uuid,
) -> Result<SheetContext, sqlx::Error> {
    let today = OffsetDateTime::now_utc().date();
    let cadence = match cadence_for(db, organization_id, person_id, today).await? {
        Some(cadence) => match cadence.period_containing(today) {
            Ok(current) => {
                let late = late_sheets(db, organization_id, None, today)
                    .await?
                    .iter()
                    .any(|sheet| sheet.person_id == person_id);
                Some(CurrentSheet {
                    period_start: current.starts_on,
                    period_end: current.ends_on,
                    late,
                })
            }
            Err(_) => None,
        },
        None => None,
    };
    let rejected = sqlx::query!(
        r#"SELECT e.id, e.local_id,
                  (SELECT d.reason FROM token_entry_decisions d
                   WHERE d.token_entry_id = e.id AND d.decision = 'rejected'
                   ORDER BY d.decided_at DESC LIMIT 1) AS reason
           FROM token_entries e
           WHERE e.organization_id = $1 AND e.agent_manager_id = $2 AND e.status = 'rejected'
           ORDER BY e.started_at"#,
        organization_id,
        person_id,
    )
    .fetch_all(db)
    .await?
    .into_iter()
    .map(|row| Rejected {
        token_entry_id: row.id,
        local_id: row.local_id,
        reason: row.reason.unwrap_or_default(),
    })
    .collect();
    Ok(SheetContext { cadence, rejected })
}

/// Which late token sheets to list.
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LateQuery {
    /// Only cadence periods overlapping this month, such as `2026-09`.
    #[param(value_type = Option<String>, example = "2026-09")]
    pub period: Option<Period>,
}

/// Late token sheets: whose, and for which cadence period. A people manager
/// sees the people within their filters; an agent manager their own.
#[utoipa::path(
    get,
    path = "/token-sheets/late",
    tag = "token_sheets",
    params(LateQuery),
    responses((status = OK, description = "Late token sheets, oldest first", body = Vec<LateSheet>)),
)]
pub(crate) async fn list_late(
    State(state): State<AppState>,
    caller: Caller,
    Query(query): Query<LateQuery>,
) -> Result<Json<Vec<LateSheet>>, ApiError> {
    let today = OffsetDateTime::now_utc().date();
    let late = late_sheets(&state.db, caller.organization_id, query.period, today).await?;
    if caller.role == Role::Controller {
        return Ok(Json(late));
    }
    let scope = Scope::of(&state.db, &caller).await?;
    let tags = sqlx::query!(
        "SELECT id, tags FROM people WHERE organization_id = $1",
        caller.organization_id
    )
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(|row| (row.id, Filter::from_json(&row.tags).0))
    .collect::<BTreeMap<_, _>>();
    let visible = late
        .into_iter()
        .filter(|sheet| {
            sheet.person_id == caller.person_id
                || tags.get(&sheet.person_id).is_some_and(|person_tags| {
                    scope
                        .filters()
                        .iter()
                        .any(|filter| filter.matches(person_tags))
                })
        })
        .collect();
    Ok(Json(visible))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn a_sheet_covers_the_periods_it_overlaps() {
        let week = CadencePeriod {
            starts_on: date!(2026 - 09 - 07),
            ends_on: date!(2026 - 09 - 13),
        };
        assert!(covered(
            week,
            &[(date!(2026 - 09 - 07), date!(2026 - 09 - 13))]
        ));
        assert!(covered(
            week,
            &[(date!(2026 - 09 - 01), date!(2026 - 09 - 30))]
        ));
        assert!(covered(
            week,
            &[(date!(2026 - 09 - 13), date!(2026 - 09 - 13))]
        ));
        assert!(!covered(
            week,
            &[(date!(2026 - 09 - 14), date!(2026 - 09 - 20))]
        ));
        assert!(!covered(
            week,
            &[(date!(2026 - 08 - 31), date!(2026 - 09 - 06))]
        ));
        assert!(!covered(week, &[]));
    }
}
