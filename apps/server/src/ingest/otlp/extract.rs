//! From decoded export requests to one [`TelemetryMessage`] per model call.

use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::any_value::Value;
use opentelemetry_proto::tonic::common::v1::{AnyValue, KeyValue};
use opentelemetry_proto::tonic::logs::v1::LogRecord;
use opentelemetry_proto::tonic::metrics::v1::metric::Data;
use opentelemetry_proto::tonic::metrics::v1::{NumberDataPoint, number_data_point};
use opentelemetry_proto::tonic::trace::v1::Span;
use sha2::{Digest as _, Sha256};
use time::OffsetDateTime;

use super::fields::Fields;
use super::harness::{self, Door};
use super::privacy::{body_carries_prompt_text, carries_prompt_text, without_prompt_text};
use crate::ingest::model::{TelemetryActivity, TelemetryHarness, TelemetryMessage, TokenCounts};

/// Where the person who made a call is named, in order.
const EMAIL_ATTRIBUTES: &[&str] = &["user.email", "cursor.user.email"];

/// Where the account of the person who made a call is named, in order.
const ACCOUNT_ATTRIBUTES: &[&str] = &[
    "user.account_uuid",
    "cursor.user.account_id",
    "user.id",
    "enduser.id",
    "enduser.pseudo.id",
];

/// Where the repository is named, in order: the OpenTelemetry VCS
/// convention `Claude Code` sends, then names teams set themselves through
/// `OTEL_RESOURCE_ATTRIBUTES`.
const REPOSITORY_ATTRIBUTES: &[&str] = &[
    "vcs.repository.url.full",
    "repository",
    "repository.url",
    "git.repository",
    "git.repository.url",
    "repo",
    "github.copilot.git.repository",
];

/// Attributes that only route an event, read before the agent harness is
/// known, and so never become tags.
const ROUTING_ATTRIBUTES: &[&str] = &["event.name", "event.timestamp", "event.sequence"];

/// Kinds of records that only repeat a stream, many per model call, and
/// become no activity: Codex CLI's streaming events. They stay in the
/// received record.
const STREAM_KINDS: &[&str] = &["sse_event", "websocket_event"];

/// Where a record that is no model call names its session, in order.
const SESSION_ATTRIBUTES: &[&str] = &[
    "session.id",
    "conversation.id",
    "gen_ai.conversation.id",
    "prompt.id",
];

/// Where an agent reports the route it reached the model by, which names
/// the provider (ADR 0028): `Claude Code`'s `anthropic.provider`, Gemini
/// CLI's `auth_type` and the `GenAI` convention Copilot CLI writes. They
/// stay on the message as signals too.
const PROVIDER_ATTRIBUTES: &[&str] = &["anthropic.provider", "auth_type", "gen_ai.provider.name"];

/// What an export request holds once content text was removed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Extracted {
    /// One per model call, in the order of the request.
    pub messages: Vec<TelemetryMessage>,
    /// One per record of an agent that is no model call, such as a tool
    /// run or a counter.
    pub activities: Vec<TelemetryActivity>,
    /// Records that carried content text, which was removed before
    /// anything was read from them (ADR 0028).
    pub content_removed: u32,
    /// Records of no agent harness, of another door than their agent's
    /// model calls, streaming events, and model calls without a session or
    /// with token counts that cannot be read.
    pub ignored: u32,
}

/// The model calls in a logs export request: `Claude Code`, Codex CLI,
/// Gemini CLI and `Cursor` send one log event per call.
#[must_use]
pub fn extract_from_logs(request: &ExportLogsServiceRequest) -> Extracted {
    let mut extracted = Extracted::default();
    for resource_logs in &request.resource_logs {
        let resource = resource_attributes(resource_logs.resource.as_ref());
        for scope_logs in &resource_logs.scope_logs {
            let scope = scope_logs
                .scope
                .as_ref()
                .map_or("", |scope| scope.name.as_str());
            for log in &scope_logs.log_records {
                extracted.add(&Record::log(resource, scope, log));
            }
        }
    }
    extracted
}

