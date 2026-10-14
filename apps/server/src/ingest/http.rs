//! The doors over HTTP: the token entries endpoint, CSV import and the
//! OpenTelemetry receiver.

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::header::{CONTENT_ENCODING, CONTENT_TYPE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use tokencontroller_core::extension::{TelemetryBatch, TelemetrySignal};
use utoipa::ToSchema;

use super::csv_import::{self, RowProblem};
use super::entry::NewTokenEntry;
use super::otlp::{self, DecodeError, Encoding, Extracted, Signal};
use super::store::{self, Door, StoredEntryBody, TelemetryOutcome};
use crate::AppState;
use crate::error::{ApiError, ErrorBody};
use crate::integrations::prices;
use crate::sign_in::{Caller, CredentialKind, Member};

/// The most token entries one call may send.
pub const MAX_ENTRIES_PER_CALL: usize = 1_000;

/// The largest request body the doors take, before decompression. OTLP
/// bodies are also limited after it, to [`otlp::MAX_BODY_BYTES`].
pub const MAX_REQUEST_BYTES: usize = otlp::MAX_BODY_BYTES;

/// One token entry, or a list of them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum TokenEntries {
    One(Box<NewTokenEntry>),
    Many(Vec<NewTokenEntry>),
}

/// What storing gave: one result per token entry sent, in the same shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
#[serde(untagged)]
pub enum StoredEntries {
    One(StoredEntryBody),
    Many(Vec<StoredEntryBody>),
}

/// Report token entries. With a service token they arrive automatically,
/// submitted, and the token's creator is their agent manager. Signed in,
/// they are manual entries on your own token sheet, as drafts. Sending one
/// again with the same `session_id` or `local_id` updates it.
#[utoipa::path(
    post,
    path = "/token-entries",
    tag = "token entries",
    request_body = TokenEntries,
    responses(
        (status = CREATED, body = StoredEntries),
        (status = BAD_REQUEST, description = "A token entry breaks the schema; nothing was stored", body = ErrorBody),
        (status = CONFLICT, description = "A token entry is in a closed period; post an adjustment instead", body = ErrorBody),
    ),
)]
pub(super) async fn create_token_entries(
    State(state): State<AppState>,
    caller: Caller,
    Json(body): Json<TokenEntries>,
) -> Result<(StatusCode, Json<StoredEntries>), ApiError> {
    caller.require_credential(&[CredentialKind::Service, CredentialKind::Session])?;
    let door = if caller.credential.kind() == CredentialKind::Service {
        Door::Endpoint
    } else {
        if caller.person_removed {
            return Err(ApiError::Unauthorized);
        }
        Door::Manual
    };
    let (one, entries) = match body {
        TokenEntries::One(entry) => (true, vec![*entry]),
        TokenEntries::Many(entries) => (false, entries),
    };
    if entries.len() > MAX_ENTRIES_PER_CALL {
        return Err(ApiError::BadRequest(format!(
            "send at most {MAX_ENTRIES_PER_CALL} token entries per call"
        )));
    }
    let pricing = prices::current_pricing(&state.db).await?;
    let mut tx = state.db.begin().await?;
    let stored = store::store_token_entries(
        &mut tx,
        caller.organization_id,
        caller.person_id,
        entries,
        door,
        &pricing,
    )
    .await?;
    tx.commit().await?;
    let mut bodies: Vec<StoredEntryBody> = stored.iter().map(StoredEntryBody::from).collect();
    let answer = match (one, bodies.pop()) {
        (true, Some(body)) => StoredEntries::One(body),
        (_, last) => {
            bodies.extend(last);
            StoredEntries::Many(bodies)
        }
    };
    Ok((StatusCode::CREATED, Json(answer)))
}

/// What a CSV file holds, before anything is saved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct CsvPreview {
    /// Rows that can be imported.
    pub ready: usize,
    /// The rows that cannot, with what is wrong in each.
    pub problems: Vec<RowProblem>,
}

/// What a CSV import saved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct CsvImported {
    pub token_entries: Vec<StoredEntryBody>,
}

fn read_csv(body: &Bytes) -> Result<Vec<csv_import::Row>, ApiError> {
    let text = std::str::from_utf8(body)
        .map_err(|_| ApiError::BadRequest("the file is not UTF-8 text".to_owned()))?;
    csv_import::parse(text).map_err(|error| ApiError::BadRequest(error.to_string()))
}

