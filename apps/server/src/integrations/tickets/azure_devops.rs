//! Azure DevOps: projects from `GET /_apis/projects`, tickets in two steps:
//! a WIQL query for the IDs of changed work items, then
//! `POST /_apis/wit/workitemsbatch` for their fields, 200 at a time.
//! Whether a state counts as closed comes from the work item type's state
//! categories, so custom processes work too.

use std::collections::HashSet;

use reqwest::header::ACCEPT;
use serde::{Deserialize, Serialize};
use time::format_description::BorrowedFormatItem;
use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

use super::transport::{endpoint, send};
use super::{Connection, ExternalProject, SyncError, SyncedTicket, TicketSystem, text};

const API_VERSION: (&str, &str) = ("api-version", "7.1");
/// WIQL answers at most 20,000 work items per query.
const WIQL_PAGE: usize = 20_000;
/// The batch call takes at most 200 IDs.
const BATCH_SIZE: usize = 200;
const FIELDS: [&str; 9] = [
    "System.Id",
    "System.Title",
    "System.Description",
    "System.Parent",
    "System.AssignedTo",
    "System.ChangedDate",
    "System.State",
    "System.WorkItemType",
    "System.TeamProject",
];
/// State categories of done work.
const CLOSED_CATEGORIES: [&str; 2] = ["Completed", "Removed"];
const WIQL_TIME: &[BorrowedFormatItem<'static>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]Z");

#[derive(Deserialize)]
struct Projects {
    #[serde(default)]
    value: Vec<Project>,
}

#[derive(Deserialize)]
struct Project {
    id: String,
    name: String,
}

#[derive(Serialize)]
struct Wiql<'a> {
    query: &'a str,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WiqlResult {
    #[serde(default)]
    work_items: Vec<WorkItemReference>,
}

