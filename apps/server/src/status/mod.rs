//! Health and status: `/api/v1/health`, read by Docker, Azure, the uptime
//! check and status.tokencontroller.com, and `/api/v1/status`, the state of
//! each part status.tokencontroller.com shows.
//!
//! The status endpoint turns signals into component states: sign-in mail
//! from `mail_deliveries`, and background jobs from the job runner's
//! heartbeats in `job_runs` (the jobs area, ADR 0002) through [`job_state`]. It shows
//! states and times only, never customer data, because it is public.
//!
//! Owner: the service platform. The status signals (token entries arriving,
//! attribution rate, reconciliation residual) go here too.

use std::time::Duration;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;

#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(health))
        .routes(routes!(status))
}

/// Whether one part of the service works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Check {
    Ok,
    Failing,
}

/// The answer of the health endpoint.
#[derive(Debug, Serialize, ToSchema)]
pub struct Health {
    /// `ok` when every check is `ok`.
    pub status: Check,
    /// The running version, such as `1.0.0`.
    pub version: &'static str,
    /// `open`, or the edition the Enterprise plan build reports.
    pub edition: &'static str,
    /// Whether Postgres answers.
    pub database: Check,
}

/// Is the service up and can it reach its database?
#[utoipa::path(
    get,
    path = "/health",
    tag = "status",
    responses(
        (status = OK, description = "The service works", body = Health),
        (status = SERVICE_UNAVAILABLE, description = "A check is failing", body = Health),
    ),
)]
pub async fn health(State(state): State<AppState>) -> (StatusCode, Json<Health>) {
    let database = database_check(&state.db).await;
    let (code, status) = if database == Check::Ok {
        (StatusCode::OK, Check::Ok)
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Check::Failing)
    };
    (
        code,
        Json(Health {
            status,
            version: env!("CARGO_PKG_VERSION"),
            edition: state.extension.edition(),
            database,
        }),
    )
}

async fn database_check(db: &PgPool) -> Check {
    match sqlx::query_scalar!(r#"SELECT 1 AS "one!""#)
        .fetch_one(db)
        .await
    {
        Ok(_) => Check::Ok,
        Err(error) => {
            tracing::warn!(%error, "health check: Postgres does not answer");
            Check::Failing
        }
    }
}

/// The state of one part, in the words of status.tokencontroller.com.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PartState {
    Operational,
    /// Works, but late or with some failures.
    Degraded,
    Down,
    /// Not set up on this instance, or switched off.
    NotRunning,
}

/// One part of the service, as status.tokencontroller.com lists them.
#[derive(Debug, Serialize, ToSchema)]
pub struct Component {
    /// `service`, `bill-pulls`, `ticket-sync`, `price-tables` or
    /// `sign-in-email`.
    pub id: &'static str,
    pub state: PartState,
    /// For a job: when it last succeeded.
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_success_at: Option<OffsetDateTime>,
}

/// The answer of the status endpoint.
#[derive(Debug, Serialize, ToSchema)]
pub struct ServiceStatus {
    /// The worst state of any running part.
    pub status: PartState,
    pub version: &'static str,
    pub components: Vec<Component>,
}

/// The jobs status.tokencontroller.com shows, by their component id.
const JOBS: [&str; 3] = ["bill-pulls", "ticket-sync", "price-tables"];

/// The state of every part status.tokencontroller.com shows.
#[utoipa::path(
    get,
    path = "/status",
    tag = "status",
    responses(
        (status = OK, body = ServiceStatus),
        (status = SERVICE_UNAVAILABLE, description = "The database does not answer", body = ServiceStatus),
    ),
)]
async fn status(State(state): State<AppState>) -> (StatusCode, Json<ServiceStatus>) {
    let version = env!("CARGO_PKG_VERSION");
    if database_check(&state.db).await == Check::Failing {
        let components = vec![Component {
            id: "service",
            state: PartState::Down,
            last_success_at: None,
        }];
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ServiceStatus {
                status: PartState::Down,
                version,
                components,
            }),
        );
    }
    let mut components = vec![Component {
        id: "service",
        state: PartState::Operational,
        last_success_at: None,
    }];
    components.extend(job_components(&state).await);
    let mail = sqlx::query!(
        r#"SELECT count(*) AS "total!", count(*) FILTER (WHERE NOT sent) AS "failed!"
           FROM mail_deliveries
           WHERE created_at > now() - interval '15 minutes'
             AND kind IN ('sign_in', 'no_account', 'invite')"#
    )
    .fetch_one(&state.db)
    .await;
    components.push(Component {
        id: "sign-in-email",
        state: match (&mail, state.config.smtp_url.is_some()) {
            (_, false) => PartState::NotRunning,
            (Ok(mail), true) => mail_state(mail.total, mail.failed),
            (Err(_), true) => PartState::Degraded,
        },
        last_success_at: None,
    });
    let status = components
        .iter()
        .map(|component| component.state)
        .filter(|state| *state != PartState::NotRunning)
        .max()
        .unwrap_or(PartState::Operational);
    (
        StatusCode::OK,
        Json(ServiceStatus {
            status,
            version,
            components,
        }),
    )
}

