//! `GitHub` Issues: repositories from `GET /user/repos`, tickets from
//! `GET /repos/{owner}/{repo}/issues`, both paged by the `Link` header.
//! The issues list also holds pull requests; they are skipped.

use reqwest::Url;
use reqwest::header::ACCEPT;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use time::format_description::BorrowedFormatItem;
use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

use super::transport::{endpoint, next_link, send};
use super::{Connection, ExternalProject, SyncError, SyncedTicket, TicketSystem, text};

const JSON: &str = "application/vnd.github+json";
/// Like [`JSON`], but each issue also carries `body_text`, `GitHub`'s own
/// plain text rendering of the Markdown body.
const TEXT_JSON: &str = "application/vnd.github.text+json";
const PAGE_SIZE: &str = "100";
const SINCE: &[BorrowedFormatItem<'static>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]Z");

#[derive(Deserialize)]
struct Repository {
    full_name: String,
    has_issues: Option<bool>,
}

#[derive(Deserialize)]
struct Issue {
    number: u64,
    title: Option<String>,
    body: Option<String>,
    body_text: Option<String>,
    html_url: Option<String>,
    /// `open` or `closed`.
    state: String,
    updated_at: String,
    assignee: Option<Account>,
    /// The parent's API URL when this is a sub-issue, such as
    /// `https://api.github.com/repos/acme/storefront/issues/7`.
    parent_issue_url: Option<String>,
    /// Present only on pull requests.
    pull_request: Option<Value>,
}

#[derive(Deserialize)]
struct Account {
    login: String,
}

pub(super) async fn list_projects(
    client: &reqwest::Client,
    connection: &Connection,
) -> Result<Vec<ExternalProject>, SyncError> {
    let mut first = endpoint(connection, &["user", "repos"])?;
    first.query_pairs_mut().append_pair("per_page", PAGE_SIZE);
    let missing = format!("No GitHub API answers at {}.", connection.base_url);
    let repositories: Vec<Repository> =
        all_pages(client, connection, first, JSON, &missing).await?;
    Ok(repositories
        .into_iter()
        .filter(|repository| repository.has_issues != Some(false))
        .map(|repository| ExternalProject {
            key: repository.full_name.clone(),
            name: repository.full_name,
        })
        .collect())
}

pub(super) async fn fetch_updated(
    client: &reqwest::Client,
    connection: &Connection,
    repository: &str,
    since: Option<OffsetDateTime>,
) -> Result<Vec<SyncedTicket>, SyncError> {
    let missing = format!(
        "The GitHub repository {repository} was not found, the token cannot see it, \
         or its issues are switched off."
    );
    let Some((owner, name)) = repository
        .split_once('/')
        .filter(|(owner, name)| !owner.is_empty() && !name.is_empty() && !name.contains('/'))
    else {
        return Err(SyncError::NotFound(missing));
    };
    let mut first = endpoint(connection, &["repos", owner, name, "issues"])?;
    {
        let mut query = first.query_pairs_mut();
        query
            .append_pair("state", "all")
            .append_pair("sort", "updated")
            .append_pair("direction", "asc")
            .append_pair("per_page", PAGE_SIZE);
        if let Some(since) = since {
            let since = since
                .to_offset(UtcOffset::UTC)
                .format(SINCE)
                .map_err(|error| SyncError::Format(format!("the sync start: {error}")))?;
            query.append_pair("since", &since);
        }
    }
    let issues: Vec<Issue> = all_pages(client, connection, first, TEXT_JSON, &missing).await?;
    issues
        .into_iter()
        .filter(|issue| issue.pull_request.is_none())
        .map(|issue| ticket(repository, name, issue))
        .collect()
}

/// Every item of every page, following `rel="next"` links from `first`.
async fn all_pages<T: DeserializeOwned>(
    client: &reqwest::Client,
    connection: &Connection,
    first: Url,
    accept: &str,
    missing: &str,
) -> Result<Vec<T>, SyncError> {
    let mut items = Vec::new();
    let mut next = Some(first);
    while let Some(url) = next {
        let request = client.get(url.clone()).header(ACCEPT, accept);
        let reply = send::<Vec<T>>(connection, request, missing).await?;
        items.extend(reply.body);
        next = next_link(&reply.headers, connection)?;
        if next.as_ref() == Some(&url) {
            return Err(SyncError::Format(
                "GitHub sent a next page link to the same page".to_owned(),
            ));
        }
    }
    Ok(items)
}

fn ticket(repository: &str, name: &str, issue: Issue) -> Result<SyncedTicket, SyncError> {
    let updated_at = OffsetDateTime::parse(&issue.updated_at, &Rfc3339)
        .map_err(|error| SyncError::Format(format!("GitHub time {:?}: {error}", issue.updated_at)))?
        .to_offset(UtcOffset::UTC);
    let body = issue.body_text.or(issue.body).unwrap_or_default();
    Ok(SyncedTicket {
        system: TicketSystem::GitHubIssues,
        external_project: repository.to_owned(),
        key: format!("{name}#{}", issue.number),
        title: issue.title.filter(|title| !title.is_empty()),
        description: text::description(text::collapse_whitespace(&body)),
        parent_key: issue.parent_issue_url.as_deref().and_then(parent_key),
        assignee_email: None,
        assignee_name: issue.assignee.map(|account| account.login),
        url: issue.html_url,
        updated_at,
        closed: issue.state == "closed",
    })
}

/// `storefront#7` from `https://api.github.com/repos/acme/storefront/issues/7`.
fn parent_key(url: &str) -> Option<String> {
    let url = Url::parse(url).ok()?;
    let mut segments = url.path_segments()?.rev();
    let number: u64 = segments.next()?.parse().ok()?;
    let issues = segments.next()?;
    let repository = segments.next()?;
    (issues == "issues" && !repository.is_empty()).then(|| format!("{repository}#{number}"))
}

// Recorded responses follow the GitHub REST API reference
// (X-GitHub-Api-Version 2022-11-28): "List repository issues"
// (https://docs.github.com/rest/issues/issues#list-repository-issues), the
// `issue` schema with `parent_issue_url`, "List repositories for the
// authenticated user" (https://docs.github.com/rest/repos/repos#list-repositories-for-the-authenticated-user)
// and "Rate limits for the REST API".
#[cfg(test)]
mod tests {
    use std::time::Duration;

    use time::macros::datetime;
    use wiremock::matchers::{
        bearer_token, header, method, path, query_param, query_param_is_missing,
    };
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::integrations::http::client;
    use crate::integrations::tickets::{Credentials, fetch_updated, list_projects};

    macro_rules! fixture {
        ($name:literal) => {
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/tickets/github/",
                $name
            ))
        };
    }

    fn connection(server: &MockServer) -> Connection {
        Connection {
            system: TicketSystem::GitHubIssues,
            base_url: server.uri(),
            credentials: Credentials::Token("github_pat_test".to_owned()),
        }
    }

    fn json_reply(status: u16, body: &str) -> ResponseTemplate {
        ResponseTemplate::new(status)
            .set_body_raw(body.to_owned(), "application/json; charset=utf-8")
    }

    #[tokio::test]
    async fn issues_come_from_all_pages_without_pull_requests() {
        let server = MockServer::start().await;
        let next = format!(
            "<{0}/repositories/1300192/issues?state=all&sort=updated&direction=asc&per_page=100\
             &since=2026-10-01T08%3A00%3A00Z&page=2>; rel=\"next\", \
             <{0}/repositories/1300192/issues?state=all&sort=updated&direction=asc&per_page=100\
             &since=2026-10-01T08%3A00%3A00Z&page=2>; rel=\"last\"",
            server.uri()
        );
        Mock::given(method("GET"))
            .and(path("/repos/acme/storefront/issues"))
            .and(bearer_token("github_pat_test"))
            .and(header("x-github-api-version", "2022-11-28"))
            .and(header("accept", TEXT_JSON))
            .and(query_param("state", "all"))
            .and(query_param("sort", "updated"))
            .and(query_param("direction", "asc"))
            .and(query_param("per_page", "100"))
            .and(query_param("since", "2026-10-01T08:00:00Z"))
            .respond_with(
                json_reply(200, fixture!("issues_page_1.json"))
                    .insert_header("link", next.as_str()),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repositories/1300192/issues"))
            .and(bearer_token("github_pat_test"))
            .and(query_param("page", "2"))
            .respond_with(json_reply(200, fixture!("issues_page_2.json")))
            .expect(1)
            .mount(&server)
            .await;

        let since = datetime!(2026-10-01 10:00 +02:00);
        let tickets = fetch_updated(
            &client().unwrap(),
            &connection(&server),
            "acme/storefront",
            Some(since),
        )
        .await
        .unwrap();

        let keys: Vec<&str> = tickets.iter().map(|ticket| ticket.key.as_str()).collect();
        assert_eq!(keys, ["storefront#12", "storefront#14", "storefront#7"]);
        let sub_issue = &tickets[0];
        assert_eq!(sub_issue.system, TicketSystem::GitHubIssues);
        assert_eq!(sub_issue.external_project, "acme/storefront");
        assert_eq!(
            sub_issue.title.as_deref(),
            Some("Guest checkout fails with saved cards")
        );
        assert_eq!(
            sub_issue.description.as_deref(),
            Some(
                "Guests who pay with a saved card see the order stuck in pending.\nSeen on iOS and web"
            )
        );
        assert_eq!(sub_issue.parent_key.as_deref(), Some("storefront#7"));
        assert_eq!(sub_issue.assignee_email, None);
        assert_eq!(sub_issue.assignee_name.as_deref(), Some("mia-krystof"));
        assert_eq!(
            sub_issue.url.as_deref(),
            Some("https://github.com/acme/storefront/issues/12")
        );
        assert_eq!(sub_issue.updated_at, datetime!(2026-10-12 08:14:03 UTC));
        assert!(!sub_issue.closed);
        let closed = &tickets[1];
        assert!(closed.closed);
        assert_eq!(
            closed.parent_key.as_deref(),
            Some("payments#3"),
            "parents may sit in another repository"
        );
        assert_eq!(closed.assignee_name, None);
        assert_eq!(closed.description, None);
        assert_eq!(tickets[2].parent_key, None);
    }

    #[tokio::test]
    async fn repositories_with_issues_come_from_all_pages() {
        let server = MockServer::start().await;
        let next = format!(
            "<{0}/user/repos?per_page=100&page=2>; rel=\"next\", <{0}/user/repos?per_page=100&page=2>; rel=\"last\"",
            server.uri()
        );
        Mock::given(method("GET"))
            .and(path("/user/repos"))
            .and(header("accept", JSON))
            .and(query_param("per_page", "100"))
            .and(query_param_is_missing("page"))
            .respond_with(
                json_reply(200, fixture!("repos_page_1.json")).insert_header("link", next.as_str()),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/user/repos"))
            .and(query_param("page", "2"))
            .respond_with(json_reply(200, fixture!("repos_page_2.json")))
            .expect(1)
            .mount(&server)
            .await;
        let projects = list_projects(&client().unwrap(), &connection(&server))
            .await
            .unwrap();
        let keys: Vec<&str> = projects
            .iter()
            .map(|project| project.key.as_str())
            .collect();
        assert_eq!(
            keys,
            ["acme/storefront", "acme/payments"],
            "acme/infra has issues switched off"
        );
    }

    #[tokio::test]
    async fn rate_limits_say_when_to_retry() {
        let server = MockServer::start().await;
        let reset = OffsetDateTime::now_utc()
            .unix_timestamp()
            .checked_add(120)
            .unwrap();
        Mock::given(path("/repos/acme/storefront/issues"))
            .respond_with(
                json_reply(403, fixture!("error_rate_limited.json"))
                    .insert_header("x-ratelimit-limit", "5000")
                    .insert_header("x-ratelimit-remaining", "0")
                    .insert_header("x-ratelimit-reset", reset.to_string().as_str()),
            )
            .mount(&server)
            .await;
        Mock::given(path("/repos/acme/payments/issues"))
            .respond_with(
                json_reply(429, fixture!("error_secondary_rate_limit.json"))
                    .insert_header("retry-after", "60"),
            )
            .mount(&server)
            .await;
        let http = client().unwrap();
        let primary = fetch_updated(&http, &connection(&server), "acme/storefront", None)
            .await
            .unwrap_err();
        assert!(
            matches!(
                primary,
                SyncError::RateLimited { retry_after: Some(wait) }
                    if wait <= Duration::from_mins(2) && wait >= Duration::from_secs(100)
            ),
            "{primary:?}"
        );
        let secondary = fetch_updated(&http, &connection(&server), "acme/payments", None)
            .await
            .unwrap_err();
        assert_eq!(
            secondary,
            SyncError::RateLimited {
                retry_after: Some(Duration::from_mins(1))
            }
        );
    }

    #[tokio::test]
    async fn refused_tokens_and_missing_repositories_are_named() {
        let server = MockServer::start().await;
        Mock::given(path("/user/repos"))
            .respond_with(json_reply(401, fixture!("error_bad_credentials.json")))
            .mount(&server)
            .await;
        Mock::given(path("/repos/acme/gone/issues"))
            .respond_with(json_reply(404, fixture!("error_not_found.json")))
            .mount(&server)
            .await;
        Mock::given(path("/repos/acme/sso/issues"))
            .respond_with(json_reply(403, fixture!("error_sso_required.json")))
            .mount(&server)
            .await;
        let http = client().unwrap();
        assert_eq!(
            list_projects(&http, &connection(&server))
                .await
                .unwrap_err(),
            SyncError::Unauthorized(
                "GitHub Issues did not accept the token. Check it under Settings, Ticket systems."
                    .to_owned()
            )
        );
        let gone = fetch_updated(&http, &connection(&server), "acme/gone", None)
            .await
            .unwrap_err();
        assert!(matches!(gone, SyncError::NotFound(message) if message.contains("acme/gone")));
        let sso = fetch_updated(&http, &connection(&server), "acme/sso", None)
            .await
            .unwrap_err();
        assert!(
            matches!(&sso, SyncError::Unauthorized(message) if message.contains("SAML enforcement")),
            "{sso:?}"
        );
    }

    #[tokio::test]
    async fn a_repository_needs_its_owner() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        for repository in ["storefront", "acme/", "/storefront", "acme/store/front"] {
            let error = fetch_updated(&client().unwrap(), &connection(&server), repository, None)
                .await
                .unwrap_err();
            assert!(matches!(error, SyncError::NotFound(_)), "{repository}");
        }
    }

    #[test]
    fn parent_keys_come_from_the_parent_url() {
        assert_eq!(
            parent_key("https://api.github.com/repos/acme/storefront/issues/7").as_deref(),
            Some("storefront#7")
        );
        assert_eq!(
            parent_key("https://api.github.com/repos/acme/storefront/pulls/7"),
            None
        );
        assert_eq!(
            parent_key("https://api.github.com/repos/acme/storefront/issues/x"),
            None
        );
        assert_eq!(parent_key("not a url"), None);
    }
}
