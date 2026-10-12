//! The customer record: one page per organization in the admin area, the
//! shared memory of everything customer relations does. It shows the plan,
//! the members, every support conversation and feedback report, newsletter
//! status, its first steps with the onboarding emails, and notes the
//! operators keep.
//!
//! Business contact data only (privacy policy: Customer relations).

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use super::admin::{self, ConversationSummary, Operator, Scope};
use super::feedback::{self, ReceivedReport};
use super::onboarding::{self, Journey};
use crate::AppState;
use crate::billing::{self, Plan};
use crate::error::{ApiError, ErrorBody};
use crate::organizations::Role;

/// An organization in the customer list.
#[derive(Debug, Serialize, ToSchema)]
pub struct CustomerSummary {
    pub id: Uuid,
    pub name: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    /// Solo or the Team plan; empty when billing is off (self-hosted).
    pub plan: Option<Plan>,
    /// Members who have not left, joined or invited.
    pub members: i64,
    /// Support conversations from its members that are not closed.
    pub open_support: i64,
    /// Feedback reports from its members nobody answered yet.
    pub open_feedback: i64,
}

/// The Team plan subscription, as Stripe last told us.
#[derive(Debug, Serialize, ToSchema)]
pub struct Subscription {
    /// Stripe's status, such as `active`, `past_due` or `canceled`.
    pub status: Option<String>,
    /// `month` or `year`.
    pub interval: Option<String>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub current_period_end: Option<OffsetDateTime>,
    pub cancel_at_period_end: bool,
}

/// A member, with their newsletter status.
#[derive(Debug, Serialize, ToSchema)]
pub struct CustomerMember {
    pub id: Uuid,
    pub name: String,
    pub email: String,
    pub role: Role,
    #[serde(with = "time::serde::rfc3339")]
    pub invited_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub joined_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339::option")]
    pub removed_at: Option<OffsetDateTime>,
    /// `confirmed`, `pending`, or empty when not subscribed.
    pub newsletter: Option<String>,
}

/// A note an operator kept about a customer.
#[derive(Debug, Serialize, ToSchema)]
pub struct Note {
    pub id: Uuid,
    pub author_email: String,
    pub body: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

/// Everything we know about one customer.
#[derive(Debug, Serialize, ToSchema)]
pub struct CustomerRecord {
    pub customer: CustomerSummary,
    pub slug: String,
    pub reporting_currency: String,
    /// Empty when the organization never started a checkout.
    pub subscription: Option<Subscription>,
    /// Everyone who was ever a member, the ones who left last.
    pub members: Vec<CustomerMember>,
    /// Conversations with its members' addresses, closed ones too.
    pub support: Vec<ConversationSummary>,
    pub feedback: Vec<ReceivedReport>,
    /// Its first steps and the onboarding emails it got.
    pub journey: Journey,
    /// Newest first.
    pub notes: Vec<Note>,
}

pub(super) async fn summaries(
    state: &AppState,
    only: Option<Uuid>,
) -> Result<Vec<CustomerSummary>, ApiError> {
    let rows = sqlx::query!(
        r#"SELECT o.id, o.name, o.created_at, b.status AS "status?",
             (SELECT count(*) FROM people p
              WHERE p.organization_id = o.id AND p.removed_at IS NULL) AS "members!",
             (SELECT count(*) FROM support_conversations c
              WHERE c.state <> 'closed' AND lower(c.contact_email) IN
                (SELECT p.email FROM people p WHERE p.organization_id = o.id)) AS "open_support!",
             (SELECT count(*) FROM feedback_reports f
              WHERE f.organization_id = o.id AND f.status = 'received') AS "open_feedback!"
           FROM organizations o
           LEFT JOIN billing_subscriptions b ON b.organization_id = o.id
           WHERE $1::uuid IS NULL OR o.id = $1
           ORDER BY o.created_at DESC, o.id"#,
        only
    )
    .fetch_all(&state.db)
    .await?;
    let billing_on = state.config.stripe.is_some();
    Ok(rows
        .into_iter()
        .map(|row| CustomerSummary {
            id: row.id,
            name: row.name,
            created_at: row.created_at,
            plan: billing_on.then(|| {
                if billing::is_team(row.status.as_deref()) {
                    Plan::Team
                } else {
                    Plan::Solo
                }
            }),
            members: row.members,
            open_support: row.open_support,
            open_feedback: row.open_feedback,
        })
        .collect())
}

/// Every organization on the instance, newest first.
#[utoipa::path(
    get,
    path = "/admin/customers",
    tag = "admin",
    responses(
        (status = OK, body = Vec<CustomerSummary>),
        (status = NOT_FOUND, description = "Not an operator", body = ErrorBody),
    ),
)]
pub async fn list_customers(
    State(state): State<AppState>,
    _operator: Operator,
) -> Result<Json<Vec<CustomerSummary>>, ApiError> {
    Ok(Json(summaries(&state, None).await?))
}