/// The model calls in a traces export request: Copilot CLI writes one `chat`
/// span per call. A span counts as one item, its events included.
#[must_use]
pub fn extract_from_traces(request: &ExportTraceServiceRequest) -> Extracted {
    let mut extracted = Extracted::default();
    for resource_spans in &request.resource_spans {
        let resource = resource_attributes(resource_spans.resource.as_ref());
        for scope_spans in &resource_spans.scope_spans {
            let scope = scope_spans
                .scope
                .as_ref()
                .map_or("", |scope| scope.name.as_str());
            for span in &scope_spans.spans {
                extracted.add(&Record::span(resource, scope, span));
            }
        }
    }
    extracted
}

/// Metric names that repeat what model calls already count.
const COUNTED_BY_MESSAGES: &[&str] = &["token.usage", "cost.usage"];

/// The counters in a metrics export request of a known agent, as
/// activities: one per data point of a sum or gauge that is not zero, such
/// as `claude_code.commit.count` or `claude_code.pull_request.count`, with
/// its value as the signal `value`. Token and cost counters repeat the
/// model calls and are left out; so are histograms.
#[must_use]
pub fn extract_from_metrics(request: &ExportMetricsServiceRequest) -> Extracted {
    let mut extracted = Extracted::default();
    for resource_metrics in &request.resource_metrics {
        let resource = resource_attributes(resource_metrics.resource.as_ref());
        let service_name = resource
            .iter()
            .find(|attribute| attribute.key == "service.name")
            .and_then(|attribute| string_value(attribute.value.as_ref()));
        for scope_metrics in &resource_metrics.scope_metrics {
            let scope = scope_metrics
                .scope
                .as_ref()
                .map_or("", |scope| scope.name.as_str());
            for metric in &scope_metrics.metrics {
                let points = match &metric.data {
                    Some(Data::Sum(sum)) => sum.data_points.as_slice(),
                    Some(Data::Gauge(gauge)) => gauge.data_points.as_slice(),
                    _ => &[],
                };
                let harness = harness::detect(&[metric.name.as_str()], scope, service_name);
                let counted = COUNTED_BY_MESSAGES
                    .iter()
                    .any(|name| metric.name.ends_with(name));
                for point in points {
                    match harness
                        .filter(|_| !counted)
                        .and_then(|harness| counter(harness, &metric.name, resource, point))
                    {
                        Some(activity) => extracted.activities.push(activity),
                        None => extracted.ignored = extracted.ignored.saturating_add(1),
                    }
                }
            }
        }
    }
    extracted
}

/// One data point as an activity, unless it is zero or has no time.
fn counter(
    harness: TelemetryHarness,
    name: &str,
    resource: &[KeyValue],
    point: &NumberDataPoint,
) -> Option<TelemetryActivity> {
    let value = match point.value? {
        number_data_point::Value::AsInt(number) => (number != 0).then(|| number.to_string()),
        number_data_point::Value::AsDouble(number) => {
            (number != 0.0 && number.is_finite()).then(|| format!("{number}"))
        }
    }?;
    let occurred_at = unix_nanos(point.time_unix_nano)?;
    let mut fields = Fields::new(resource, &point.attributes);
    let session_id = fields.take_text(SESSION_ATTRIBUTES);
    let person_email = fields
        .take_text(EMAIL_ATTRIBUTES)
        .map(|email| email.to_lowercase());
    let mut attributes = fields.into_attributes();
    attributes.insert("value".to_owned(), value);
    Some(TelemetryActivity {
        harness,
        kind: name.to_owned(),
        session_id,
        occurred_at,
        person_email,
        attributes,
    })
}

impl Extracted {
    fn add(&mut self, record: &Record<'_>) {
        let (outcome, removed) = record.outcome();
        if removed {
            self.content_removed = self.content_removed.saturating_add(1);
        }
        match outcome {
            Outcome::Message(message) => self.messages.push(*message),
            Outcome::Activity(activity) => self.activities.push(*activity),
            Outcome::Ignored => self.ignored = self.ignored.saturating_add(1),
        }
    }
}

enum Outcome {
    Message(Box<TelemetryMessage>),
    Activity(Box<TelemetryActivity>),
    Ignored,
}

