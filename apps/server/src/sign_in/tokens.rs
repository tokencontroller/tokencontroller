//! API tokens (docs: Reference > HTTP API): service tokens for agents that
//! report token entries, read tokens for reports, and the organization token
//! for telemetry push. Created under Settings in the web app; the secret is
//! shown once. The list also shows the CLI tokens `tc connect` made, so they
//! can be revoked in the same place.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::ToSchema;
use uuid::Uuid;

use super::caller::Member;
use super::secret;
use crate::AppState;
use crate::error::ApiError;
use crate::organizations::{self, Role};

/// What a token is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TokenKind {
    /// `tc_svc_`: reports token entries. Its person is their agent manager.
    Service,
    /// `tc_read_`: reads reports; sees what its person's role sees.
    Read,
    /// `tc_org_`: telemetry push for the whole organization.
    Organization,
    /// `tc_cli_`: the CLI token of one person on one machine. Only
    /// `tc connect` makes them.
    Cli,
}

impl TokenKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Service => "service",
            Self::Read => "read",
            Self::Organization => "organization",
            Self::Cli => "cli",
        }
    }

    const fn prefix(self) -> &'static str {
        match self {
            Self::Service => secret::SERVICE,
            Self::Read => secret::READ,
            Self::Organization => secret::ORGANIZATION,
            Self::Cli => secret::CLI,
        }
    }

    fn from_db(kind: &str) -> Self {
        match kind {
            "service" => Self::Service,
            "read" => Self::Read,
            _ => Self::Organization,
        }
    }
}

/// An API token as lists show it, without its secret.
#[derive(Debug, Serialize, ToSchema)]
pub struct ApiToken {
    pub id: Uuid,
    pub kind: TokenKind,
    /// A label, such as `review bot`.
    pub name: String,
    /// The start of the token, such as `tc_svc_3f9a`.
    pub shown_prefix: String,
    /// The person it belongs to: for a service token, the agent manager of
    /// everything it reports.
    pub person_id: Uuid,
    /// The person left the organization. A service token in this state
    /// needs a new owner; its token entries show as an error until then.
    pub needs_owner: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_used_at: Option<OffsetDateTime>,
}

/// A new token.
#[derive(Debug, Deserialize, ToSchema)]
pub struct NewApiToken {
    pub kind: TokenKind,
    #[serde(default)]
    pub name: String,
}

/// A token just created: the only time its secret is shown.
#[derive(Debug, Serialize, ToSchema)]
pub struct CreatedApiToken {
    #[serde(flatten)]
    pub api_token: ApiToken,
    /// The token itself. Store it now; it is never shown again.
    pub token: String,
}

/// Move a service token to another person, who becomes the agent manager
/// of its future token entries.
#[derive(Debug, Deserialize, ToSchema)]
pub struct HandOver {
    pub person_id: Uuid,
}

/// Your API tokens and CLI tokens, or every one in the organization for a
/// controller.
#[utoipa::path(
    get,
    path = "/api-tokens",
    tag = "api-tokens",
    responses((status = OK, body = Vec<ApiToken>)),
)]
pub(super) async fn list(
    State(state): State<AppState>,
    member: Member,
) -> Result<Json<Vec<ApiToken>>, ApiError> {
    let everyone = member.role >= Role::Controller;
    let rows = sqlx::query!(
        r#"SELECT t.id, t.kind, t.name, t.shown_prefix, t.person_id, t.created_at, t.last_used_at,
                  p.removed_at IS NOT NULL AS "needs_owner!"
           FROM api_tokens t JOIN people p ON p.id = t.person_id
           WHERE t.organization_id = $1 AND t.revoked_at IS NULL AND ($2 OR t.person_id = $3)
           ORDER BY t.created_at"#,
        member.organization_id,
        everyone,
        member.person_id
    )
    .fetch_all(&state.db)
    .await?;
    let cli_tokens = sqlx::query!(
        r#"SELECT s.id, s.shown_prefix, s.person_id, s.created_at, s.last_seen_at
           FROM sessions s JOIN people p ON p.id = s.person_id
           WHERE p.organization_id = $1 AND s.client = 'local_tool' AND s.expires_at > now()
             AND ($2 OR s.person_id = $3)"#,
        member.organization_id,
        everyone,
        member.person_id
    )
    .fetch_all(&state.db)
    .await?;
    let mut tokens: Vec<ApiToken> = rows
        .into_iter()
        .map(|row| ApiToken {
            id: row.id,
            kind: TokenKind::from_db(&row.kind),
            name: row.name,
            shown_prefix: row.shown_prefix,
            person_id: row.person_id,
            needs_owner: row.needs_owner,
            created_at: row.created_at,
            last_used_at: row.last_used_at,
        })
        .chain(cli_tokens.into_iter().map(|row| ApiToken {
            id: row.id,
            kind: TokenKind::Cli,
            name: "tc connect".to_owned(),
            shown_prefix: row.shown_prefix.unwrap_or_else(|| secret::CLI.to_owned()),
            person_id: row.person_id,
            needs_owner: false,
            created_at: row.created_at,
            last_used_at: Some(row.last_seen_at),
        }))
        .collect();
    tokens.sort_by_key(|token| token.created_at);
    Ok(Json(tokens))
}