async fn notes(db: &PgPool, organization_id: Uuid) -> Result<Vec<Note>, sqlx::Error> {
    sqlx::query_as!(
        Note,
        "SELECT id, author_email, body, created_at FROM customer_notes
         WHERE organization_id = $1 ORDER BY created_at DESC, id",
        organization_id
    )
    .fetch_all(db)
    .await
}

/// The customer record of one organization.
///
/// # Errors
///
/// [`ApiError::NotFound`] for an unknown organization, or when the database
/// fails.
pub async fn record(state: &AppState, id: Uuid) -> Result<CustomerRecord, ApiError> {
    let customer = summaries(state, Some(id))
        .await?
        .into_iter()
        .next()
        .ok_or(ApiError::NotFound)?;
    let organization = sqlx::query!(
        "SELECT slug, reporting_currency FROM organizations WHERE id = $1",
        id
    )
    .fetch_one(&state.db)
    .await?;
    let subscription = sqlx::query_as!(
        Subscription,
        r#"SELECT status, billing_interval AS interval, current_period_end, cancel_at_period_end
           FROM billing_subscriptions WHERE organization_id = $1"#,
        id
    )
    .fetch_optional(&state.db)
    .await?;
    let members = sqlx::query!(
        r#"SELECT p.id, p.name, p.email, p.role, p.invited_at, p.joined_at, p.removed_at,
                  n.status AS "newsletter?"
           FROM people p
           LEFT JOIN newsletter_subscribers n ON lower(n.email) = p.email
           WHERE p.organization_id = $1
           ORDER BY p.removed_at IS NOT NULL, p.invited_at, p.id"#,
        id
    )
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(|row| {
        Ok(CustomerMember {
            id: row.id,
            name: row.name,
            email: row.email,
            role: row
                .role
                .parse()
                .map_err(|error| ApiError::Internal(anyhow::anyhow!("{error}")))?,
            invited_at: row.invited_at,
            joined_at: row.joined_at,
            removed_at: row.removed_at,
            newsletter: row.newsletter,
        })
    })
    .collect::<Result<Vec<_>, ApiError>>()?;
    let emails: Vec<String> = members.iter().map(|member| member.email.clone()).collect();
    let support = admin::conversations(&state.db, None, Some(&emails)).await?;
    Ok(CustomerRecord {
        customer,
        slug: organization.slug,
        reporting_currency: organization.reporting_currency,
        subscription,
        members,
        support,
        feedback: feedback::received(&state.db, Scope::Organization(id), None, None).await?,
        journey: onboarding::journey(&state.db, id)
            .await?
            .ok_or(ApiError::NotFound)?,
        notes: notes(&state.db, id).await?,
    })
}

/// Everything about one customer: plan, members, support, feedback,
/// newsletter and notes.
#[utoipa::path(
    get,
    path = "/admin/customers/{id}",
    tag = "admin",
    params(("id" = Uuid, Path)),
    responses(
        (status = OK, body = CustomerRecord),
        (status = NOT_FOUND, body = ErrorBody),
    ),
)]
pub async fn get_customer(
    State(state): State<AppState>,
    _operator: Operator,
    Path(id): Path<Uuid>,
) -> Result<Json<CustomerRecord>, ApiError> {
    Ok(Json(record(&state, id).await?))
}

/// A note to keep with a customer.
#[derive(Debug, Deserialize, ToSchema)]
pub struct NewNote {
    /// Such as what a call was about, or their answer to "What was hard
    /// this month?".
    pub text: String,
}

/// Keeps a note with a customer.
#[utoipa::path(
    post,
    path = "/admin/customers/{id}/notes",
    tag = "admin",
    params(("id" = Uuid, Path)),
    request_body = NewNote,
    responses(
        (status = CREATED, body = Note),
        (status = BAD_REQUEST, body = ErrorBody),
        (status = NOT_FOUND, body = ErrorBody),
    ),
)]
pub async fn add_note(
    State(state): State<AppState>,
    operator: Operator,
    Path(id): Path<Uuid>,
    Json(note): Json<NewNote>,
) -> Result<(StatusCode, Json<Note>), ApiError> {
    let body = admin::text(&note.text, "note")?;
    let note = sqlx::query_as!(
        Note,
        "INSERT INTO customer_notes (organization_id, author_email, body)
         SELECT id, $2, $3 FROM organizations WHERE id = $1
         RETURNING id, author_email, body, created_at",
        id,
        operator.email,
        body
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::NotFound)?;
    Ok((StatusCode::CREATED, Json(note)))
}
