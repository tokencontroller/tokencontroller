//! Health and status: `/api/v1/health`, read by Docker, Azure, the uptime
//! check and status.tokencontroller.com, and `/api/v1/status`, the state of
//! each part status.tokencontroller.com shows.
//!
//! Background jobs report every run with [`record_job_run`]; the status
//! endpoint turns those heartbeats into component states. It shows states
//! and times only, never customer data, because it is public.
//!
//! Owner: the service platform. The status signals (token entries arriving,
//! attribution rate, reconciliation residual, job heartbeats) go here too.

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
    let now = OffsetDateTime::now_utc();
    let mut components = vec![Component {
        id: "service",
        state: PartState::Operational,
        last_success_at: None,
    }];
    let heartbeats = sqlx::query!(
        "SELECT job, last_success_at, last_failure_at, expected_every_seconds FROM job_heartbeats"
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();
    for job in JOBS {
        let beat = heartbeats.iter().find(|beat| beat.job == job);
        components.push(Component {
            id: job,
            state: beat.map_or(PartState::NotRunning, |beat| {
                job_state(
                    now,
                    beat.last_success_at,
                    beat.last_failure_at,
                    beat.expected_every_seconds,
                )
            }),
            last_success_at: beat.and_then(|beat| beat.last_success_at),
        });
    }
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

/// A job is late when its last success is older than twice its rhythm
/// plus five minutes, and degraded when its last run failed.
fn job_state(
    now: OffsetDateTime,
    last_success: Option<OffsetDateTime>,
    last_failure: Option<OffsetDateTime>,
    every_seconds: i32,
) -> PartState {
    let Some(last_success) = last_success else {
        return PartState::Degraded;
    };
    let allowed = i64::from(every_seconds)
        .saturating_mul(2)
        .saturating_add(300);
    let late = last_success
        .checked_add(time::Duration::seconds(allowed))
        .is_none_or(|deadline| now > deadline);
    let failed_since = last_failure.is_some_and(|failure| failure > last_success);
    if late || failed_since {
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

/// Records one run of a background job, such as `bill-pulls`, for the
/// status page. `every` is how often the job should succeed; a job whose
/// last success is older than twice that shows as degraded. Call it after
/// every run, with the error in plain words (no secrets) if it failed.
///
/// # Errors
///
/// Fails if the job name is not lower-case words with dashes, `every` is
/// under a second or over 68 years, or a query fails.
pub async fn record_job_run(
    db: &PgPool,
    job: &str,
    every: Duration,
    outcome: Result<(), &str>,
) -> Result<(), sqlx::Error> {
    let every_seconds = i32::try_from(every.as_secs()).unwrap_or(i32::MAX).max(1);
    let (succeeded, error) = match outcome {
        Ok(()) => (true, None),
        Err(error) => (false, Some(error.chars().take(500).collect::<String>())),
    };
    sqlx::query!(
        r#"INSERT INTO job_heartbeats (job, last_success_at, last_failure_at, last_error, expected_every_seconds)
           VALUES ($1, CASE WHEN $2 THEN now() END, CASE WHEN $2 THEN NULL ELSE now() END, $3, $4)
           ON CONFLICT (job) DO UPDATE SET
             last_success_at = CASE WHEN $2 THEN now() ELSE job_heartbeats.last_success_at END,
             last_failure_at = CASE WHEN $2 THEN job_heartbeats.last_failure_at ELSE now() END,
             last_error = CASE WHEN $2 THEN job_heartbeats.last_error ELSE $3 END,
             expected_every_seconds = $4"#,
        job,
        succeeded,
        error,
        every_seconds
    )
    .execute(db)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Duration as Span;

    #[test]
    fn a_job_on_time_is_operational() {
        let now = OffsetDateTime::now_utc();
        let hour = 3_600;
        assert_eq!(
            job_state(
                now,
                Some(now.checked_sub(Span::minutes(30)).unwrap()),
                None,
                hour
            ),
            PartState::Operational
        );
        // Twice the rhythm plus five minutes is still on time.
        assert_eq!(
            job_state(
                now,
                Some(now.checked_sub(Span::seconds(7_500)).unwrap()),
                None,
                hour
            ),
            PartState::Operational
        );
        assert_eq!(
            job_state(
                now,
                Some(now.checked_sub(Span::seconds(7_501)).unwrap()),
                None,
                hour
            ),
            PartState::Degraded
        );
    }

    #[test]
    fn a_failed_last_run_or_no_success_is_degraded() {
        let now = OffsetDateTime::now_utc();
        let success = now.checked_sub(Span::minutes(10)).unwrap();
        assert_eq!(
            job_state(
                now,
                Some(success),
                Some(now.checked_sub(Span::minutes(5)).unwrap()),
                3_600
            ),
            PartState::Degraded
        );
        assert_eq!(
            job_state(
                now,
                Some(success),
                Some(now.checked_sub(Span::minutes(20)).unwrap()),
                3_600
            ),
            PartState::Operational
        );
        assert_eq!(job_state(now, None, Some(now), 3_600), PartState::Degraded);
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