/// A log record or span, seen the same way.
struct Record<'a> {
    door: Door,
    resource: &'a [KeyValue],
    scope: &'a str,
    attributes: &'a [KeyValue],
    /// Attribute lists that belong to the record too: a span's events.
    inner_attributes: Vec<&'a [KeyValue]>,
    body: Option<&'a AnyValue>,
    /// The event name attribute, the log record's event name and a string
    /// body, or the span name.
    names: Vec<&'a str>,
    /// The record's own time in Unix nanos, then a fallback after the
    /// `event.timestamp` attribute; zero when unset.
    time: u64,
    fallback_time: u64,
    trace_id: &'a [u8],
}

impl<'a> Record<'a> {
    fn log(resource: &'a [KeyValue], scope: &'a str, log: &'a LogRecord) -> Self {
        let event_name = log
            .attributes
            .iter()
            .find(|attribute| attribute.key == "event.name");
        let body_text = match log.body.as_ref().and_then(|body| body.value.as_ref()) {
            Some(Value::StringValue(text)) => Some(text.as_str()),
            _ => None,
        };
        let names = [
            event_name.and_then(|attribute| string_value(attribute.value.as_ref())),
            Some(log.event_name.as_str()),
            body_text,
        ]
        .into_iter()
        .flatten()
        .filter(|name| !name.is_empty())
        .collect();
        Self {
            door: Door::Logs,
            resource,
            scope,
            attributes: &log.attributes,
            inner_attributes: Vec::new(),
            body: log.body.as_ref(),
            names,
            time: log.time_unix_nano,
            fallback_time: log.observed_time_unix_nano,
            trace_id: &log.trace_id,
        }
    }

    fn span(resource: &'a [KeyValue], scope: &'a str, span: &'a Span) -> Self {
        Self {
            door: Door::Spans,
            resource,
            scope,
            attributes: &span.attributes,
            inner_attributes: span
                .events
                .iter()
                .map(|event| event.attributes.as_slice())
                .collect(),
            body: None,
            names: vec![span.name.as_str()],
            time: span.end_time_unix_nano,
            fallback_time: span.start_time_unix_nano,
            trace_id: &span.trace_id,
        }
    }

    /// What the record is once its content text is removed, and whether
    /// it carried any.
    fn outcome(&self) -> (Outcome, bool) {
        let attribute_lists = [self.attributes]
            .into_iter()
            .chain(self.inner_attributes.iter().copied());
        if carries_prompt_text(attribute_lists, self.body, &self.names) {
            return (self.without_content_text(), true);
        }
        (self.read(), false)
    }

    /// The record read from what carries no content text: the attributes
    /// without those that carry some, no body, no span events, and no name
    /// taken from a body that is text.
    fn without_content_text(&self) -> Outcome {
        let attributes = without_prompt_text(self.attributes);
        let body_is_text = body_carries_prompt_text(self.body, &self.names);
        let body_text = self.body.and_then(|body| string_value(Some(body)));
        let names = self
            .names
            .iter()
            .copied()
            .filter(|name| !(body_is_text && Some(*name) == body_text))
            .collect();
        Record {
            attributes: &attributes,
            inner_attributes: Vec::new(),
            body: None,
            names,
            ..*self
        }
        .read()
    }

    /// A model call; else an activity of a known agent on its own door;
    /// else nothing.
    fn read(&self) -> Outcome {
        if let Some(message) = self.message() {
            return Outcome::Message(Box::new(message));
        }
        self.activity().map_or(Outcome::Ignored, |activity| {
            Outcome::Activity(Box::new(activity))
        })
    }

    fn harness(&self) -> Option<TelemetryHarness> {
        let service_name = self
            .resource
            .iter()
            .find(|attribute| attribute.key == "service.name")
            .and_then(|attribute| string_value(attribute.value.as_ref()));
        let harness = harness::detect(&self.names, self.scope, service_name)?;
        (harness::door(harness) == self.door).then_some(harness)
    }

