//! Ticket sync from Jira Cloud: titles, descriptions, parents and
//! assignees, every 15 minutes or on demand.
//!
//! Two calls make up the whole API. [`list_projects`] lists what a
//! connection can see, so a person can map each Jira project to a project
//! tag value. [`fetch_updated`] returns every ticket of one Jira project
//! that changed since the last sync, all pages read. Jira's paging, sign-in
//! and text formats stay inside its own module.

mod jira;
mod text;
mod transport;

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use utoipa::ToSchema;

/// The ticket systems tickets sync from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub enum TicketSystem {
    #[serde(rename = "jira")]
    Jira,
}

impl TicketSystem {
    /// How the API and the database write it: `jira`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Jira => "jira",
        }
    }

    /// The system [`as_str`](Self::as_str) names.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Jira]
            .into_iter()
            .find(|system| system.as_str() == name)
    }

    /// The name people know the system by, as used in messages.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Jira => "Jira Cloud",
        }
    }

    /// Checks that a base URL is one this system serves, before a
    /// connection is saved, so no request goes to another host: `https`
    /// only, on `*.atlassian.net`.
    ///
    /// # Errors
    ///
    /// A plain message saying what the address should look like.
    pub fn check_base_url(self, base_url: &str) -> Result<(), String> {
        let expected = match self {
            Self::Jira => "a Jira Cloud site, such as https://acme.atlassian.net",
        };
        let fits = reqwest::Url::parse(base_url.trim_end_matches('/')).is_ok_and(|url| {
            let host = url.host_str().unwrap_or_default();
            let path: Vec<&str> = url
                .path_segments()
                .map(|segments| segments.filter(|s| !s.is_empty()).collect())
                .unwrap_or_default();
            let plain = url.scheme() == "https"
                && url.port().is_none()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none();
            plain
                && match self {
                    Self::Jira => host.ends_with(".atlassian.net") && path.is_empty(),
                }
        });
        if fits {
            Ok(())
        } else {
            Err(format!("The address should be {expected}."))
        }
    }
}

/// How Token Controller signs in to a ticket system.
#[derive(Clone, PartialEq, Eq)]
pub enum Credentials {
    /// Jira Cloud: the Atlassian account's email and an API token.
    EmailAndToken { email: String, token: String },
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmailAndToken { .. } => f
                .debug_struct("EmailAndToken")
                .field("email", &"<hidden>")
                .field("token", &"<hidden>")
                .finish(),
        }
    }
}

/// One connected ticket system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    pub system: TicketSystem,
    /// The Jira Cloud site (`https://acme.atlassian.net`).
    pub base_url: String,
    pub credentials: Credentials,
}

/// A project in a ticket system, to map onto a project tag value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ExternalProject {
    /// What [`fetch_updated`] takes: the Jira Cloud project key (`KD`).
    pub key: String,
    /// The name people see.
    pub name: String,
}

/// The longest description kept, in characters.
pub const MAX_DESCRIPTION_CHARS: usize = 4_000;

/// One ticket as its ticket system has it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncedTicket {
    pub system: TicketSystem,
    /// The [`ExternalProject::key`] it was fetched for.
    pub external_project: String,
    /// The ticket key people write in branches and prompts: `KD-214`.
    pub key: String,
    pub title: Option<String>,
    /// Plain text, at most [`MAX_DESCRIPTION_CHARS`] characters.
    pub description: Option<String>,
    /// The parent ticket's key, in the same form as `key`.
    pub parent_key: Option<String>,
    /// The assignee's email, where the system shares it.
    pub assignee_email: Option<String>,
    /// The assignee's name.
    pub assignee_name: Option<String>,
    /// Where people open the ticket.
    pub url: Option<String>,
    pub updated_at: OffsetDateTime,
    /// Done, closed or removed.
    pub closed: bool,
}

