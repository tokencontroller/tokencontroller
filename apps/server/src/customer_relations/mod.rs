//! Customer relations: the support inbox, the site assistant, the newsletter,
//! feedback reports and the operators' admin area.
//!
//! Owner: the customer relations work. Keep this area's code, routes and queries in
//! this folder, and its migrations in `migrations/` named
//! `<timestamp>_customer_relations_<what>.sql`.
//!
//! Everything here is off until its settings are set (see [`Settings`]), so
//! a self-hosted instance runs none of it:
//!
//! - The support inbox takes mail from the mail worker (`/inbound/mail`),
//!   the website's contact form (`/contact`) and members in the app
//!   (`/support/messages`). The support answerer answers
//!   from the docs within minutes and hands what the docs cannot answer to
//!   the operators, with a proposed answer.
//! - The site assistant (`/assistant/questions`) answers website visitors
//!   from the docs.
//! - The newsletter (`/newsletter/...`) has double opt-in and one-click
//!   unsubscribe, and goes out through Resend.
//! - Feedback reports (`/feedback`) come from the web app's feedback button
//!   and are on whenever the service runs.
//! - The admin area (`/admin/...`) is for the operators (`TC_OPERATOR_EMAILS`):
//!   support, the assistant's questions, the newsletter, feedback reports and
//!   one customer record per organization.
//! - Onboarding emails (`TC_ONBOARDING_EMAILS`) go to a new organization's
//!   controllers when it stalls at one of its first steps. The job runner
//!   sends them (`onboarding-emails`), not this loop.
//!
//! The background loop also deletes what is kept long enough (`retention`).

mod admin;
mod assistant;
mod claude;
mod customers;
mod feedback;
mod inbox;
mod mail;
mod newsletter;
mod onboarding;
mod retention;
mod settings;
mod support;
mod tools;
mod web;

use std::sync::OnceLock;
use std::time::Duration;

use axum::extract::DefaultBodyLimit;
use axum::routing::options;
use tokio::sync::Notify;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

pub use admin::is_operator;
pub use onboarding::{
    JOB as ONBOARDING_JOB, JOB_EVERY as ONBOARDING_JOB_EVERY, job as onboarding_job,
    send_due as send_onboarding_emails,
};
pub use settings::{Settings, SettingsError};
pub use tools::{call_operator_tool, operator_tools};

use crate::hosted::Hosted;

/// How often the background loop runs when nothing wakes it.
const EVERY: Duration = Duration::from_mins(1);

/// The Postgres advisory lock that keeps the loop to one replica at a time
/// (ADR 0002), so no mail goes out twice.
const LOCK: i64 = 0x7463_6372;

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<Hosted> {
    OpenApiRouter::new()
        .routes(routes!(inbox::receive_mail))
        .layer(DefaultBodyLimit::max(inbox::MAX_MAIL_BYTES))
        .routes(routes!(inbox::contact))
        .route("/contact", options(inbox::contact_preflight))
        .routes(routes!(assistant::ask))
        .route("/assistant/questions", options(assistant::ask_preflight))
        .routes(routes!(newsletter::sign_up))
        .route(
            "/newsletter/subscribers",
            options(newsletter::sign_up_preflight),
        )
        .routes(routes!(newsletter::confirm))
        .route(
            "/newsletter/confirm",
            options(newsletter::confirm_preflight),
        )
        .routes(routes!(newsletter::unsubscribe))
        .route(
            "/newsletter/unsubscribe",
            options(newsletter::unsubscribe_preflight),
        )
        .merge(
            OpenApiRouter::new()
                .routes(routes!(feedback::send_report))
                .layer(DefaultBodyLimit::max(feedback::MAX_REQUEST_BYTES)),
        )
        .routes(routes!(feedback::my_reports))
        .routes(routes!(inbox::support))
        .routes(routes!(inbox::support_message))
        .routes(routes!(feedback::screenshot))
        .routes(routes!(admin::overview))
        .routes(routes!(admin::list_support))
        .routes(routes!(admin::get_support))
        .routes(routes!(admin::reply))
        .routes(routes!(admin::close))
        .routes(routes!(admin::list_questions))
        .routes(routes!(admin::newsletter))
        .routes(routes!(admin::create_issue))
        .routes(routes!(admin::update_issue))
        .routes(routes!(admin::send_issue))
        .routes(routes!(feedback::list_received))
        .routes(routes!(feedback::get_received))
        .routes(routes!(feedback::update_received))
        .routes(routes!(customers::list_customers))
        .routes(routes!(customers::get_customer))
        .routes(routes!(customers::add_note))
}

fn waker() -> &'static Notify {
    static WAKER: OnceLock<Notify> = OnceLock::new();
    WAKER.get_or_init(Notify::new)
}

/// Asks the background loop to run now, such as after new mail arrived.
pub fn wake() {
    waker().notify_one();
}

/// Starts the background loop: answering support, sending replies, notices
/// and the newsletter. It runs every minute and whenever [`wake`] is called,
/// and does nothing while every feature is off.
pub fn start(state: Hosted) {
    tokio::spawn(async move {
        loop {
            run_once(&state).await;
            let _woken_or_timed_out = tokio::time::timeout(EVERY, waker().notified()).await;
        }
    });
}

/// One run of the background loop. Failures are logged and tried again on
/// the next run. When another replica is running it, this run does nothing.
pub async fn run_once(state: &Hosted) {
    let Ok(mut connection) = state.db.acquire().await else {
        tracing::warn!("customer relations: no database connection, skipping this run");
        return;
    };
    let locked = sqlx::query_scalar!(r#"SELECT pg_try_advisory_lock($1) AS "locked!""#, LOCK)
        .fetch_one(&mut *connection)
        .await;
    if !matches!(locked, Ok(true)) {
        return;
    }
    run_locked(state).await;
    if let Err(error) = sqlx::query_scalar!(r#"SELECT pg_advisory_unlock($1) AS "unlocked!""#, LOCK)
        .fetch_one(&mut *connection)
        .await
    {
        tracing::warn!(%error, "customer relations: could not release the lock");
    }
}

async fn run_locked(state: &Hosted) {
    if let Err(error) = retention::forget(&state.db).await {
        tracing::error!("deleting old support and assistant data failed: {error:#}");
    }
    if let Err(error) = feedback::notify(state).await {
        tracing::error!("sending feedback notices failed: {error:#}");
    }
    let settings = &state.settings.customer_relations;
    if settings.support_from.is_some() {
        // A handful per run, so one run never takes long.
        for _ in 0..10 {
            match support::answer_next(state).await {
                Ok(true) => {}
                Ok(false) => break,
                Err(error) => {
                    tracing::error!("support answering failed: {error:#}");
                    break;
                }
            }
        }
        if let Err(error) = support::send_replies(state).await {
            tracing::error!("sending support replies failed: {error:#}");
        }
        if let Err(error) = support::notify_operators(state).await {
            tracing::error!("telling the operators failed: {error:#}");
        }
    }
    if settings.resend_api_key.is_some() {
        if let Err(error) = newsletter::send_confirmations(state).await {
            tracing::error!("sending newsletter confirmations failed: {error:#}");
        }
        if let Err(error) = newsletter::send_issues(state).await {
            tracing::error!("sending the newsletter failed: {error:#}");
        }
    }
}
