//! Team plan subscriptions through Stripe for the hosted service. Off unless
//! the Stripe settings are set, so self-hosted instances never see it
//! (ADR 0002): every endpoint here answers 404, and the member limit is the
//! one the docs give for self-hosting.
//!
//! An organization without an active subscription is on the free Solo plan:
//! one person. The Team plan allows up to 150 members (docs: Plans and
//! limits). Plan switches, cancelling and card changes happen in Stripe's
//! customer portal. The Enterprise plan is invoiced by hand and needs no code.
//!
//! Owner: the Stripe billing work. Keep this area's code, routes and queries in
//! this folder, and its migrations in `migrations/` named
//! `<timestamp>_billing_<what>.sql`.

mod stripe;
mod webhook;

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use uuid::Uuid;

use crate::AppState;
use crate::config::StripeConfig;
use crate::error::ApiError;
use crate::organizations::{self, Role};
use crate::sign_in::Member;

/// The Team plan's member limit (docs: Plans and limits).
pub const TEAM_PLAN_MEMBERS: u32 = 150;
/// The Solo plan is for one person.
pub const SOLO_MEMBERS: u32 = 1;

/// This area's routes, relative to `/api/v1`.
#[must_use]
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(get_billing))
        .routes(routes!(checkout))
        .routes(routes!(portal))
        .routes(routes!(webhook::receive))
}

/// How many members an organization may have, and what to say when it is
/// reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemberLimit {
    pub max: i64,
    pub message: &'static str,
}

/// The member limit of an organization: Solo or Team plan on the hosted
/// service, otherwise what the extension says or 150.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn member_limit(
    state: &AppState,
    organization_id: Uuid,
) -> Result<MemberLimit, ApiError> {
    if state.config.stripe.is_none() {
        let max = state.extension.member_limit().unwrap_or(TEAM_PLAN_MEMBERS);
        return Ok(MemberLimit {
            max: i64::from(max),
            message: "This instance allows no more members.",
        });
    }
    Ok(match plan(state, organization_id).await? {
        Plan::Team => MemberLimit {
            max: i64::from(TEAM_PLAN_MEMBERS),
            message: "The Team plan stops at 150 members. More members come with the Enterprise plan.",
        },
        Plan::Solo => MemberLimit {
            max: i64::from(SOLO_MEMBERS),
            message: "The Solo plan is for one person. Choose the Team plan under Billing to invite people.",
        },
    })
}

/// The hosted plans billing knows about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Plan {
    /// Free forever for one person. No card.
    Solo,
    /// Paid by card, up to 150 members.
    Team,
}

/// Stripe statuses in which the Team plan is in force. `past_due` keeps it
/// while Stripe retries the card.
#[must_use]
pub fn is_team(status: Option<&str>) -> bool {
    matches!(status, Some("active" | "past_due"))
}

async fn plan(state: &AppState, organization_id: Uuid) -> Result<Plan, ApiError> {
    let status = sqlx::query_scalar!(
        "SELECT status FROM billing_subscriptions WHERE organization_id = $1",
        organization_id
    )
    .fetch_optional(&state.db)
    .await?
    .flatten();
    Ok(if is_team(status.as_deref()) {
        Plan::Team
    } else {
        Plan::Solo
    })
}

fn stripe_config(state: &AppState) -> Result<&StripeConfig, ApiError> {
    state.config.stripe.as_ref().ok_or(ApiError::NotFound)
}

/// Monthly or yearly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Interval {
    Month,
    Year,
}

impl Interval {
    /// The Stripe price's lookup key.
    const fn lookup_key(self) -> &'static str {
        match self {
            Self::Month => "team_monthly",
            Self::Year => "team_yearly",
        }
    }
}

/// The organization's plan.
#[derive(Debug, Serialize, ToSchema)]
pub struct Billing {
    pub plan: Plan,
    /// Stripe's subscription status, such as `active` or `past_due`.
    pub status: Option<String>,
    pub interval: Option<Interval>,
    /// When the current period ends: the next charge, or the end of the
    /// Team plan if it was cancelled.
    #[serde(with = "time::serde::rfc3339::option")]
    pub current_period_end: Option<OffsetDateTime>,
    pub cancel_at_period_end: bool,
    pub members: i64,
    pub member_limit: i64,
}

/// The organization's plan and members. Only on the hosted service.
#[utoipa::path(
    get,
    path = "/billing",
    tag = "billing",
    responses(
        (status = OK, body = Billing),
        (status = NOT_FOUND, description = "Billing is off on this instance", body = crate::error::ErrorBody),
    ),
)]
async fn get_billing(
    State(state): State<AppState>,
    member: Member,
) -> Result<Json<Billing>, ApiError> {
    stripe_config(&state)?;
    let row = sqlx::query!(
        r#"SELECT status, billing_interval, current_period_end, cancel_at_period_end
           FROM billing_subscriptions WHERE organization_id = $1"#,
        member.organization_id
    )
    .fetch_optional(&state.db)
    .await?;
    let limit = member_limit(&state, member.organization_id).await?;
    let members = organizations::member_count(&state.db, member.organization_id).await?;
    let billing = match row {
        Some(row) => Billing {
            plan: if is_team(row.status.as_deref()) {
                Plan::Team
            } else {
                Plan::Solo
            },
            status: row.status,
            interval: match row.billing_interval.as_deref() {
                Some("month") => Some(Interval::Month),
                Some("year") => Some(Interval::Year),
                _ => None,
            },
            current_period_end: row.current_period_end,
            cancel_at_period_end: row.cancel_at_period_end,
            members,
            member_limit: limit.max,
        },
        None => Billing {
            plan: Plan::Solo,
            status: None,
            interval: None,
            current_period_end: None,
            cancel_at_period_end: false,
            members,
            member_limit: limit.max,
        },
    };
    Ok(Json(billing))
}

