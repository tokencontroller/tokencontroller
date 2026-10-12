//! The background job runner: a Postgres advisory lock per job and a heartbeat
//! per job (ADR 0002).
//!
//! Every replica runs the same loop. Each tick it looks for jobs that are due
//! (their interval has passed since the last start, or someone asked for a
//! run now), takes the job's advisory lock without waiting, checks again that
//! the job is still due, runs it and writes the heartbeat. A replica that
//! does not get the lock skips the job, so each run happens once.
//!
//! Owner: the collection and integrations work. Keep this area's code, routes and queries in
//! this folder, and its migrations in `migrations/` named
//! `<timestamp>_jobs_<what>.sql`.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use serde::Serialize;
use sha2::{Digest as _, Sha256};
use sqlx::PgPool;
use time::OffsetDateTime;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;

use crate::AppState;

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
}

/// How often each replica looks for due jobs.
pub const TICK: Duration = Duration::from_secs(15);

/// The longest one run may take before it counts as failed, so one stuck
/// job never holds up the others.
pub const JOB_TIMEOUT: Duration = Duration::from_mins(10);

/// What a job does when it runs. It gets the shared state and returns a
/// short plain-English note for the log, or an error.
pub type JobFn = Arc<
    dyn Fn(AppState) -> Pin<Box<dyn Future<Output = anyhow::Result<String>> + Send>> + Send + Sync,
>;

/// One background job.
#[derive(Clone)]
pub struct Job {
    /// A stable name, such as `price_refresh`. It names the heartbeat row and
    /// the advisory lock.
    pub name: &'static str,
    /// How long after one start the next run is due.
    pub every: Duration,
    pub run: JobFn,
}

impl Job {
    /// A job from an async function.
    #[must_use]
    pub fn new<F, Fut>(name: &'static str, every: Duration, run: F) -> Self
    where
        F: Fn(AppState) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<String>> + Send + 'static,
    {
        Self {
            name,
            every,
            run: Arc::new(move |state| Box::pin(run(state))),
        }
    }
}

impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Job")
            .field("name", &self.name)
            .field("every", &self.every)
            .finish_non_exhaustive()
    }
}

/// A job's heartbeat, as the status endpoint shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct Heartbeat {
    pub job: String,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_started_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_finished_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_succeeded_at: Option<OffsetDateTime>,
    /// The last run's error, if it failed.
    pub last_error: Option<String>,
    pub runs: i64,
    pub failures_in_a_row: i32,
}

/// Every job's heartbeat, by name.
///
/// # Errors
///
/// Fails if the query fails.
pub async fn heartbeats(db: &PgPool) -> Result<Vec<Heartbeat>, sqlx::Error> {
    sqlx::query_as!(
        Heartbeat,
        "SELECT job, last_started_at, last_finished_at, last_succeeded_at, last_error,
                runs, failures_in_a_row
         FROM job_heartbeats ORDER BY job"
    )
    .fetch_all(db)
    .await
}

/// Asks for a run of `job` at the next tick, whatever its interval, such as
/// a ticket sync on demand.
///
/// # Errors
///
/// Fails if the query fails.
pub async fn request_now(db: &PgPool, job: &str) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO job_heartbeats (job, requested_at) VALUES ($1, now())
         ON CONFLICT (job) DO UPDATE SET requested_at = now()",
        job
    )
    .execute(db)
    .await?;
    Ok(())
}

/// The advisory lock key for a job: the first eight bytes of the SHA-256 of
/// its name, so every replica and every version agree on it.
#[must_use]
pub fn lock_key(job: &str) -> i64 {
    let digest = Sha256::digest(format!("tokencontroller.job.{job}").as_bytes());
    let mut first = [0_u8; 8];
    for (slot, byte) in first.iter_mut().zip(digest.iter()) {
        *slot = *byte;
    }
    i64::from_be_bytes(first)
}

/// Whether a job is due: never started, asked for, or its interval has
/// passed since the last start.
async fn is_due(connection: &mut sqlx::PgConnection, job: &Job) -> anyhow::Result<bool> {
    let every_ms = i64::try_from(job.every.as_millis()).context("job interval is too long")?;
    let due = sqlx::query_scalar!(
        r#"SELECT COALESCE((
             SELECT last_started_at IS NULL
                 OR COALESCE(requested_at >= last_started_at, false)
                 OR last_started_at <= now() - make_interval(secs => $2::bigint / 1000.0)
             FROM job_heartbeats WHERE job = $1
           ), true) AS "due!""#,
        job.name,
        every_ms
    )
    .fetch_one(&mut *connection)
    .await?;
    Ok(due)
}

