//! Who is calling: the extractors every handler uses.
//!
//! - [`Member`] is what most endpoints take: a signed-in person (browser
//!   cookie or a `tc_cli_` token from `tc connect`), or a `tc_read_` token on a
//!   read (GET). It answers 401 to anything else.
//! - [`Caller`] also takes service tokens (`tc_svc_`) and the organization
//!   token (`tc_org_`). A handler that takes it says which credentials it
//!   accepts with [`Caller::require_credential`].
//!
//! Service tokens and the organization token keep working after the person
//! who created them leaves: the token entries then show as an error until
//! someone takes the token over (docs: Account > Roles).

use axum::extract::{FromRef, FromRequestParts};
use axum::http::header::{AUTHORIZATION, COOKIE, ORIGIN};
use axum::http::request::Parts;
use axum::http::{HeaderMap, Method};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::secret;
use crate::AppState;
use crate::error::ApiError;
use crate::organizations::Role;

/// The name of the browser session cookie.
pub const SESSION_COOKIE: &str = "tc_session";

/// How the caller proved who they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Credential {
    /// The browser's session cookie.
    BrowserSession,
    /// A `tc_cli_` token from `tc connect`, held by a local tool such as
    /// the CLI.
    LocalToolSession,
    Service {
        token_id: Uuid,
    },
    Read {
        token_id: Uuid,
    },
    Organization {
        token_id: Uuid,
    },
}

/// The kinds of [`Credential`], for [`Caller::require_credential`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    /// A signed-in person, in a browser or a local tool.
    Session,
    Service,
    Read,
    Organization,
}

impl Credential {
    #[must_use]
    pub const fn kind(self) -> CredentialKind {
        match self {
            Self::BrowserSession | Self::LocalToolSession => CredentialKind::Session,
            Self::Service { .. } => CredentialKind::Service,
            Self::Read { .. } => CredentialKind::Read,
            Self::Organization { .. } => CredentialKind::Organization,
        }
    }
}

/// Any authenticated caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    pub organization_id: Uuid,
    /// The signed-in person, or the person a token belongs to: for a service
    /// token, the agent manager of what it reports.
    pub person_id: Uuid,
    /// That person's role. A read token sees what this role sees.
    pub role: Role,
    /// Whether that person has left the organization. Only service and
    /// organization tokens of people who left still work.
    pub person_removed: bool,
    pub credential: Credential,
}

impl Caller {
    /// Refuses callers whose role is below `role`.
    ///
    /// # Errors
    ///
    /// [`ApiError::Forbidden`] when the role is too low.
    pub fn require_role(&self, role: Role) -> Result<(), ApiError> {
        if self.role >= role {
            Ok(())
        } else {
            Err(ApiError::Forbidden)
        }
    }

    /// Refuses callers whose credential is not one of `kinds`.
    ///
    /// # Errors
    ///
    /// [`ApiError::Forbidden`] for any other credential.
    pub fn require_credential(&self, kinds: &[CredentialKind]) -> Result<(), ApiError> {
        if kinds.contains(&self.credential.kind()) {
            Ok(())
        } else {
            Err(ApiError::Forbidden)
        }
    }

    /// Refuses everything but a person signed in to the web app: changes to
    /// the organization are made by people in the browser, not by tokens,
    /// and not by a CLI token either, which may only submit its person's
    /// token sheet and edit their rules.
    ///
    /// # Errors
    ///
    /// [`ApiError::Forbidden`] for tokens.
    pub fn require_session(&self) -> Result<(), ApiError> {
        if self.credential == Credential::BrowserSession {
            Ok(())
        } else {
            Err(ApiError::Forbidden)
        }
    }
}

/// A signed-in person, or a read token on a read. See the module docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member(pub Caller);

impl std::ops::Deref for Member {
    type Target = Caller;

    fn deref(&self) -> &Caller {
        &self.0
    }
}

/// Works with any router state the service's [`AppState`] can be taken
/// from, so a build's own areas with their own state can use it.
impl<S> FromRequestParts<S> for Caller
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        let state = &AppState::from_ref(state);
        let caller = authenticate(&parts.headers, state).await?;
        if caller.credential == Credential::BrowserSession
            && !is_safe(&parts.method)
            && !same_origin(&parts.headers, &state.config.public_url)
        {
            return Err(ApiError::Forbidden);
        }
        Ok(caller)
    }
}

impl<S> FromRequestParts<S> for Member
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        let caller = Caller::from_request_parts(parts, state).await?;
        let allowed = match caller.credential.kind() {
            CredentialKind::Session => true,
            CredentialKind::Read => is_safe(&parts.method),
            CredentialKind::Service | CredentialKind::Organization => false,
        };
        if allowed && !caller.person_removed {
            Ok(Self(caller))
        } else {
            Err(ApiError::Unauthorized)
        }
    }
}

/// The person a request is signed in as, for code outside the handlers such
/// as the Enterprise plan's settings: a browser or local tool session of a
/// member who was not removed. `None` without one, for an API token, and
/// for a browser request from another site.
///
/// # Errors
///
/// Fails if a query fails.
pub async fn signed_in(state: &AppState, headers: &HeaderMap) -> Result<Option<Caller>, ApiError> {
    if !same_origin(headers, &state.config.public_url) {
        return Ok(None);
    }
    match authenticate(headers, state).await {
        Ok(caller) if caller.credential.kind() == CredentialKind::Session => Ok(Some(caller)),
        Ok(_) | Err(ApiError::Unauthorized) => Ok(None),
        Err(error) => Err(error),
    }
}

fn is_safe(method: &Method) -> bool {
    method == Method::GET || method == Method::HEAD
}