/// Start paying for the Team plan.
#[derive(Debug, Deserialize, ToSchema)]
pub struct CheckoutRequest {
    pub interval: Interval,
    /// The billing country as an ISO 3166 code, such as `AT`. Customers in
    /// the EU pay in EUR, everyone else in USD.
    pub country: String,
}

/// A Stripe page to send the controller to.
#[derive(Debug, Serialize, ToSchema)]
pub struct StripeRedirect {
    pub url: String,
}

/// Open Stripe Checkout for the Team plan. Controllers only. No trial: the
/// card is charged from day one.
#[utoipa::path(
    post,
    path = "/billing/checkout",
    tag = "billing",
    request_body = CheckoutRequest,
    responses(
        (status = OK, body = StripeRedirect),
        (status = CONFLICT, description = "Already on the Team plan", body = crate::error::ErrorBody),
        (status = NOT_FOUND, description = "Billing is off on this instance", body = crate::error::ErrorBody),
    ),
)]
async fn checkout(
    State(state): State<AppState>,
    member: Member,
    Json(request): Json<CheckoutRequest>,
) -> Result<Json<StripeRedirect>, ApiError> {
    let stripe = stripe_config(&state)?;
    member.require_session()?;
    member.require_role(Role::Controller)?;
    if plan(&state, member.organization_id).await? == Plan::Team {
        return Err(ApiError::Conflict(
            "The organization is on the Team plan already. Change the plan under Manage billing."
                .to_owned(),
        ));
    }
    let country = request.country.trim().to_uppercase();
    if country.len() != 2 || !country.chars().all(|c| c.is_ascii_uppercase()) {
        return Err(ApiError::BadRequest(format!(
            "{country:?} is not a country code; use two letters such as AT or US."
        )));
    }
    let customer = sqlx::query_scalar!(
        "SELECT stripe_customer_id FROM billing_subscriptions WHERE organization_id = $1",
        member.organization_id
    )
    .fetch_optional(&state.db)
    .await?;
    let email = sqlx::query_scalar!("SELECT email FROM people WHERE id = $1", member.person_id)
        .fetch_one(&state.db)
        .await?;
    let url = stripe::checkout_session(
        stripe,
        &stripe::CheckoutParams {
            public_url: &state.config.public_url,
            organization_id: member.organization_id,
            lookup_key: request.interval.lookup_key(),
            eur: stripe::is_eu(&country),
            customer: customer.as_deref(),
            email: &email,
        },
    )
    .await?;
    Ok(Json(StripeRedirect { url }))
}

/// Open Stripe's customer portal: switch between monthly and yearly,
/// cancel, change the card or the invoice address, see invoices.
/// Controllers only.
#[utoipa::path(
    post,
    path = "/billing/portal",
    tag = "billing",
    responses(
        (status = OK, body = StripeRedirect),
        (status = CONFLICT, description = "The organization never paid for the Team plan", body = crate::error::ErrorBody),
        (status = NOT_FOUND, description = "Billing is off on this instance", body = crate::error::ErrorBody),
    ),
)]
async fn portal(
    State(state): State<AppState>,
    member: Member,
) -> Result<Json<StripeRedirect>, ApiError> {
    let stripe = stripe_config(&state)?;
    member.require_session()?;
    member.require_role(Role::Controller)?;
    let customer = sqlx::query_scalar!(
        "SELECT stripe_customer_id FROM billing_subscriptions WHERE organization_id = $1",
        member.organization_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| {
        ApiError::Conflict(
            "The organization is on the Solo plan; there is nothing to manage yet.".to_owned(),
        )
    })?;
    let url = stripe::portal_session(stripe, &state.config.public_url, &customer).await?;
    Ok(Json(StripeRedirect { url }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_team_plan_holds_while_stripe_retries_the_card() {
        assert!(is_team(Some("active")));
        assert!(is_team(Some("past_due")));
        for status in [
            "canceled",
            "unpaid",
            "incomplete",
            "incomplete_expired",
            "paused",
            "trialing",
        ] {
            assert!(!is_team(Some(status)), "{status}");
        }
        assert!(!is_team(None));
    }

    #[test]
    fn prices_are_found_by_lookup_key() {
        assert_eq!(Interval::Month.lookup_key(), "team_monthly");
        assert_eq!(Interval::Year.lookup_key(), "team_yearly");
    }
}
