//! The job runner against a real Postgres database: advisory locks,
//! heartbeats and runs on demand (ADR 0002).

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use sqlx::PgPool;
use tokencontroller_core::NoExtension;
use tokencontroller_server::jobs::{self, Attempt, Job};
use tokencontroller_server::{AppState, Config};

fn state(pool: PgPool) -> AppState {
    let config = Config {
        database_url: "postgres://unused".to_owned(),
        public_url: "http://localhost:8080".to_owned(),
        secret_key: "0123456789abcdef0123456789abcdef".to_owned(),
        smtp_url: None,
        smtp_from: None,
        prices_refresh: false,
        customer_relations: tokencontroller_server::customer_relations::Settings::default(),
        stripe: None,
        embeddings: None,
    };
    AppState::new(pool, config, Arc::new(NoExtension))
}

fn counting_job(name: &'static str, runs: Arc<AtomicU32>, fail: bool) -> Job {
    Job::new(name, Duration::from_hours(1), move |_state| {
        let runs = Arc::clone(&runs);
        async move {
            runs.fetch_add(1, Ordering::SeqCst);
            if fail {
                Err(anyhow::anyhow!("the provider said no"))
            } else {
                Ok("done".to_owned())
            }
        }
    })
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_job_runs_once_per_interval_and_writes_its_heartbeat(pool: PgPool) {
    let state = state(pool.clone());
    let runs = Arc::new(AtomicU32::new(0));
    let job = counting_job("example", Arc::clone(&runs), false);

    assert_eq!(
        jobs::run_if_due(&state, &job).await.unwrap(),
        Attempt::Ran { succeeded: true }
    );
    assert_eq!(
        jobs::run_if_due(&state, &job).await.unwrap(),
        Attempt::NotDue
    );
    assert_eq!(runs.load(Ordering::SeqCst), 1);

    let beats = jobs::runs(&pool).await.unwrap();
    assert_eq!(beats.len(), 1);
    assert_eq!(beats[0].job, "example");
    assert_eq!(beats[0].runs, 1);
    assert_eq!(beats[0].failures_in_a_row, 0);
    assert!(beats[0].last_succeeded_at.is_some());
    assert_eq!(beats[0].last_error, None);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_run_on_demand_is_due_at_once(pool: PgPool) {
    let state = state(pool.clone());
    let runs = Arc::new(AtomicU32::new(0));
    let job = counting_job("on-demand", Arc::clone(&runs), false);

    jobs::run_if_due(&state, &job).await.unwrap();
    jobs::request_now(&pool, "on-demand").await.unwrap();
    assert_eq!(
        jobs::run_if_due(&state, &job).await.unwrap(),
        Attempt::Ran { succeeded: true }
    );
    assert_eq!(
        jobs::run_if_due(&state, &job).await.unwrap(),
        Attempt::NotDue
    );
    assert_eq!(runs.load(Ordering::SeqCst), 2);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_job_another_replica_holds_is_skipped(pool: PgPool) {
    let state = state(pool.clone());
    let runs = Arc::new(AtomicU32::new(0));
    let job = counting_job("locked", Arc::clone(&runs), false);

    let mut other_replica = pool.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(jobs::lock_key("locked"))
        .execute(&mut *other_replica)
        .await
        .unwrap();
    assert_eq!(
        jobs::run_if_due(&state, &job).await.unwrap(),
        Attempt::Locked
    );
    assert_eq!(runs.load(Ordering::SeqCst), 0);

    sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(jobs::lock_key("locked"))
        .execute(&mut *other_replica)
        .await
        .unwrap();
    assert_eq!(
        jobs::run_if_due(&state, &job).await.unwrap(),
        Attempt::Ran { succeeded: true }
    );
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_failed_run_keeps_its_error_until_the_next_success(pool: PgPool) {
    let state = state(pool.clone());
    let runs = Arc::new(AtomicU32::new(0));
    let failing = counting_job("flaky", Arc::clone(&runs), true);

    assert_eq!(
        jobs::run_if_due(&state, &failing).await.unwrap(),
        Attempt::Ran { succeeded: false }
    );
    jobs::request_now(&pool, "flaky").await.unwrap();
    jobs::run_if_due(&state, &failing).await.unwrap();
    let beat = &jobs::runs(&pool).await.unwrap()[0];
    assert_eq!(beat.failures_in_a_row, 2);
    assert_eq!(beat.last_error.as_deref(), Some("the provider said no"));
    assert_eq!(beat.last_succeeded_at, None);

    let working = counting_job("flaky", Arc::clone(&runs), false);
    jobs::request_now(&pool, "flaky").await.unwrap();
    jobs::run_if_due(&state, &working).await.unwrap();
    let beat = &jobs::runs(&pool).await.unwrap()[0];
    assert_eq!(beat.failures_in_a_row, 0);
    assert_eq!(beat.last_error, None);
    assert_eq!(beat.runs, 3);
}

#[sqlx::test(migrator = "tokencontroller_server::MIGRATOR")]
async fn a_job_is_due_again_once_its_interval_has_passed(pool: PgPool) {
    let state = state(pool.clone());
    let runs = Arc::new(AtomicU32::new(0));
    let job = counting_job("hourly", Arc::clone(&runs), false);

    jobs::run_if_due(&state, &job).await.unwrap();
    sqlx::query(
        "UPDATE job_runs SET last_started_at = now() - interval '59 minutes' WHERE job = 'hourly'",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        jobs::run_if_due(&state, &job).await.unwrap(),
        Attempt::NotDue
    );
    sqlx::query(
        "UPDATE job_runs SET last_started_at = now() - interval '61 minutes' WHERE job = 'hourly'",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        jobs::run_if_due(&state, &job).await.unwrap(),
        Attempt::Ran { succeeded: true }
    );
    assert_eq!(runs.load(Ordering::SeqCst), 2);
}
