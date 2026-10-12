//! The Token Controller service (ADR 0002): one program that serves the API,
//! receives telemetry, runs the background jobs and serves the web app, with
//! all state in one Postgres database.
//!
//! The Enterprise plan build links this library and passes its own
//! [`Extension`] to [`serve_with`], with its own routes; the open build
//! passes `NoExtension` to [`serve`].
//!
//! Each area of the product has its own module folder, listed in [`api`].
//! See the layout table in the repository README for who owns which.

pub mod api;
pub mod change;
pub mod config;
pub mod error;
pub mod mail;
pub mod state;

pub mod billing;
pub mod bills;
pub mod budgets;
pub mod close;
pub mod customer_relations;
pub mod ingest;
pub mod integrations;
pub mod jobs;
pub mod mcp;
pub mod organizations;
pub mod projects;
pub mod reports;
pub mod seed;
pub mod sign_in;
pub mod status;
pub mod tags;
pub mod token_sheets;

use std::io::IsTerminal as _;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use sqlx::postgres::{PgPool, PgPoolOptions};
use tokencontroller_core::Extension;

pub use api::{openapi_json, router};
pub use config::Config;
pub use state::AppState;

/// The address the service listens on inside its container.
pub const LISTEN_ADDRESS: &str = "0.0.0.0:8080";

/// Every migration in `migrations/`, applied in order when the service starts
/// (ADR 0005).
pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Starts the service: reads the configuration, connects to Postgres,
/// applies migrations and serves HTTP until the process is asked to stop.
///
/// # Errors
///
/// Fails if the configuration is incomplete, Postgres cannot be reached, a
/// migration fails or the port is taken.
pub async fn serve(extension: Arc<dyn Extension>) -> anyhow::Result<()> {
    serve_with(extension, |_state| async { Ok(axum::Router::new()) }).await
}

/// Like [`serve`], and also serves the routes `extra_routes` returns. The
/// Enterprise plan build uses it to add its own endpoints (OIDC, SCIM) next
/// to the open ones; it gets the shared state after migrations ran, so it can
/// apply its own migrations to the same database first.
///
/// # Errors
///
/// As [`serve`], or whatever `extra_routes` returns.
pub async fn serve_with<F, Fut>(
    extension: Arc<dyn Extension>,
    extra_routes: F,
) -> anyhow::Result<()>
where
    F: FnOnce(AppState) -> Fut,
    Fut: Future<Output = anyhow::Result<axum::Router>>,
{
    init_tracing();
    let config = Config::from_env()?;
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        edition = extension.edition(),
        public_url = %config.public_url,
        "starting Token Controller"
    );

    let db = connect_and_migrate(&config.database_url).await?;

    let state = AppState::new(db, config, extension);
    customer_relations::start(state.clone());
    let extra = extra_routes(state.clone()).await?;
    let app = router(state).merge(extra);

    let listener = tokio::net::TcpListener::bind(LISTEN_ADDRESS)
        .await
        .with_context(|| format!("could not listen on {LISTEN_ADDRESS}"))?;
    tracing::info!(address = LISTEN_ADDRESS, "listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("the HTTP server stopped")?;
    tracing::info!("stopped");
    Ok(())
}

/// Loads the example organization into the database at `DATABASE_URL`,
/// after applying migrations, then returns. Staging runs this after every
/// reset; it can run more than once.
///
/// # Errors
///
/// Fails if `DATABASE_URL` is not set, Postgres cannot be reached, or a
/// migration or the seed data fails.
pub async fn seed() -> anyhow::Result<()> {
    init_tracing();
    let database_url = std::env::var("DATABASE_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
        .ok_or(config::ConfigError::Missing("DATABASE_URL"))?;
    let db = connect_and_migrate(&database_url).await?;
    seed::load(&db).await?;
    db.close().await;
    Ok(())
}

/// A sign-in link for a member, for instances without a mail server and for
/// screenshots on staging. Needs the same settings as the service.
///
/// # Errors
///
/// Fails if the configuration is incomplete, Postgres cannot be reached, or
/// the address belongs to no member.
pub async fn sign_in_link(email: &str) -> anyhow::Result<String> {
    // No log output: the link is the only thing on stdout, so scripts can
    // read it.
    let config = Config::from_env()?;
    let db = connect_and_migrate(&config.database_url).await?;
    let link = sign_in::link_for_member(&db, &config, email).await?;
    db.close().await;
    Ok(link)
}

async fn connect_and_migrate(database_url: &str) -> anyhow::Result<PgPool> {
    let db = connect(database_url).await?;
    MIGRATOR
        .run(&db)
        .await
        .context("could not apply database migrations")?;
    tracing::info!("database migrations applied");
    Ok(db)
}

/// Connects to Postgres, waiting up to a minute for it to come up, so the
/// service and the database can start together with Docker Compose.
async fn connect(database_url: &str) -> anyhow::Result<PgPool> {
    const ATTEMPTS: u32 = 30;
    const PAUSE: Duration = Duration::from_secs(2);

    let mut attempt: u32 = 1;
    loop {
        match PgPoolOptions::new()
            .max_connections(10)
            .acquire_timeout(Duration::from_secs(5))
            .connect(database_url)
            .await
        {
            Ok(pool) => return Ok(pool),
            Err(error) if attempt < ATTEMPTS => {
                tracing::warn!(attempt, %error, "Postgres is not reachable yet, trying again");
                tokio::time::sleep(PAUSE).await;
                attempt = attempt.saturating_add(1);
            }
            Err(error) => {
                return Err(error).context("could not connect to Postgres at DATABASE_URL");
            }
        }
    }
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    // A second call (in tests) keeps the first subscriber.
    let _already_set = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_ansi(std::io::stdout().is_terminal())
        .try_init();
}

async fn shutdown_signal() {
    let interrupt = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::warn!(%error, "could not listen for Ctrl-C");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::warn!(%error, "could not listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = interrupt => {},
        () = terminate => {},
    }
    tracing::info!("stopping");
}