#[derive(Deserialize)]
struct WorkItemReference {
    id: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchRequest<'a> {
    ids: &'a [u64],
    fields: [&'a str; 9],
    error_policy: &'a str,
}

#[derive(Deserialize)]
struct Batch {
    /// With the omit error policy, a work item that cannot be read is null.
    #[serde(default)]
    value: Vec<Option<WorkItem>>,
}

#[derive(Deserialize)]
struct WorkItem {
    id: u64,
    fields: WorkItemFields,
}

/// Fields without a value are left out of the answer.
#[derive(Deserialize)]
struct WorkItemFields {
    #[serde(rename = "System.Title")]
    title: Option<String>,
    #[serde(rename = "System.Description")]
    description: Option<String>,
    #[serde(rename = "System.Parent")]
    parent: Option<u64>,
    #[serde(rename = "System.AssignedTo")]
    assigned_to: Option<Identity>,
    #[serde(rename = "System.ChangedDate")]
    changed_date: String,
    #[serde(rename = "System.State")]
    state: Option<String>,
    #[serde(rename = "System.WorkItemType")]
    work_item_type: Option<String>,
    #[serde(rename = "System.TeamProject")]
    team_project: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Identity {
    display_name: Option<String>,
    /// The email for Microsoft Entra and Microsoft accounts.
    unique_name: Option<String>,
}

#[derive(Deserialize)]
struct WorkItemTypes {
    #[serde(default)]
    value: Vec<WorkItemType>,
}

#[derive(Deserialize)]
struct WorkItemType {
    name: String,
    #[serde(default)]
    states: Vec<WorkItemState>,
}

#[derive(Deserialize)]
struct WorkItemState {
    name: String,
    category: Option<String>,
}

pub(super) async fn list_projects(
    client: &reqwest::Client,
    connection: &Connection,
) -> Result<Vec<ExternalProject>, SyncError> {
    let url = endpoint(connection, &["_apis", "projects"])?;
    let missing = format!(
        "No Azure DevOps organization answers at {}.",
        connection.base_url
    );
    let mut projects = Vec::new();
    let mut continuation: Option<String> = None;
    loop {
        let mut request = client
            .get(url.clone())
            .query(&[API_VERSION])
            .header(ACCEPT, "application/json");
        if let Some(token) = &continuation {
            request = request.query(&[("continuationToken", token)]);
        }
        let reply = send::<Projects>(connection, request, &missing).await?;
        projects.extend(reply.body.value.into_iter().map(|project| ExternalProject {
            key: project.id,
            name: project.name,
        }));
        let next = reply
            .headers
            .get("x-ms-continuationtoken")
            .and_then(|value| value.to_str().ok())
            .filter(|token| !token.is_empty());
        match next {
            None => return Ok(projects),
            Some(next) if continuation.as_deref() == Some(next) => {
                return Err(SyncError::Format(
                    "Azure DevOps sent the same continuation token twice".to_owned(),
                ));
            }
            Some(next) => continuation = Some(next.to_owned()),
        }
    }
}

pub(super) async fn fetch_updated(
    client: &reqwest::Client,
    connection: &Connection,
    project: &str,
    since: Option<OffsetDateTime>,
) -> Result<Vec<SyncedTicket>, SyncError> {
    let ids = changed_ids(client, connection, project, since, WIQL_PAGE).await?;
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let closed = closed_states(client, connection, project).await?;
    let url = endpoint(connection, &["_apis", "wit", "workitemsbatch"])?;
    let missing = missing_project(project);
    let mut tickets = Vec::with_capacity(ids.len());
    for chunk in ids.chunks(BATCH_SIZE) {
        let body = BatchRequest {
            ids: chunk,
            fields: FIELDS,
            error_policy: "omit",
        };
        let request = client
            .post(url.clone())
            .query(&[API_VERSION])
            .header(ACCEPT, "application/json")
            .json(&body);
        let batch: Batch = send(connection, request, &missing).await?.body;
        for item in batch.value.into_iter().flatten() {
            tickets.push(ticket(connection, project, item, &closed)?);
        }
    }
    Ok(tickets)
}

fn missing_project(project: &str) -> String {
    format!("The Azure DevOps project {project} was not found, or the token cannot see it.")
}

/// The IDs of the project's work items changed at or after `since`, in
/// ID order, `page` per query: each query asks only for IDs above the
/// last one seen, so no project is too big.
async fn changed_ids(
    client: &reqwest::Client,
    connection: &Connection,
    project: &str,
    since: Option<OffsetDateTime>,
    page: usize,
) -> Result<Vec<u64>, SyncError> {
    let url = endpoint(connection, &[project, "_apis", "wit", "wiql"])?;
    let missing = missing_project(project);
    let changed = match since {
        Some(since) => {
            let since = since
                .to_offset(UtcOffset::UTC)
                .format(WIQL_TIME)
                .map_err(|error| SyncError::Format(format!("the sync start: {error}")))?;
            format!(" AND [System.ChangedDate] >= '{since}'")
        }
        None => String::new(),
    };
    let top = page.to_string();
    let mut ids: Vec<u64> = Vec::new();
    loop {
        let after = ids.last().copied().unwrap_or_default();
        let query = format!(
            "SELECT [System.Id] FROM WorkItems WHERE [System.TeamProject] = @project \
             AND [System.Id] > {after}{changed} ORDER BY [System.Id] ASC"
        );
        let request = client
            .post(url.clone())
            .query(&[
                API_VERSION,
                ("timePrecision", "true"),
                ("$top", top.as_str()),
            ])
            .header(ACCEPT, "application/json")
            .json(&Wiql { query: &query });
        let result: WiqlResult = send(connection, request, &missing).await?.body;
        let count = result.work_items.len();
        ids.extend(
            result
                .work_items
                .iter()
                .map(|item| item.id)
                .filter(|id| *id > after),
        );
        if count < page {
            return Ok(ids);
        }
        if ids.last().copied().unwrap_or_default() <= after {
            return Err(SyncError::Format(
                "Azure DevOps answered the same work items twice".to_owned(),
            ));
        }
    }
}

/// The (work item type, state) pairs whose state category is done work.
async fn closed_states(
    client: &reqwest::Client,
    connection: &Connection,
    project: &str,
) -> Result<HashSet<(String, String)>, SyncError> {
    let url = endpoint(connection, &[project, "_apis", "wit", "workitemtypes"])?;
    let request = client
        .get(url)
        .query(&[API_VERSION])
        .header(ACCEPT, "application/json");
    let types: WorkItemTypes = send(connection, request, &missing_project(project))
        .await?
        .body;
    Ok(types
        .value
        .into_iter()
        .flat_map(|kind| {
            let name = kind.name;
            kind.states
                .into_iter()
                .filter(|state| {
                    state
                        .category
                        .as_deref()
                        .is_some_and(|category| CLOSED_CATEGORIES.contains(&category))
                })
                .map(move |state| (name.clone(), state.name))
        })
        .collect())
}

fn ticket(
    connection: &Connection,
    project: &str,
    item: WorkItem,
    closed: &HashSet<(String, String)>,
) -> Result<SyncedTicket, SyncError> {
    let fields = item.fields;
    let id = item.id.to_string();
    let updated_at = OffsetDateTime::parse(&fields.changed_date, &Rfc3339)
        .map_err(|error| {
            SyncError::Format(format!(
                "Azure DevOps time {:?}: {error}",
                fields.changed_date
            ))
        })?
        .to_offset(UtcOffset::UTC);
    let url = fields.team_project.as_deref().and_then(|team_project| {
        endpoint(connection, &[team_project, "_workitems", "edit", &id])
            .ok()
            .map(String::from)
    });
    let is_closed = match (fields.work_item_type, fields.state) {
        (Some(kind), Some(state)) => closed.contains(&(kind, state)),
        _ => false,
    };
    let (assignee_email, assignee_name) = fields.assigned_to.map_or((None, None), |identity| {
        (
            identity.unique_name.filter(|name| name.contains('@')),
            identity.display_name.filter(|name| !name.is_empty()),
        )
    });
    Ok(SyncedTicket {
        system: TicketSystem::AzureDevOps,
        external_project: project.to_owned(),
        key: format!("AB#{id}"),
        title: fields.title.filter(|title| !title.is_empty()),
        description: fields
            .description
            .and_then(|html| text::description(text::from_html(&html))),
        parent_key: fields.parent.map(|parent| format!("AB#{parent}")),
        assignee_email,
        assignee_name,
        url,
        updated_at,
        closed: is_closed,
    })
}

// Recorded responses follow the Azure DevOps REST API 7.1 reference on
// Microsoft Learn: Wiql - Query By Wiql, Work Items - Get Work Items Batch,
// Work Item Types - List and Projects - List
// (https://learn.microsoft.com/en-us/rest/api/azure/devops/wit/?view=azure-devops-rest-7.1).
#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;
    use time::macros::datetime;
    use wiremock::matchers::{
        basic_auth, body_partial_json, body_string_contains, method, path, query_param,
        query_param_is_missing,
    };
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::integrations::http::client;
    use crate::integrations::tickets::{
        Credentials, MAX_DESCRIPTION_CHARS, fetch_updated, list_projects,
    };