fn problems(rows: &[csv_import::Row]) -> Vec<RowProblem> {
    rows.iter()
        .filter_map(|row| {
            row.result.as_ref().err().map(|problem| RowProblem {
                line: row.line,
                problem: problem.clone(),
            })
        })
        .collect()
}

/// Read a CSV file of token entries and show what it holds, with the
/// problem in each row that cannot be read. Saves nothing.
#[utoipa::path(
    post,
    path = "/token-entries/csv/preview",
    tag = "token entries",
    request_body(content = String, content_type = "text/csv"),
    responses(
        (status = OK, body = CsvPreview),
        (status = BAD_REQUEST, description = "The file as a whole cannot be read", body = ErrorBody),
    ),
)]
pub(super) async fn preview_csv(
    _member: Member,
    body: Bytes,
) -> Result<Json<CsvPreview>, ApiError> {
    let rows = read_csv(&body)?;
    let problems = problems(&rows);
    Ok(Json(CsvPreview {
        ready: rows.len().saturating_sub(problems.len()),
        problems,
    }))
}

/// Import a CSV file of token entries onto your token sheet, as drafts.
/// Every row must be readable; otherwise nothing is saved.
#[utoipa::path(
    post,
    path = "/token-entries/csv",
    tag = "token entries",
    request_body(content = String, content_type = "text/csv"),
    responses(
        (status = CREATED, body = CsvImported),
        (status = BAD_REQUEST, description = "A row cannot be read; nothing was saved", body = ErrorBody),
        (status = CONFLICT, description = "A row is in a closed period", body = ErrorBody),
    ),
)]
pub(super) async fn import_csv(
    State(state): State<AppState>,
    member: Member,
    body: Bytes,
) -> Result<(StatusCode, Json<CsvImported>), ApiError> {
    member.require_credential(&[CredentialKind::Session])?;
    let rows = read_csv(&body)?;
    if let Some(first) = problems(&rows).first() {
        return Err(ApiError::BadRequest(format!(
            "line {}: {}; nothing was saved",
            first.line, first.problem
        )));
    }
    let checked = rows.into_iter().filter_map(|row| row.result.ok()).collect();
    let pricing = prices::current_pricing(&state.db).await?;
    let mut tx = state.db.begin().await?;
    let stored = store::store_checked(
        &mut tx,
        member.organization_id,
        member.person_id,
        checked,
        Door::Csv,
        &pricing,
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(CsvImported {
            token_entries: stored.iter().map(StoredEntryBody::from).collect(),
        }),
    ))
}

fn decode_failure(error: &DecodeError) -> Response {
    let (status, code) = match error {
        DecodeError::UnsupportedContentType(_) | DecodeError::UnsupportedContentEncoding(_) => {
            (StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported_media_type")
        }
        DecodeError::TooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "too_large"),
        DecodeError::Gzip(_) | DecodeError::Protobuf(_) | DecodeError::Json(_) => {
            (StatusCode::BAD_REQUEST, "bad_request")
        }
    };
    (
        status,
        Json(ErrorBody {
            error: code,
            message: error.to_string(),
        }),
    )
        .into_response()
}