/// Why a ticket sync failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SyncError {
    /// The system did not accept the credentials, or they lack access.
    #[error("{0}")]
    Unauthorized(String),
    /// The external project is gone, or the credentials cannot see it.
    #[error("{0}")]
    NotFound(String),
    /// Too many requests; try again after `retry_after`, when known.
    #[error("the ticket system limits requests for now; try again later")]
    RateLimited { retry_after: Option<Duration> },
    /// Any other error answer.
    #[error("the ticket system answered {status}: {message}")]
    Provider { status: u16, message: String },
    /// An answer in a shape this code does not know.
    #[error("the ticket system's answer could not be read: {0}")]
    Format(String),
    /// The system could not be reached.
    #[error("the ticket system could not be reached: {0}")]
    Network(String),
}

/// Lists the Jira Cloud projects the connection can see.
///
/// # Errors
///
/// [`SyncError`] if a request fails or an answer cannot be read.
pub async fn list_projects(
    client: &reqwest::Client,
    connection: &Connection,
) -> Result<Vec<ExternalProject>, SyncError> {
    match connection.system {
        TicketSystem::Jira => jira::list_projects(client, connection).await,
    }
}

/// Fetches every ticket of `external_project` that changed at or after
/// `since`, or all of them when `since` is `None`, reading all pages.
///
/// A ticket can come twice when it changes during the sync; the newer
/// copy comes last.
///
/// # Errors
///
/// [`SyncError::NotFound`] if the external project is gone, and any other
/// [`SyncError`] if a request fails or an answer cannot be read.
pub async fn fetch_updated(
    client: &reqwest::Client,
    connection: &Connection,
    external_project: &str,
    since: Option<OffsetDateTime>,
) -> Result<Vec<SyncedTicket>, SyncError> {
    match connection.system {
        TicketSystem::Jira => {
            jira::fetch_updated(client, connection, external_project, since).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systems_have_stable_names() {
        let names = serde_json::to_string(&[TicketSystem::Jira]).unwrap();
        assert_eq!(names, r#"["jira"]"#);
        let back: TicketSystem = serde_json::from_str(r#""jira""#).unwrap();
        assert_eq!(back, TicketSystem::Jira);
        assert_eq!(TicketSystem::from_name("jira"), Some(TicketSystem::Jira));
        assert_eq!(TicketSystem::from_name("azure_devops"), None);
        assert_eq!(TicketSystem::Jira.name(), "Jira Cloud");
    }

    #[test]
    fn debug_output_hides_credentials() {
        let connection = Connection {
            system: TicketSystem::Jira,
            base_url: "https://acme.atlassian.net".to_owned(),
            credentials: Credentials::EmailAndToken {
                email: "dev@acme.example".to_owned(),
                token: "ATATT3xFfGF0secret".to_owned(),
            },
        };
        let shown = format!("{connection:?}");
        assert!(shown.contains("https://acme.atlassian.net"));
        assert!(!shown.contains("secret"));
        assert!(!shown.contains("dev@acme.example"));
    }

    #[test]
    fn only_the_systems_own_hosts_are_accepted() {
        let jira = TicketSystem::Jira;
        assert_eq!(jira.check_base_url("https://acme.atlassian.net"), Ok(()));
        assert_eq!(jira.check_base_url("https://acme.atlassian.net/"), Ok(()));
        for url in [
            "http://acme.atlassian.net",
            "https://atlassian.net.evil.example",
            "https://acme.atlassian.net/wiki",
            "https://acme.atlassian.net:8443",
            "https://user:pass@acme.atlassian.net",
            "https://dev.azure.com/acme",
            "not a url",
        ] {
            assert!(jira.check_base_url(url).is_err(), "{url} should be refused");
        }
        assert_eq!(
            jira.check_base_url("https://github.com"),
            Err(
                "The address should be a Jira Cloud site, such as https://acme.atlassian.net."
                    .to_owned()
            )
        );
    }
}