/// What one attempt at a job did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attempt {
    /// Not due yet.
    NotDue,
    /// Another replica holds the lock.
    Locked,
    /// It ran; the note or the error is in the heartbeat.
    Ran { succeeded: bool },
}

/// Runs `job` once if it is due and no other replica runs it, and writes its
/// heartbeat. The lock is held on one connection for the whole run and
/// released when the run ends, or by Postgres if the connection dies.
///
/// # Errors
///
/// Fails only if the database fails; the job's own error goes into its
/// heartbeat.
pub async fn run_if_due(state: &AppState, job: &Job) -> anyhow::Result<Attempt> {
    let mut lock_connection = state.db.acquire().await?;
    if !is_due(&mut lock_connection, job).await? {
        return Ok(Attempt::NotDue);
    }
    let key = lock_key(job.name);
    let locked = sqlx::query_scalar!(r#"SELECT pg_try_advisory_lock($1) AS "locked!""#, key)
        .fetch_one(&mut *lock_connection)
        .await?;
    if !locked {
        return Ok(Attempt::Locked);
    }
    let result = run_locked(state, job, &mut lock_connection).await;
    let unlocked = sqlx::query_scalar!(r#"SELECT pg_advisory_unlock($1) AS "unlocked!""#, key)
        .fetch_one(&mut *lock_connection)
        .await;
    if !matches!(unlocked, Ok(true)) {
        // Closing the connection releases every lock it holds.
        let _closed = lock_connection.detach();
    }
    result
}

async fn run_locked(
    state: &AppState,
    job: &Job,
    lock_connection: &mut sqlx::PgConnection,
) -> anyhow::Result<Attempt> {
    // Another replica may have finished a run between our check and the lock.
    if !is_due(lock_connection, job).await? {
        return Ok(Attempt::NotDue);
    }
    sqlx::query!(
        "INSERT INTO job_heartbeats (job, last_started_at) VALUES ($1, now())
         ON CONFLICT (job) DO UPDATE SET last_started_at = now()",
        job.name
    )
    .execute(&mut *lock_connection)
    .await?;

    let outcome = match tokio::time::timeout(JOB_TIMEOUT, (job.run)(state.clone())).await {
        Ok(outcome) => outcome,
        Err(_elapsed) => Err(anyhow::anyhow!(
            "the run took longer than {} minutes and was stopped",
            JOB_TIMEOUT.as_secs().checked_div(60).unwrap_or_default()
        )),
    };
    let succeeded = outcome.is_ok();
    match &outcome {
        Ok(note) => tracing::info!(job = job.name, "{note}"),
        Err(error) => tracing::warn!(job = job.name, "job failed: {error:#}"),
    }
    let error = outcome.err().map(|error| format!("{error:#}"));
    sqlx::query!(
        "UPDATE job_heartbeats SET
             last_finished_at = now(),
             last_succeeded_at = CASE WHEN $2 THEN now() ELSE last_succeeded_at END,
             last_error = $3,
             runs = runs + 1,
             failures_in_a_row = CASE WHEN $2 THEN 0 ELSE failures_in_a_row + 1 END
         WHERE job = $1",
        job.name,
        succeeded,
        error
    )
    .execute(&mut *lock_connection)
    .await?;
    Ok(Attempt::Ran { succeeded })
}

/// Runs `jobs` in the background for as long as the service runs: one tick
/// every [`TICK`], each due job in turn.
#[must_use]
pub fn spawn(state: AppState, jobs: Vec<Job>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(TICK);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            for job in &jobs {
                if let Err(error) = run_if_due(&state, job).await {
                    tracing::warn!(job = job.name, "could not run the job: {error:#}");
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_keys_are_stable_and_distinct() {
        assert_eq!(lock_key("price_refresh"), lock_key("price_refresh"));
        assert_ne!(lock_key("price_refresh"), lock_key("ticket_sync"));
    }
}