/// Create an API token. Anyone can create service and read tokens for
/// themselves; only a controller creates the organization token.
#[utoipa::path(
    post,
    path = "/api-tokens",
    tag = "api-tokens",
    request_body = NewApiToken,
    responses(
        (status = CREATED, body = CreatedApiToken),
        (status = FORBIDDEN, body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn create(
    State(state): State<AppState>,
    member: Member,
    Json(request): Json<NewApiToken>,
) -> Result<(StatusCode, Json<CreatedApiToken>), ApiError> {
    member.require_session()?;
    if request.kind == TokenKind::Cli {
        return Err(ApiError::BadRequest(
            "CLI tokens come from `tc connect`.".to_owned(),
        ));
    }
    if request.kind == TokenKind::Organization {
        member.require_role(Role::Controller)?;
    }
    let name = request.name.trim().to_owned();
    if name.chars().count() > 100 {
        return Err(ApiError::BadRequest(
            "The name is longer than 100 characters.".to_owned(),
        ));
    }
    let secret = secret::generate(request.kind.prefix(), &state.config.secret_key)
        .map_err(|error| ApiError::Internal(anyhow::anyhow!("no randomness: {error}")))?;
    let row = sqlx::query!(
        r#"INSERT INTO api_tokens (organization_id, kind, name, person_id, token_hash, shown_prefix)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING id, created_at"#,
        member.organization_id,
        request.kind.as_str(),
        name,
        member.person_id,
        secret.hash,
        secret.shown_prefix()
    )
    .fetch_one(&state.db)
    .await?;
    organizations::record_audit_event(
        &state,
        member.organization_id,
        Some(member.person_id),
        "api_token.created",
        Some(row.id),
    );
    Ok((
        StatusCode::CREATED,
        Json(CreatedApiToken {
            api_token: ApiToken {
                id: row.id,
                kind: request.kind,
                name,
                shown_prefix: secret.shown_prefix(),
                person_id: member.person_id,
                needs_owner: false,
                created_at: row.created_at,
                last_used_at: None,
            },
            token: secret.token,
        }),
    ))
}

/// Revoke an API token or a CLI token: your own, or any as a controller.
#[utoipa::path(
    delete,
    path = "/api-tokens/{id}",
    tag = "api-tokens",
    params(("id" = Uuid, Path)),
    responses(
        (status = NO_CONTENT, description = "Revoked"),
        (status = NOT_FOUND, body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn revoke(
    State(state): State<AppState>,
    member: Member,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    member.require_session()?;
    let everyone = member.role >= Role::Controller;
    let revoked = sqlx::query!(
        r#"UPDATE api_tokens SET revoked_at = now()
           WHERE id = $1 AND organization_id = $2 AND revoked_at IS NULL AND ($3 OR person_id = $4)"#,
        id,
        member.organization_id,
        everyone,
        member.person_id
    )
    .execute(&state.db)
    .await?;
    if revoked.rows_affected() == 0 {
        // A CLI token: ending its session signs that machine out.
        let ended = sqlx::query!(
            r#"DELETE FROM sessions s USING people p
               WHERE s.id = $1 AND s.client = 'local_tool' AND p.id = s.person_id
                 AND p.organization_id = $2 AND ($3 OR s.person_id = $4)"#,
            id,
            member.organization_id,
            everyone,
            member.person_id
        )
        .execute(&state.db)
        .await?;
        if ended.rows_affected() == 0 {
            return Err(ApiError::NotFound);
        }
    }
    organizations::record_audit_event(
        &state,
        member.organization_id,
        Some(member.person_id),
        "api_token.revoked",
        Some(id),
    );
    Ok(StatusCode::NO_CONTENT)
}

/// Hand a service token over to another member, for example when its
/// creator leaves. Only future token entries follow. Its owner or a
/// controller can do this.
#[utoipa::path(
    post,
    path = "/api-tokens/{id}/hand-over",
    tag = "api-tokens",
    params(("id" = Uuid, Path)),
    request_body = HandOver,
    responses(
        (status = NO_CONTENT, description = "Handed over"),
        (status = BAD_REQUEST, body = crate::error::ErrorBody),
        (status = NOT_FOUND, body = crate::error::ErrorBody),
    ),
)]
pub(super) async fn hand_over(
    State(state): State<AppState>,
    member: Member,
    Path(id): Path<Uuid>,
    Json(request): Json<HandOver>,
) -> Result<StatusCode, ApiError> {
    member.require_session()?;
    let everyone = member.role >= Role::Controller;
    let target_active = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM people
           WHERE id = $1 AND organization_id = $2 AND removed_at IS NULL) AS "exists!""#,
        request.person_id,
        member.organization_id
    )
    .fetch_one(&state.db)
    .await?;
    if !target_active {
        return Err(ApiError::BadRequest(
            "The new owner must be a member of the organization.".to_owned(),
        ));
    }
    let moved = sqlx::query!(
        r#"UPDATE api_tokens SET person_id = $5
           WHERE id = $1 AND organization_id = $2 AND kind = 'service' AND revoked_at IS NULL
             AND ($3 OR person_id = $4)"#,
        id,
        member.organization_id,
        everyone,
        member.person_id,
        request.person_id
    )
    .execute(&state.db)
    .await?;
    if moved.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    organizations::record_audit_event(
        &state,
        member.organization_id,
        Some(member.person_id),
        "api_token.handed_over",
        Some(id),
    );
    Ok(StatusCode::NO_CONTENT)
}