/// The background jobs, each from its heartbeat in the job runner's
/// `job_runs` table through [`job_state`]. A job this instance does not run,
/// such as the price refresh with `TC_PRICES_REFRESH=off`, shows as not
/// running.
async fn job_components(state: &AppState) -> Vec<Component> {
    let jobs = crate::jobs::standard(&state.config);
    let now = OffsetDateTime::now_utc();
    match crate::jobs::runs(&state.db).await {
        Ok(runs) => JOBS
            .into_iter()
            .map(|id| {
                let every = jobs.iter().find(|job| job.name == id).map(|job| job.every);
                job_component(id, every, runs.iter().find(|run| run.job == id), now)
            })
            .collect(),
        Err(error) => {
            tracing::warn!("could not read the job heartbeats: {error}");
            JOBS.into_iter()
                .map(|id| Component {
                    id,
                    state: PartState::Degraded,
                    last_success_at: None,
                })
                .collect()
        }
    }
}

/// One job's component: `every` is how often this instance runs it, or
/// `None` when it does not.
fn job_component(
    id: &'static str,
    every: Option<Duration>,
    run: Option<&crate::jobs::JobRun>,
    now: OffsetDateTime,
) -> Component {
    let last_success_at = run.and_then(|run| run.last_succeeded_at);
    let state = every.map_or(PartState::NotRunning, |every| {
        job_state(
            now,
            last_success_at,
            run.map_or(0, |run| run.failures_in_a_row),
            every,
        )
    });
    Component {
        id,
        state,
        last_success_at,
    }
}

/// The state of a background job from its heartbeat: `every` is how often
/// it should succeed. It is degraded when its last run failed, or when its
/// last success is older than twice `every` plus five minutes, and not
/// running before its first run ends.
#[must_use]
pub fn job_state(
    now: OffsetDateTime,
    last_succeeded_at: Option<OffsetDateTime>,
    failures_in_a_row: i32,
    every: Duration,
) -> PartState {
    if failures_in_a_row > 0 {
        return PartState::Degraded;
    }
    let Some(last_success) = last_succeeded_at else {
        return PartState::NotRunning;
    };
    let allowed = time::Duration::try_from(every)
        .unwrap_or(time::Duration::MAX)
        .saturating_mul(2)
        .saturating_add(time::Duration::minutes(5));
    // A deadline past the last date the calendar holds is never late.
    let late = last_success
        .checked_add(allowed)
        .is_some_and(|deadline| now > deadline);
    if late {
        PartState::Degraded
    } else {
        PartState::Operational
    }
}

/// Sign-in mail is down when at least half of the last 15 minutes' mail
/// failed, and degraded when any did.
fn mail_state(total: i64, failed: i64) -> PartState {
    if failed == 0 {
        PartState::Operational
    } else if failed.saturating_mul(2) >= total {
        PartState::Down
    } else {
        PartState::Degraded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Duration as Span;

    #[test]
    fn a_job_on_time_is_operational() {
        let now = OffsetDateTime::now_utc();
        let hour = Duration::from_hours(1);
        let ago = |span: Span| now.checked_sub(span).unwrap();
        assert_eq!(
            job_state(now, Some(ago(Span::minutes(30))), 0, hour),
            PartState::Operational
        );
        // Twice the rhythm plus five minutes is still on time.
        assert_eq!(
            job_state(now, Some(ago(Span::seconds(7_500))), 0, hour),
            PartState::Operational
        );
        assert_eq!(
            job_state(now, Some(ago(Span::seconds(7_501))), 0, hour),
            PartState::Degraded
        );
    }

    #[test]
    fn a_failed_last_run_is_degraded_and_no_run_yet_is_not_running() {
        let now = OffsetDateTime::now_utc();
        let hour = Duration::from_hours(1);
        let success = now.checked_sub(Span::minutes(10));
        assert_eq!(job_state(now, success, 1, hour), PartState::Degraded);
        assert_eq!(job_state(now, None, 2, hour), PartState::Degraded);
        assert_eq!(job_state(now, None, 0, hour), PartState::NotRunning);
        // A rhythm too long for a date stays operational, never panics.
        assert_eq!(
            job_state(now, success, 0, Duration::MAX),
            PartState::Operational
        );
    }

    #[test]
    fn jobs_show_as_not_running_until_the_runner_runs_them() {
        let now = OffsetDateTime::now_utc();
        let hour = Duration::from_hours(1);
        let never = job_component("ticket-sync", Some(hour), None, now);
        assert_eq!(never.state, PartState::NotRunning);
        assert_eq!(never.last_success_at, None);
        let ran = crate::jobs::JobRun {
            job: "ticket-sync".to_owned(),
            last_started_at: Some(now),
            last_finished_at: Some(now),
            last_succeeded_at: Some(now),
            last_error: None,
            runs: 1,
            failures_in_a_row: 0,
        };
        let on_time = job_component("ticket-sync", Some(hour), Some(&ran), now);
        assert_eq!(on_time.state, PartState::Operational);
        assert_eq!(on_time.last_success_at, Some(now));
        // Switched off on this instance: not running, whatever ran before.
        let off = job_component("price-tables", None, Some(&ran), now);
        assert_eq!(off.state, PartState::NotRunning);
    }

    #[test]
    fn mail_is_down_when_half_fails() {
        assert_eq!(mail_state(0, 0), PartState::Operational);
        assert_eq!(mail_state(10, 0), PartState::Operational);
        assert_eq!(mail_state(10, 4), PartState::Degraded);
        assert_eq!(mail_state(10, 5), PartState::Down);
        assert_eq!(mail_state(1, 1), PartState::Down);
    }

    #[test]
    fn states_order_from_best_to_worst() {
        assert!(PartState::Operational < PartState::Degraded);
        assert!(PartState::Degraded < PartState::Down);
    }
}