    const PROJECT: &str = "6ce954b1-ce1f-45d1-b94d-e6bf2464ba2c";

    macro_rules! fixture {
        ($name:literal) => {
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/tickets/azure_devops/",
                $name
            ))
        };
    }

    fn connection(server: &MockServer) -> Connection {
        Connection {
            system: TicketSystem::AzureDevOps,
            base_url: format!("{}/acme", server.uri()),
            credentials: Credentials::Token("ado-pat-test".to_owned()),
        }
    }

    fn json_reply(status: u16, body: &str) -> ResponseTemplate {
        ResponseTemplate::new(status).set_body_raw(body.to_owned(), "application/json")
    }

    async fn mount_types(server: &MockServer) {
        Mock::given(method("GET"))
            .and(path(format!("/acme/{PROJECT}/_apis/wit/workitemtypes")))
            .and(query_param("api-version", "7.1"))
            .respond_with(json_reply(200, fixture!("workitemtypes.json")))
            .expect(1)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn changed_work_items_become_tickets() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(format!("/acme/{PROJECT}/_apis/wit/wiql")))
            .and(basic_auth("", "ado-pat-test"))
            .and(query_param("api-version", "7.1"))
            .and(query_param("timePrecision", "true"))
            .and(query_param("$top", "20000"))
            .and(body_string_contains(
                "[System.Id] > 0 AND [System.ChangedDate] >= '2026-10-01T08:00:00Z' ORDER BY [System.Id] ASC",
            ))
            .respond_with(json_reply(200, fixture!("wiql.json")))
            .expect(1)
            .mount(&server)
            .await;
        mount_types(&server).await;
        Mock::given(method("POST"))
            .and(path("/acme/_apis/wit/workitemsbatch"))
            .and(basic_auth("", "ado-pat-test"))
            .and(query_param("api-version", "7.1"))
            .and(body_partial_json(json!({
                "ids": [1201, 1234, 1240],
                "fields": FIELDS,
                "errorPolicy": "omit"
            })))
            .respond_with(json_reply(200, fixture!("workitemsbatch.json")))
            .expect(1)
            .mount(&server)
            .await;

        let since = datetime!(2026-10-01 10:00 +02:00);
        let tickets = fetch_updated(
            &client().unwrap(),
            &connection(&server),
            PROJECT,
            Some(since),
        )
        .await
        .unwrap();

        let keys: Vec<&str> = tickets.iter().map(|ticket| ticket.key.as_str()).collect();
        assert_eq!(keys, ["AB#1201", "AB#1234", "AB#1240"]);
        let epic = &tickets[0];
        assert_eq!(epic.system, TicketSystem::AzureDevOps);
        assert_eq!(epic.external_project, PROJECT);
        assert_eq!(epic.title.as_deref(), Some("Checkout revamp"));
        assert_eq!(epic.parent_key, None);
        assert!(!epic.closed);
        let story = &tickets[1];
        assert_eq!(
            story.description.as_deref(),
            Some(
                "Guests who pay with a saved card see the order stuck in \"pending\".\nSeen on iOS & web"
            )
        );
        assert_eq!(story.parent_key.as_deref(), Some("AB#1201"));
        assert_eq!(
            story.assignee_email.as_deref(),
            Some("mia.krystof@acme.example")
        );
        assert_eq!(story.assignee_name.as_deref(), Some("Mia Krystof"));
        assert_eq!(
            story.url,
            Some(format!(
                "{}/acme/Fabrikam%20Fiber/_workitems/edit/1234",
                server.uri()
            ))
        );
        assert_eq!(story.updated_at, datetime!(2026-10-12 08:15:42.48 UTC));
        assert!(story.closed);
        let task = &tickets[2];
        assert_eq!(task.assignee_email, None);
        assert_eq!(task.assignee_name.as_deref(), Some("Build Service (acme)"));
        assert_eq!(task.description, None);
        assert!(task.closed, "Removed counts as closed");
    }

    #[tokio::test]
    async fn work_items_are_fetched_200_at_a_time() {
        let server = MockServer::start().await;
        let ids: Vec<u64> = (1..=201).collect();
        let references: Vec<_> = ids
            .iter()
            .map(|id| json!({ "id": id, "url": format!("https://dev.azure.com/acme/_apis/wit/workItems/{id}") }))
            .collect();
        Mock::given(method("POST"))
            .and(path(format!("/acme/{PROJECT}/_apis/wit/wiql")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "queryType": "flat",
                "queryResultType": "workItem",
                "asOf": "2026-10-12T08:20:11.437Z",
                "workItems": references
            })))
            .mount(&server)
            .await;
        mount_types(&server).await;
        for (chunk, page) in [
            (&ids[..200], fixture!("workitemsbatch_page_1.json")),
            (&ids[200..], fixture!("workitemsbatch_page_2.json")),
        ] {
            Mock::given(method("POST"))
                .and(path("/acme/_apis/wit/workitemsbatch"))
                .and(body_partial_json(json!({ "ids": chunk })))
                .respond_with(json_reply(200, page))
                .expect(1)
                .mount(&server)
                .await;
        }

        let tickets = fetch_updated(&client().unwrap(), &connection(&server), PROJECT, None)
            .await
            .unwrap();

        let keys: Vec<&str> = tickets.iter().map(|ticket| ticket.key.as_str()).collect();
        assert_eq!(keys, ["AB#1", "AB#2", "AB#201"]);
        let long = tickets[2].description.as_deref().unwrap();
        assert_eq!(long.chars().count(), MAX_DESCRIPTION_CHARS);
        assert!(long.starts_with("Agents ran long on this ticket."));
    }

    #[tokio::test]
    async fn wiql_pages_continue_after_the_last_id() {
        let server = MockServer::start().await;
        for (after, ids) in [("> 0 ", vec![5, 9]), ("> 9 ", vec![12])] {
            let references: Vec<_> = ids.iter().map(|id| json!({ "id": id })).collect();
            Mock::given(method("POST"))
                .and(path(format!("/acme/{PROJECT}/_apis/wit/wiql")))
                .and(query_param("$top", "2"))
                .and(body_string_contains(format!("[System.Id] {after}ORDER BY")))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({ "workItems": references })),
                )
                .expect(1)
                .mount(&server)
                .await;
        }
        let ids = changed_ids(&client().unwrap(), &connection(&server), PROJECT, None, 2)
            .await
            .unwrap();
        assert_eq!(ids, [5, 9, 12]);
    }

    #[tokio::test]
    async fn projects_follow_the_continuation_token() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/acme/_apis/projects"))
            .and(query_param("continuationToken", "2"))
            .respond_with(json_reply(200, fixture!("projects_page_2.json")))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/acme/_apis/projects"))
            .and(query_param("api-version", "7.1"))
            .and(query_param_is_missing("continuationToken"))
            .respond_with(
                json_reply(200, fixture!("projects_page_1.json"))
                    .insert_header("x-ms-continuationtoken", "2"),
            )
            .expect(1)
            .mount(&server)
            .await;
        let projects = list_projects(&client().unwrap(), &connection(&server))
            .await
            .unwrap();
        let names: Vec<&str> = projects
            .iter()
            .map(|project| project.name.as_str())
            .collect();
        assert_eq!(names, ["Fabrikam Fiber", "Payments", "Storefront"]);
        assert_eq!(projects[0].key, PROJECT);
    }

    #[tokio::test]
    async fn error_answers_become_sync_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/acme/Gone/_apis/wit/wiql"))
            .respond_with(json_reply(404, fixture!("error_project_not_found.json")))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/acme/Busy/_apis/wit/wiql"))
            .respond_with(
                json_reply(429, fixture!("error_rate_limited.json"))
                    .insert_header("Retry-After", "12"),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/acme/_apis/projects"))
            .respond_with(ResponseTemplate::new(203).set_body_raw(
                "<html><head><title>Azure DevOps Services | Sign In</title></head></html>",
                "text/html",
            ))
            .mount(&server)
            .await;
        let http = client().unwrap();
        let gone = fetch_updated(&http, &connection(&server), "Gone", None)
            .await
            .unwrap_err();
        assert_eq!(
            gone,
            SyncError::NotFound(
                "The Azure DevOps project Gone was not found, or the token cannot see it."
                    .to_owned()
            )
        );
        let busy = fetch_updated(&http, &connection(&server), "Busy", None)
            .await
            .unwrap_err();
        assert_eq!(
            busy,
            SyncError::RateLimited {
                retry_after: Some(Duration::from_secs(12))
            }
        );
        let sign_in_page = list_projects(&http, &connection(&server))
            .await
            .unwrap_err();
        assert_eq!(
            sign_in_page,
            SyncError::Unauthorized(
                "Azure DevOps did not accept the personal access token. Check it under Settings, Ticket systems."
                    .to_owned()
            )
        );
    }

    #[tokio::test]
    async fn a_refused_token_is_unauthorized() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let error = fetch_updated(&client().unwrap(), &connection(&server), PROJECT, None)
            .await
            .unwrap_err();
        assert!(
            matches!(error, SyncError::Unauthorized(message) if message.starts_with("Azure DevOps"))
        );
    }
}
