//! Jira Cloud: projects from `GET /rest/api/3/project/search`, tickets from
//! the enhanced JQL search `POST /rest/api/3/search/jql`, paged by
//! `nextPageToken`. Descriptions arrive in the Atlassian Document Format.

use reqwest::header::ACCEPT;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::format_description::BorrowedFormatItem;
use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

use super::transport::{endpoint, send};
use super::{Connection, ExternalProject, SyncError, SyncedTicket, TicketSystem, text};

const PAGE_SIZE: usize = 100;
const FIELDS: [&str; 6] = [
    "summary",
    "description",
    "parent",
    "assignee",
    "updated",
    "status",
];
/// How Jira Cloud writes times, such as `2026-10-12T10:14:03.512+0200`.
const JIRA_TIME: &[BorrowedFormatItem<'static>] = format_description!(
    "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond][offset_hour sign:mandatory][offset_minute]"
);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectPage {
    #[serde(default)]
    values: Vec<Project>,
    #[serde(default)]
    is_last: bool,
}

#[derive(Deserialize)]
struct Project {
    key: String,
    name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchRequest<'a> {
    jql: &'a str,
    fields: [&'a str; 6],
    max_results: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_page_token: Option<&'a str>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchPage {
    #[serde(default)]
    issues: Vec<Issue>,
    next_page_token: Option<String>,
    #[serde(default)]
    is_last: bool,
}

#[derive(Deserialize)]
struct Issue {
    key: String,
    fields: IssueFields,
}

#[derive(Deserialize)]
struct IssueFields {
    summary: Option<String>,
    description: Option<Value>,
    parent: Option<Parent>,
    assignee: Option<User>,
    updated: String,
    status: Option<Status>,
}

#[derive(Deserialize)]
struct Parent {
    key: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct User {
    /// Missing or null when the person's privacy settings hide it.
    email_address: Option<String>,
    display_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    status_category: Option<StatusCategory>,
}

#[derive(Deserialize)]
struct StatusCategory {
    /// `new`, `indeterminate` or `done`.
    key: String,
}

pub(super) async fn list_projects(
    client: &reqwest::Client,
    connection: &Connection,
) -> Result<Vec<ExternalProject>, SyncError> {
    let url = endpoint(connection, &["rest", "api", "3", "project", "search"])?;
    let missing = format!("No Jira Cloud site answers at {}.", connection.base_url);
    let max_results = PAGE_SIZE.to_string();
    let mut projects = Vec::new();
    loop {
        let start_at = projects.len().to_string();
        let request = client
            .get(url.clone())
            .query(&[
                ("startAt", start_at.as_str()),
                ("maxResults", max_results.as_str()),
            ])
            .header(ACCEPT, "application/json");
        let page: ProjectPage = send(connection, request, &missing).await?.body;
        let done = page.is_last || page.values.is_empty();
        projects.extend(page.values.into_iter().map(|project| ExternalProject {
            key: project.key,
            name: project.name,
        }));
        if done {
            return Ok(projects);
        }
    }
}

pub(super) async fn fetch_updated(
    client: &reqwest::Client,
    connection: &Connection,
    project: &str,
    since: Option<OffsetDateTime>,
) -> Result<Vec<SyncedTicket>, SyncError> {
    let url = endpoint(connection, &["rest", "api", "3", "search", "jql"])?;
    let jql = jql(project, since)?;
    let missing = missing_project(project);
    let mut tickets = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let body = SearchRequest {
            jql: &jql,
            fields: FIELDS,
            max_results: PAGE_SIZE,
            next_page_token: token.as_deref(),
        };
        let request = client
            .post(url.clone())
            .header(ACCEPT, "application/json")
            .json(&body);
        let page: SearchPage = match send(connection, request, &missing).await {
            // A project that is gone makes the JQL invalid: 400, not 404.
            Err(error @ SyncError::Provider { status: 400, .. }) => {
                return Err(gone_or(client, connection, project, error).await);
            }
            reply => reply?.body,
        };
        for issue in page.issues {
            tickets.push(ticket(connection, project, issue)?);
        }
        match page.next_page_token {
            Some(next) if !page.is_last => {
                if token.as_deref() == Some(next.as_str()) {
                    return Err(SyncError::Format(
                        "Jira Cloud sent the same page token twice".to_owned(),
                    ));
                }
                token = Some(next);
            }
            _ => return Ok(tickets),
        }
    }
}

fn missing_project(project: &str) -> String {
    format!("The Jira Cloud project {project} was not found, or the account cannot see it.")
}

/// [`SyncError::NotFound`] if the project is gone, else `error`.
async fn gone_or(
    client: &reqwest::Client,
    connection: &Connection,
    project: &str,
    error: SyncError,
) -> SyncError {
    let Ok(url) = endpoint(connection, &["rest", "api", "3", "project", project]) else {
        return error;
    };
    let request = client.get(url).header(ACCEPT, "application/json");
    match send::<Value>(connection, request, &missing_project(project)).await {
        Err(gone @ SyncError::NotFound(_)) => gone,
        _ => error,
    }
}

/// The search for the project's tickets, oldest change first. `since` is
/// sent as milliseconds since 1970: Jira Cloud reads a quoted date such as
/// `"2026-10-12 08:15"` in the account's own time zone, which would skip
/// changes for accounts west of UTC.
fn jql(project: &str, since: Option<OffsetDateTime>) -> Result<String, SyncError> {
    let project = project.replace('\\', "\\\\").replace('"', "\\\"");
    let Some(since) = since else {
        return Ok(format!("project = \"{project}\" ORDER BY updated ASC"));
    };
    let millis = since
        .unix_timestamp_nanos()
        .checked_div(1_000_000)
        .ok_or_else(|| SyncError::Format("the sync start is out of range".to_owned()))?;
    Ok(format!(
        "project = \"{project}\" AND updated >= {millis} ORDER BY updated ASC"
    ))
}

fn ticket(connection: &Connection, project: &str, issue: Issue) -> Result<SyncedTicket, SyncError> {
    let fields = issue.fields;
    let updated_at = OffsetDateTime::parse(&fields.updated, JIRA_TIME)
        .or_else(|_| OffsetDateTime::parse(&fields.updated, &Rfc3339))
        .map_err(|error| {
            SyncError::Format(format!("Jira Cloud time {:?}: {error}", fields.updated))
        })?
        .to_offset(UtcOffset::UTC);
    let description = fields.description.and_then(|description| {
        text::description(match description {
            Value::String(plain) => plain,
            document => text::from_adf(&document),
        })
    });
    let (assignee_email, assignee_name) = fields.assignee.map_or((None, None), |user| {
        (
            user.email_address.filter(|email| !email.is_empty()),
            user.display_name.filter(|name| !name.is_empty()),
        )
    });
    Ok(SyncedTicket {
        system: TicketSystem::Jira,
        external_project: project.to_owned(),
        url: endpoint(connection, &["browse", &issue.key])
            .ok()
            .map(String::from),
        key: issue.key,
        title: fields.summary.filter(|summary| !summary.is_empty()),
        description,
        parent_key: fields.parent.map(|parent| parent.key),
        assignee_email,
        assignee_name,
        updated_at,
        closed: fields
            .status
            .and_then(|status| status.status_category)
            .is_some_and(|category| category.key == "done"),
    })
}

// Recorded responses follow the Jira Cloud REST API v3 reference:
// https://developer.atlassian.com/cloud/jira/platform/rest/v3/api-group-issue-search/
// (search for issues using JQL enhanced search, POST) and
// https://developer.atlassian.com/cloud/jira/platform/rest/v3/api-group-projects/
// (get projects paginated).
#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;
    use time::macros::datetime;
    use wiremock::matchers::{basic_auth, body_partial_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::integrations::http::client;
    use crate::integrations::tickets::{Credentials, fetch_updated, list_projects};

    macro_rules! fixture {
        ($name:literal) => {
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/tickets/jira/",
                $name
            ))
        };
    }

    fn connection(server: &MockServer) -> Connection {
        Connection {
            system: TicketSystem::Jira,
            base_url: server.uri(),
            credentials: Credentials::EmailAndToken {
                email: "dev@acme.example".to_owned(),
                token: "ATATT3xFfGF0-test".to_owned(),
            },
        }
    }

    fn json_reply(status: u16, body: &str) -> ResponseTemplate {
        ResponseTemplate::new(status).set_body_raw(body.to_owned(), "application/json")
    }

    #[tokio::test]
    async fn changed_tickets_come_from_all_pages() {
        let server = MockServer::start().await;
        let jql = "project = \"KD\" AND updated >= 1790842500000 ORDER BY updated ASC";
        Mock::given(method("POST"))
            .and(path("/rest/api/3/search/jql"))
            .and(basic_auth("dev@acme.example", "ATATT3xFfGF0-test"))
            .and(body_partial_json(json!({ "nextPageToken": "CAEaAggD" })))
            .respond_with(json_reply(200, fixture!("search_page_2.json")))
            .with_priority(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/rest/api/3/search/jql"))
            .and(body_partial_json(json!({
                "jql": jql,
                "fields": ["summary", "description", "parent", "assignee", "updated", "status"],
                "maxResults": 100
            })))
            .respond_with(json_reply(200, fixture!("search_page_1.json")))
            .expect(1)
            .mount(&server)
            .await;

        let since = datetime!(2026-10-01 08:15 UTC);
        let tickets = fetch_updated(&client().unwrap(), &connection(&server), "KD", Some(since))
            .await
            .unwrap();

        let keys: Vec<&str> = tickets.iter().map(|ticket| ticket.key.as_str()).collect();
        assert_eq!(keys, ["KD-214", "KD-215", "KD-216"]);
        let first = &tickets[0];
        assert_eq!(first.system, TicketSystem::Jira);
        assert_eq!(first.external_project, "KD");
        assert_eq!(
            first.title.as_deref(),
            Some("Guest checkout fails with saved cards")
        );
        assert_eq!(
            first.description.as_deref(),
            Some(
                "Guests who pay with a saved card see the order stuck in pending.\nSeen on iOS and web"
            )
        );
        assert_eq!(first.parent_key.as_deref(), Some("KD-200"));
        assert_eq!(
            first.assignee_email.as_deref(),
            Some("mia.krystof@acme.example")
        );
        assert_eq!(first.assignee_name.as_deref(), Some("Mia Krystof"));
        assert_eq!(first.url, Some(format!("{}/browse/KD-214", server.uri())));
        assert_eq!(first.updated_at, datetime!(2026-10-12 08:14:03.512 UTC));
        assert!(!first.closed);
        let hidden_email = &tickets[1];
        assert_eq!(hidden_email.assignee_email, None);
        assert_eq!(
            hidden_email.assignee_name.as_deref(),
            Some("Jamal Hartnett")
        );
        assert_eq!(hidden_email.description, None);
        let done = &tickets[2];
        assert!(done.closed);
        assert_eq!(done.parent_key, None);
        assert_eq!(done.assignee_name, None);
    }

    #[tokio::test]
    async fn without_since_every_ticket_is_asked_for() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/api/3/search/jql"))
            .and(body_partial_json(
                json!({ "jql": "project = \"KD\" ORDER BY updated ASC" }),
            ))
            .respond_with(json_reply(200, fixture!("search_page_2.json")))
            .expect(1)
            .mount(&server)
            .await;
        let tickets = fetch_updated(&client().unwrap(), &connection(&server), "KD", None)
            .await
            .unwrap();
        assert_eq!(tickets.len(), 1);
    }

    #[test]
    fn jql_quotes_the_project_and_counts_from_1970() {
        assert_eq!(
            jql(r#"K"D\"#, None).unwrap(),
            r#"project = "K\"D\\" ORDER BY updated ASC"#
        );
        assert_eq!(
            jql("KD", Some(datetime!(2026-10-01 10:15:00.999 +02:00))).unwrap(),
            "project = \"KD\" AND updated >= 1790842500999 ORDER BY updated ASC"
        );
    }

    #[tokio::test]
    async fn projects_come_from_all_pages() {
        let server = MockServer::start().await;
        for (start_at, page) in [
            ("0", fixture!("project_search_page_1.json")),
            ("2", fixture!("project_search_page_2.json")),
        ] {
            Mock::given(method("GET"))
                .and(path("/rest/api/3/project/search"))
                .and(query_param("startAt", start_at))
                .and(query_param("maxResults", "100"))
                .respond_with(json_reply(200, page))
                .expect(1)
                .mount(&server)
                .await;
        }
        let projects = list_projects(&client().unwrap(), &connection(&server))
            .await
            .unwrap();
        let shown: Vec<(&str, &str)> = projects
            .iter()
            .map(|project| (project.key.as_str(), project.name.as_str()))
            .collect();
        assert_eq!(
            shown,
            [
                ("KD", "Kundendienst"),
                ("OPS", "Operations"),
                ("WEB", "Storefront")
            ]
        );
    }

    #[tokio::test]
    async fn a_project_that_is_gone_is_not_found() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/api/3/search/jql"))
            .respond_with(json_reply(
                400,
                fixture!("error_search_unknown_project.json"),
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/rest/api/3/project/GONE"))
            .respond_with(json_reply(404, fixture!("error_project_not_found.json")))
            .expect(1)
            .mount(&server)
            .await;
        let error = fetch_updated(&client().unwrap(), &connection(&server), "GONE", None)
            .await
            .unwrap_err();
        assert_eq!(
            error,
            SyncError::NotFound(
                "The Jira Cloud project GONE was not found, or the account cannot see it."
                    .to_owned()
            )
        );
    }

    #[tokio::test]
    async fn a_bad_search_for_a_project_that_exists_stays_a_provider_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/api/3/search/jql"))
            .respond_with(json_reply(
                400,
                fixture!("error_search_unknown_project.json"),
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/rest/api/3/project/KD"))
            .respond_with(json_reply(
                200,
                r#"{"id":"10000","key":"KD","name":"Kundendienst"}"#,
            ))
            .mount(&server)
            .await;
        let error = fetch_updated(&client().unwrap(), &connection(&server), "KD", None)
            .await
            .unwrap_err();
        assert_eq!(
            error,
            SyncError::Provider {
                status: 400,
                message: "The value 'GONE' does not exist for the field 'project'.".to_owned()
            }
        );
    }

    #[tokio::test]
    async fn refused_credentials_and_rate_limits_are_named() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/rest/api/3/project/search"))
            .respond_with(
                ResponseTemplate::new(401)
                    .set_body_string("Client must be authenticated to access this resource."),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/rest/api/3/search/jql"))
            .respond_with(
                json_reply(429, fixture!("error_rate_limited.json"))
                    .insert_header("Retry-After", "30"),
            )
            .mount(&server)
            .await;
        let http = client().unwrap();
        let refused = list_projects(&http, &connection(&server))
            .await
            .unwrap_err();
        assert_eq!(
            refused,
            SyncError::Unauthorized(
                "Jira Cloud did not accept the account email and API token. \
                 Check it under Settings, Ticket systems."
                    .to_owned()
            )
        );
        let limited = fetch_updated(&http, &connection(&server), "KD", None)
            .await
            .unwrap_err();
        assert_eq!(
            limited,
            SyncError::RateLimited {
                retry_after: Some(Duration::from_secs(30))
            }
        );
    }
}