    /// The record as an activity: its kind is its most specific name, such
    /// as `claude_code.tool_result`.
    fn activity(&self) -> Option<TelemetryActivity> {
        let harness = self.harness()?;
        let kind = self
            .names
            .iter()
            .copied()
            .find(|name| name.contains('.') && !name.contains(' '))
            .or_else(|| self.names.first().copied())?;
        if STREAM_KINDS.iter().any(|stream| kind.ends_with(stream)) {
            return None;
        }
        let mut fields = Fields::new(self.resource, self.attributes);
        if harness::is_named_as_call(harness, &self.names, &fields) {
            return None;
        }
        let occurred_at = unix_nanos(self.time)
            .or_else(|| fields.take_timestamp("event.timestamp"))
            .or_else(|| unix_nanos(self.fallback_time))?;
        let session_id = fields
            .take_text(SESSION_ATTRIBUTES)
            .or_else(|| trace_id(self.trace_id));
        let person_email = fields
            .take_text(EMAIL_ATTRIBUTES)
            .map(|email| email.to_lowercase());
        fields.discard(ROUTING_ATTRIBUTES);
        Some(TelemetryActivity {
            harness,
            kind: kind.to_owned(),
            session_id,
            occurred_at,
            person_email,
            attributes: fields.into_attributes(),
        })
    }

    fn message(&self) -> Option<TelemetryMessage> {
        let harness = self.harness()?;
        let mut fields = Fields::new(self.resource, self.attributes);
        let call = harness::read_call(harness, &self.names, &mut fields)?;
        let occurred_at = unix_nanos(self.time)
            .or_else(|| fields.take_timestamp("event.timestamp"))
            .or_else(|| unix_nanos(self.fallback_time))?;
        let session_id = call
            .session_id
            .or_else(|| fields.take_text(&["prompt.id"]))
            .or_else(|| trace_id(self.trace_id))?;
        let message_id = call.message_id.unwrap_or_else(|| {
            synthetic_message_id(&session_id, occurred_at, &call.model, &call.tokens)
        });
        let person_email = fields
            .take_text(EMAIL_ATTRIBUTES)
            .map(|email| email.to_lowercase());
        let person_account_id = fields.take_text(ACCOUNT_ATTRIBUTES);
        let repository = take_repository(&mut fields);
        let provider = PROVIDER_ATTRIBUTES
            .iter()
            .find_map(|key| reported_provider(key, fields.peek_text(key)?));
        fields.discard(ROUTING_ATTRIBUTES);
        Some(TelemetryMessage {
            harness,
            provider,
            session_id,
            message_id,
            model: call.model,
            occurred_at,
            tokens: call.tokens,
            reported_cost: call.reported_cost,
            person_email,
            person_account_id,
            repository,
            attributes: fields.into_attributes(),
        })
    }
}

/// The provider a reported route names, spelled as bills spell it. A
/// route that names no provider we know gives none; it stays a signal.
fn reported_provider(key: &str, route: &str) -> Option<String> {
    let provider = match (key, route.trim()) {
        ("anthropic.provider", "firstParty") => "anthropic",
        ("anthropic.provider", "bedrock") => "aws_bedrock",
        ("anthropic.provider", "vertex") | ("auth_type", "vertex-ai") => "gcp_vertex",
        ("anthropic.provider", "foundry") => "azure_foundry",
        ("auth_type", "gemini-api-key" | "oauth-personal") => "google",
        ("gen_ai.provider.name", name) => {
            return crate::ingest::provider::PROVIDERS
                .iter()
                .find(|known| **known == name)
                .map(|known| (*known).to_owned());
        }
        _ => return None,
    };
    Some(provider.to_owned())
}

fn resource_attributes(
    resource: Option<&opentelemetry_proto::tonic::resource::v1::Resource>,
) -> &[KeyValue] {
    resource
        .map(|resource| resource.attributes.as_slice())
        .unwrap_or_default()
}

fn string_value(value: Option<&AnyValue>) -> Option<&str> {
    match value?.value.as_ref()? {
        Value::StringValue(text) => Some(text.as_str()),
        _ => None,
    }
}

/// A time in Unix nanos, where zero means unset.
fn unix_nanos(nanos: u64) -> Option<OffsetDateTime> {
    if nanos == 0 {
        return None;
    }
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(nanos)).ok()
}

/// A trace ID as hex, unless unset or all zeros.
fn trace_id(bytes: &[u8]) -> Option<String> {
    bytes
        .iter()
        .any(|byte| *byte != 0)
        .then(|| hex::encode(bytes))
}