/// One OTLP export: checks the organization token, hands the batch to the
/// extension, drops prompt text and stores each model call.
async fn receive(
    state: &AppState,
    caller: &Caller,
    headers: &HeaderMap,
    body: &[u8],
    signal: Signal,
) -> Result<Response, ApiError> {
    caller.require_credential(&[CredentialKind::Organization])?;
    let header = |name| headers.get(name).and_then(|value| value.to_str().ok());
    let decoded = Encoding::from_content_type(header(CONTENT_TYPE)).and_then(|encoding| {
        otlp::decompress(header(CONTENT_ENCODING), body).map(|raw| (encoding, raw))
    });
    let (encoding, raw) = match decoded {
        Ok(decoded) => decoded,
        Err(error) => return Ok(decode_failure(&error)),
    };
    let organization_id = caller.organization_id.to_string();
    state.extension.forward_telemetry(&TelemetryBatch {
        organization_id: &organization_id,
        signal: match signal {
            Signal::Logs => TelemetrySignal::Logs,
            Signal::Traces => TelemetrySignal::Traces,
            Signal::Metrics => TelemetrySignal::Metrics,
        },
        content_type: encoding.content_type(),
        body: &raw,
    });
    let extracted = match signal {
        Signal::Logs => {
            otlp::decode_logs(&raw, encoding).map(|request| otlp::extract_from_logs(&request))
        }
        Signal::Traces => {
            otlp::decode_traces(&raw, encoding).map(|request| otlp::extract_from_traces(&request))
        }
        // Metrics repeat the calls the events already count.
        Signal::Metrics => otlp::decode_metrics(&raw, encoding).map(|_| Extracted::default()),
    };
    let extracted = match extracted {
        Ok(extracted) => extracted,
        Err(error) => return Ok(decode_failure(&error)),
    };
    let outcome = if extracted.messages.is_empty() {
        TelemetryOutcome::default()
    } else {
        let pricing = prices::current_pricing(&state.db).await?;
        let mut tx = state.db.begin().await?;
        let outcome = store::record_telemetry(
            &mut tx,
            caller.organization_id,
            extracted.messages,
            &pricing,
        )
        .await?;
        tx.commit().await?;
        outcome
    };
    tracing::debug!(
        new = outcome.new_messages,
        known = outcome.known_messages,
        late = outcome.late_messages,
        dropped = extracted.dropped_with_prompt_text,
        stripped = extracted.stripped_prompt_text,
        "telemetry received"
    );
    let rejected = i64::from(extracted.dropped_with_prompt_text);
    let stripped = extracted.stripped_prompt_text;
    let message = match (rejected, stripped) {
        (0, 0) => String::new(),
        (_, 0) => format!(
            "{rejected} events carried prompt text and were dropped; turn off prompt logging in the agent's telemetry settings"
        ),
        (0, _) => format!(
            "{stripped} model calls carried prompt text, which was removed before they were counted; turn off prompt logging in the agent's telemetry settings"
        ),
        (_, _) => format!(
            "{rejected} events carried prompt text and were dropped, and {stripped} model calls had theirs removed before they were counted; turn off prompt logging in the agent's telemetry settings"
        ),
    };
    let (content_type, answer) = otlp::encode_response(signal, rejected, &message, encoding);
    Ok(([(CONTENT_TYPE, content_type)], answer).into_response())
}

/// OTLP logs: one model call per event, for Claude Code, Codex CLI, Gemini
/// CLI and Cursor. Takes the organization token.
#[utoipa::path(
    post,
    path = "/ingest/otlp/v1/logs",
    tag = "telemetry",
    request_body(content = Vec<u8>, content_type = "application/x-protobuf", description = "An OTLP ExportLogsServiceRequest, as protobuf or JSON, optionally gzip-compressed"),
    responses(
        (status = OK, description = "An OTLP ExportLogsServiceResponse"),
        (status = BAD_REQUEST, body = ErrorBody),
        (status = PAYLOAD_TOO_LARGE, body = ErrorBody),
        (status = UNSUPPORTED_MEDIA_TYPE, body = ErrorBody),
    ),
)]
pub(super) async fn otlp_logs(
    State(state): State<AppState>,
    caller: Caller,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    receive(&state, &caller, &headers, &body, Signal::Logs).await
}

/// OTLP traces: one model call per chat span, for Copilot CLI. Takes the
/// organization token.
#[utoipa::path(
    post,
    path = "/ingest/otlp/v1/traces",
    tag = "telemetry",
    request_body(content = Vec<u8>, content_type = "application/x-protobuf", description = "An OTLP ExportTraceServiceRequest, as protobuf or JSON, optionally gzip-compressed"),
    responses(
        (status = OK, description = "An OTLP ExportTraceServiceResponse"),
        (status = BAD_REQUEST, body = ErrorBody),
        (status = PAYLOAD_TOO_LARGE, body = ErrorBody),
        (status = UNSUPPORTED_MEDIA_TYPE, body = ErrorBody),
    ),
)]
pub(super) async fn otlp_traces(
    State(state): State<AppState>,
    caller: Caller,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    receive(&state, &caller, &headers, &body, Signal::Traces).await
}

/// OTLP metrics: accepted and not counted, because the events already count
/// every model call. Takes the organization token.
#[utoipa::path(
    post,
    path = "/ingest/otlp/v1/metrics",
    tag = "telemetry",
    request_body(content = Vec<u8>, content_type = "application/x-protobuf", description = "An OTLP ExportMetricsServiceRequest, as protobuf or JSON, optionally gzip-compressed"),
    responses(
        (status = OK, description = "An OTLP ExportMetricsServiceResponse"),
        (status = BAD_REQUEST, body = ErrorBody),
        (status = PAYLOAD_TOO_LARGE, body = ErrorBody),
        (status = UNSUPPORTED_MEDIA_TYPE, body = ErrorBody),
    ),
)]
pub(super) async fn otlp_metrics(
    State(state): State<AppState>,
    caller: Caller,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    receive(&state, &caller, &headers, &body, Signal::Metrics).await
}