/// A browser sends `Origin` on cross-site requests. Requests without it come
/// from the same site or from a tool, so only a different origin is refused.
fn same_origin(headers: &HeaderMap, public_url: &str) -> bool {
    headers
        .get(ORIGIN)
        .and_then(|origin| origin.to_str().ok())
        .is_none_or(|origin| origin.trim_end_matches('/') == public_url)
}

/// The token in `Authorization: Bearer …`, if any.
#[must_use]
pub fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|token| !token.is_empty())
}

/// The value of the session cookie, if any.
#[must_use]
pub fn session_cookie(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == SESSION_COOKIE)
        .map(|(_, value)| value)
        .filter(|value| !value.is_empty())
}

async fn authenticate(headers: &HeaderMap, state: &AppState) -> Result<Caller, ApiError> {
    if let Some(token) = bearer_token(headers) {
        let kind = [
            (secret::SERVICE, "service"),
            (secret::READ, "read"),
            (secret::ORGANIZATION, "organization"),
        ]
        .into_iter()
        .find(|(prefix, _)| token.starts_with(prefix));
        return match kind {
            Some((_, kind)) => api_token(state, token, kind).await,
            None if token.starts_with(secret::CLI) => session(state, token, false).await,
            None => Err(ApiError::Unauthorized),
        };
    }
    match session_cookie(headers) {
        Some(token) => session(state, token, true).await,
        None => Err(ApiError::Unauthorized),
    }
}

async fn session(state: &AppState, token: &str, from_cookie: bool) -> Result<Caller, ApiError> {
    let hash = secret::hash(token, &state.config.secret_key);
    let row = sqlx::query!(
        r#"
        UPDATE sessions s
        SET last_seen_at = now()
        FROM people p
        WHERE s.token_hash = $1
          AND s.expires_at > now()
          AND p.id = s.person_id
          AND p.removed_at IS NULL
        RETURNING p.id, p.organization_id, p.role, s.client
        "#,
        hash
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::Unauthorized)?;
    // A cookie only ever holds a browser session; a local tool's token is
    // only accepted as a bearer token.
    let credential = match (row.client.as_str(), from_cookie) {
        ("browser", true) => Credential::BrowserSession,
        ("local_tool", false) => Credential::LocalToolSession,
        _ => return Err(ApiError::Unauthorized),
    };
    Ok(Caller {
        organization_id: row.organization_id,
        person_id: row.id,
        role: parse_role(&row.role)?,
        person_removed: false,
        credential,
    })
}

async fn api_token(state: &AppState, token: &str, kind: &str) -> Result<Caller, ApiError> {
    let hash = secret::hash(token, &state.config.secret_key);
    let row = sqlx::query!(
        r#"
        UPDATE api_tokens t
        SET last_used_at = now()
        FROM people p
        WHERE t.token_hash = $1
          AND t.kind = $2
          AND t.revoked_at IS NULL
          AND p.id = t.person_id
        RETURNING t.id, t.organization_id, p.id AS person_id, p.role,
                  p.removed_at IS NOT NULL AS "person_removed!"
        "#,
        hash,
        kind
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(ApiError::Unauthorized)?;
    let credential = match kind {
        "service" => Credential::Service { token_id: row.id },
        "read" => Credential::Read { token_id: row.id },
        _ => Credential::Organization { token_id: row.id },
    };
    // A read token of a person who left reads nothing.
    if row.person_removed && credential.kind() == CredentialKind::Read {
        return Err(ApiError::Unauthorized);
    }
    Ok(Caller {
        organization_id: row.organization_id,
        person_id: row.person_id,
        role: parse_role(&row.role)?,
        person_removed: row.person_removed,
        credential,
    })
}

fn parse_role(role: &str) -> Result<Role, ApiError> {
    role.parse()
        .map_err(|error: crate::organizations::UnknownRole| ApiError::Internal(error.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(*name, HeaderValue::from_static(value));
        }
        map
    }

    #[test]
    fn bearer_tokens_are_read_from_the_authorization_header() {
        assert_eq!(
            bearer_token(&headers(&[("authorization", "Bearer tc_svc_abc")])),
            Some("tc_svc_abc")
        );
        assert_eq!(
            bearer_token(&headers(&[("authorization", "Basic abc")])),
            None
        );
        assert_eq!(
            bearer_token(&headers(&[("authorization", "Bearer ")])),
            None
        );
        assert_eq!(bearer_token(&headers(&[])), None);
    }

    #[test]
    fn the_session_cookie_is_found_among_others() {
        let map = headers(&[("cookie", "theme=dark; tc_session=tc_ses_abc; other=1")]);
        assert_eq!(session_cookie(&map), Some("tc_ses_abc"));
        assert_eq!(session_cookie(&headers(&[("cookie", "tc_session=")])), None);
        assert_eq!(session_cookie(&headers(&[("cookie", "x=1")])), None);
    }

    #[test]
    fn only_another_origin_is_refused() {
        let public = "https://app.tokencontroller.com";
        assert!(same_origin(&headers(&[]), public));
        assert!(same_origin(
            &headers(&[("origin", "https://app.tokencontroller.com")]),
            public
        ));
        assert!(!same_origin(
            &headers(&[("origin", "https://evil.example")]),
            public
        ));
    }

    #[test]
    fn roles_below_the_required_one_are_refused() {
        let caller = Caller {
            organization_id: Uuid::nil(),
            person_id: Uuid::nil(),
            role: Role::PeopleManager,
            person_removed: false,
            credential: Credential::BrowserSession,
        };
        assert!(caller.require_role(Role::AgentManager).is_ok());
        assert!(caller.require_role(Role::PeopleManager).is_ok());
        assert!(caller.require_role(Role::Controller).is_err());
        assert!(caller.require_session().is_ok());
        assert!(
            caller
                .require_credential(&[CredentialKind::Service])
                .is_err()
        );
    }
}