/// A message ID for a call the agent harness gives none: the same call in a
/// retried export gets the same ID, so it is stored once.
fn synthetic_message_id(
    session_id: &str,
    occurred_at: OffsetDateTime,
    model: &str,
    tokens: &TokenCounts,
) -> String {
    let mut hasher = Sha256::new();
    for part in [
        session_id.to_owned(),
        occurred_at.unix_timestamp_nanos().to_string(),
        model.to_owned(),
        tokens
            .get(&tokencontroller_core::TokenKind::INPUT)
            .to_string(),
        tokens
            .get(&tokencontroller_core::TokenKind::OUTPUT)
            .to_string(),
        tokens
            .get(&tokencontroller_core::TokenKind::CACHE_WRITE)
            .to_string(),
        tokens
            .get(&tokencontroller_core::TokenKind::CACHE_READ)
            .to_string(),
    ] {
        hasher.update(part.as_bytes());
        // A separator, so parts cannot run into each other.
        hasher.update([0x1f]);
    }
    let digest = hasher.finalize();
    let short: Vec<u8> = digest.iter().copied().take(16).collect();
    format!("otel:{}", hex::encode(short))
}

/// The first repository attribute that reads as one, taken.
fn take_repository(fields: &mut Fields<'_>) -> Option<String> {
    REPOSITORY_ATTRIBUTES.iter().find_map(|key| {
        let repository = normalize_repository(fields.peek_text(key)?)?;
        fields.discard(&[key]);
        Some(repository)
    })
}

/// A repository as `host/owner/name`, lowercased: from an HTTPS or SSH URL,
/// an `scp`-style `git@host:owner/name` or a bare path, without scheme,
/// credentials, port, query, trailing slash or `.git`. `None` unless there is
/// a host and at least two path segments.
fn normalize_repository(raw: &str) -> Option<String> {
    let text = raw.trim().to_lowercase();
    let without_scheme = text
        .split_once("://")
        .map_or(text.as_str(), |(_, rest)| rest);
    let without_query = without_scheme.split(['?', '#']).next().unwrap_or_default();
    let (authority, path) = without_query.split_once('/').unwrap_or((without_query, ""));
    let host_and_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    // `host:port` keeps the host; `host:owner` (scp style) starts the path.
    let is_port = |after: &str| after.chars().all(|c| c.is_ascii_digit());
    let (host, path) = match host_and_port.split_once(':') {
        Some((host, after)) if is_port(after) => (host, path.to_owned()),
        Some((host, after)) => (host, format!("{after}/{path}")),
        None => (host_and_port, path.to_owned()),
    };
    let mut segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    if let Some(last) = segments.pop() {
        let last = last.strip_suffix(".git").unwrap_or(last);
        if !last.is_empty() {
            segments.push(last);
        }
    }
    if host.is_empty() || segments.len() < 2 {
        return None;
    }
    Some(format!("{host}/{}", segments.join("/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repositories_read_from_every_common_form() {
        for (raw, expected) in [
            (
                "https://github.com/Acme/Storefront",
                "github.com/acme/storefront",
            ),
            (
                "https://github.com/acme/storefront.git",
                "github.com/acme/storefront",
            ),
            (
                "https://github.com/acme/storefront/",
                "github.com/acme/storefront",
            ),
            (
                "https://token:x@github.com/acme/storefront.git?ref=main#readme",
                "github.com/acme/storefront",
            ),
            (
                "git@github.com:acme/storefront.git",
                "github.com/acme/storefront",
            ),
            (
                "ssh://git@gitlab.example.com:2222/platform/web/api.git",
                "gitlab.example.com/platform/web/api",
            ),
            ("github.com/acme/storefront", "github.com/acme/storefront"),
            (
                "https://dev.azure.com/acme/shop/_git/storefront",
                "dev.azure.com/acme/shop/_git/storefront",
            ),
        ] {
            assert_eq!(
                normalize_repository(raw).as_deref(),
                Some(expected),
                "{raw}"
            );
        }
        for raw in [
            "",
            "storefront",
            "acme/storefront",
            "https://github.com/acme",
            "https://github.com/acme/.git",
        ] {
            assert_eq!(normalize_repository(raw), None, "{raw}");
        }
    }
}
