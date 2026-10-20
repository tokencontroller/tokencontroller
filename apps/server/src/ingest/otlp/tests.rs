// The fixtures follow each agent harness's own telemetry reference:
// - `Claude Code`: https://code.claude.com/docs/en/monitoring-usage (events,
//   standard attributes, service information, traces)
// - Codex CLI: https://developers.openai.com/codex/config-advanced and
//   codex-rs/otel/src/events/session_telemetry.rs in github.com/openai/codex
// - Gemini CLI: docs/cli/telemetry.md and packages/core/src/telemetry/types.ts
//   in github.com/google-gemini/gemini-cli
// - Copilot CLI: https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-command-reference#opentelemetry-monitoring
// - Cursor: https://cursor.com/docs/enterprise/opentelemetry-export/wire
// - OTLP/HTTP and its JSON encoding: https://opentelemetry.io/docs/specs/otlp/

use std::collections::BTreeMap;
use std::error::Error;
use std::io::Write as _;

use flate2::Compression;
use flate2::write::GzEncoder;
use opentelemetry_proto::tonic::collector::logs::v1::{
    ExportLogsServiceRequest, ExportLogsServiceResponse,
};
use opentelemetry_proto::tonic::collector::metrics::v1::{
    ExportMetricsServiceRequest, ExportMetricsServiceResponse,
};
use opentelemetry_proto::tonic::collector::trace::v1::{
    ExportTraceServiceRequest, ExportTraceServiceResponse,
};
use opentelemetry_proto::tonic::common::v1::any_value::Value;
use opentelemetry_proto::tonic::common::v1::{
    AnyValue, ArrayValue, InstrumentationScope, KeyValue, KeyValueList,
};
use opentelemetry_proto::tonic::logs::v1::{LogRecord, ResourceLogs, ScopeLogs};
use opentelemetry_proto::tonic::resource::v1::Resource;
use opentelemetry_proto::tonic::trace::v1::{ResourceSpans, ScopeSpans, Span};
use prost::Message as _;
use time::macros::datetime;
use tokencontroller_core::{Currency, Money};

use super::*;
use crate::ingest::model::{TelemetryHarness, TelemetryMessage, TokenCounts};

const CLAUDE_CODE_LOGS: &str = include_str!("../../../tests/fixtures/otlp/claude_code_logs.json");
const CODEX_CLI_LOGS: &str = include_str!("../../../tests/fixtures/otlp/codex_cli_logs.json");
const COPILOT_CLI_TRACES: &str =
    include_str!("../../../tests/fixtures/otlp/copilot_cli_traces.json");
const CLAUDE_CODE_METRICS: &str =
    include_str!("../../../tests/fixtures/otlp/claude_code_metrics.json");

// Builders for protobuf requests, from the generated structs.

fn any(value: Value) -> AnyValue {
    AnyValue { value: Some(value) }
}

fn attribute(key: &str, value: Value) -> KeyValue {
    KeyValue {
        key: key.to_owned(),
        value: Some(any(value)),
        key_strindex: 0,
    }
}

fn text(key: &str, value: &str) -> KeyValue {
    attribute(key, Value::StringValue(value.to_owned()))
}

fn int(key: &str, value: i64) -> KeyValue {
    attribute(key, Value::IntValue(value))
}

fn log(time_unix_nano: u64, body: &str, attributes: Vec<KeyValue>) -> LogRecord {
    LogRecord {
        time_unix_nano,
        observed_time_unix_nano: time_unix_nano,
        severity_number: 9,
        body: Some(any(Value::StringValue(body.to_owned()))),
        attributes,
        ..LogRecord::default()
    }
}

fn logs_request(
    resource: Vec<KeyValue>,
    scope: &str,
    log_records: Vec<LogRecord>,
) -> ExportLogsServiceRequest {
    ExportLogsServiceRequest {
        resource_logs: vec![ResourceLogs {
            resource: Some(Resource {
                attributes: resource,
                ..Resource::default()
            }),
            scope_logs: vec![ScopeLogs {
                scope: Some(InstrumentationScope {
                    name: scope.to_owned(),
                    ..InstrumentationScope::default()
                }),
                log_records,
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }],
    }
}

fn span(name: &str, end_time_unix_nano: u64, attributes: Vec<KeyValue>) -> Span {
    Span {
        trace_id: vec![
            0x4b, 0xf9, 0x2f, 0x35, 0x77, 0xb3, 0x4d, 0xa6, 0xa3, 0xce, 0x92, 0x9d, 0x0e, 0x0e,
            0x47, 0x36,
        ],
        span_id: vec![0x00, 0xf0, 0x67, 0xaa, 0x0b, 0xa9, 0x02, 0xb7],
        name: name.to_owned(),
        kind: 3,
        start_time_unix_nano: 1_791_205_500_000_000_000,
        end_time_unix_nano,
        attributes,
        ..Span::default()
    }
}

fn traces_request(
    resource: Vec<KeyValue>,
    scope: &str,
    spans: Vec<Span>,
) -> ExportTraceServiceRequest {
    ExportTraceServiceRequest {
        resource_spans: vec![ResourceSpans {
            resource: Some(Resource {
                attributes: resource,
                ..Resource::default()
            }),
            scope_spans: vec![ScopeSpans {
                scope: Some(InstrumentationScope {
                    name: scope.to_owned(),
                    ..InstrumentationScope::default()
                }),
                spans,
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }],
    }
}

/// Sends a request through protobuf and back, as the endpoint receives it.
fn through_protobuf_logs(
    request: &ExportLogsServiceRequest,
) -> Result<ExportLogsServiceRequest, Box<dyn Error>> {
    Ok(decode_logs(&request.encode_to_vec(), Encoding::Protobuf)?)
}

fn through_protobuf_traces(
    request: &ExportTraceServiceRequest,
) -> Result<ExportTraceServiceRequest, Box<dyn Error>> {
    Ok(decode_traces(&request.encode_to_vec(), Encoding::Protobuf)?)
}

fn gzip(body: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(body)?;
    Ok(encoder.finish()?)
}

fn tags(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

fn usd(nanos: i64) -> Money {
    Money::from_nanos(nanos, Currency::USD)
}

// The wire.

#[test]
fn content_types_pick_the_encoding() {
    for (header, expected) in [
        (None, Encoding::Protobuf),
        (Some(""), Encoding::Protobuf),
        (Some("application/x-protobuf"), Encoding::Protobuf),
        (Some("Application/X-Protobuf"), Encoding::Protobuf),
        (Some("application/json"), Encoding::Json),
        (Some("application/json; charset=utf-8"), Encoding::Json),
        (Some(" APPLICATION/JSON ;charset=UTF-8"), Encoding::Json),
    ] {
        assert_eq!(
            Encoding::from_content_type(header).unwrap(),
            expected,
            "{header:?}"
        );
    }
    assert_eq!(Encoding::Protobuf.content_type(), "application/x-protobuf");
    assert_eq!(Encoding::Json.content_type(), "application/json");
}

#[test]
fn other_content_types_are_refused() {
    for header in [
        "text/plain",
        "application/grpc",
        "application/protobuf; x=1",
        "application/jsonl",
    ] {
        let error = Encoding::from_content_type(Some(header)).unwrap_err();
        assert!(
            matches!(&error, DecodeError::UnsupportedContentType(named) if named == header),
            "{header}: {error}"
        );
    }
}

#[test]
fn bodies_pass_through_or_are_inflated() {
    let body = CODEX_CLI_LOGS.as_bytes();
    for encoding in [None, Some(""), Some("identity"), Some("IDENTITY")] {
        assert_eq!(decompress(encoding, body).unwrap(), body);
    }
    let compressed = gzip(body).unwrap();
    for encoding in ["gzip", "GZIP", " gzip ", "x-gzip"] {
        assert_eq!(decompress(Some(encoding), &compressed).unwrap(), body);
    }
    // Two gzip members one after the other are one body.
    let mut two_members = gzip(b"{\"resourceLogs\":").unwrap();
    two_members.extend(gzip(b"[]}").unwrap());
    assert_eq!(
        decompress(Some("gzip"), &two_members).unwrap(),
        b"{\"resourceLogs\":[]}"
    );
}

#[test]
fn other_encodings_and_broken_gzip_are_refused() {
    for encoding in ["deflate", "br", "zstd", "gzip, identity"] {
        let error = decompress(Some(encoding), b"{}").unwrap_err();
        assert!(
            matches!(error, DecodeError::UnsupportedContentEncoding(_)),
            "{encoding}"
        );
    }
    let error = decompress(Some("gzip"), b"not gzip at all").unwrap_err();
    assert!(matches!(error, DecodeError::Gzip(_)), "{error}");
}

#[test]
fn bodies_over_the_limit_are_refused() {
    let at_limit = vec![b' '; MAX_BODY_BYTES];
    assert_eq!(decompress(None, &at_limit).unwrap().len(), MAX_BODY_BYTES);
    let over = vec![b' '; MAX_BODY_BYTES.checked_add(1).unwrap()];
    assert!(matches!(
        decompress(None, &over),
        Err(DecodeError::TooLarge)
    ));

    // A small body that inflates past the limit stops at the limit.
    let bomb = gzip(&[0_u8; 4096]).unwrap();
    assert!(bomb.len() < 100);
    assert!(matches!(
        wire_limit(Some("gzip"), &bomb, 4095),
        Err(DecodeError::TooLarge)
    ));
    assert_eq!(wire_limit(Some("gzip"), &bomb, 4096).unwrap().len(), 4096);
    assert!(matches!(
        wire_limit(None, &[0; 11], 10),
        Err(DecodeError::TooLarge)
    ));
}

fn wire_limit(encoding: Option<&str>, body: &[u8], limit: usize) -> Result<Vec<u8>, DecodeError> {
    wire::decompress_up_to(encoding, body, limit)
}

#[test]
fn every_signal_decodes_from_json_and_protobuf() {
    let logs = decode_logs(CLAUDE_CODE_LOGS.as_bytes(), Encoding::Json).unwrap();
    assert_eq!(logs.resource_logs[0].scope_logs[0].log_records.len(), 7);
    assert_eq!(
        decode_logs(&logs.encode_to_vec(), Encoding::Protobuf).unwrap(),
        logs
    );

    let traces = decode_traces(COPILOT_CLI_TRACES.as_bytes(), Encoding::Json).unwrap();
    let first = &traces.resource_spans[0].scope_spans[0].spans[0];
    assert_eq!(
        hex::encode(&first.trace_id),
        "4bf92f3577b34da6a3ce929d0e0e4736"
    );
    assert_eq!(hex::encode(&first.span_id), "00f067aa0ba902b7");
    assert_eq!(first.kind, 3);
    assert_eq!(first.end_time_unix_nano, 1_791_205_507_777_000_000);
    assert_eq!(
        decode_traces(&traces.encode_to_vec(), Encoding::Protobuf).unwrap(),
        traces
    );

    let metrics = decode_metrics(CLAUDE_CODE_METRICS.as_bytes(), Encoding::Json).unwrap();
    let names: Vec<&str> = metrics.resource_metrics[0].scope_metrics[0]
        .metrics
        .iter()
        .map(|metric| metric.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "claude_code.token.usage",
            "claude_code.cost.usage",
            "claude_code.session.count"
        ]
    );
    assert_eq!(
        decode_metrics(&metrics.encode_to_vec(), Encoding::Protobuf).unwrap(),
        metrics
    );
    assert_eq!(
        decode_metrics(b"", Encoding::Protobuf).unwrap(),
        ExportMetricsServiceRequest::default()
    );
}

#[test]
fn broken_bodies_are_named() {
    assert!(matches!(
        decode_logs(b"\xff\xff\xff", Encoding::Protobuf),
        Err(DecodeError::Protobuf(_))
    ));
    assert!(matches!(
        decode_traces(b"{\"resourceSpans\":", Encoding::Json),
        Err(DecodeError::Json(_))
    ));
    assert!(matches!(
        decode_metrics(b"\"metrics\"", Encoding::Json),
        Err(DecodeError::Json(_))
    ));
    // A protobuf body sent as JSON is refused, not misread.
    let protobuf = decode_logs(CODEX_CLI_LOGS.as_bytes(), Encoding::Json)
        .unwrap()
        .encode_to_vec();
    assert!(matches!(
        decode_logs(&protobuf, Encoding::Json),
        Err(DecodeError::Json(_))
    ));
}

#[test]
fn collector_json_that_the_generated_types_refuse_is_repaired() {
    let body = r#"{"resourceLogs":[{"scopeLogs":[{"logRecords":[{"attributes":[
        {"key":"empty.list","value":{"arrayValue":{}}},
        {"key":"empty.map","value":{"kvlistValue":{}}},
        {"key":"ratio","value":{"doubleValue":"0.25"}},
        {"key":"broken","value":{"doubleValue":"NaN"}},
        {"key":"nested","value":{"arrayValue":{"values":[{"kvlistValue":{}}]}}}
    ]}]}]}]}"#;
    let request = decode_logs(body.as_bytes(), Encoding::Json).unwrap();
    let values: Vec<Option<Value>> = request.resource_logs[0].scope_logs[0].log_records[0]
        .attributes
        .iter()
        .map(|attribute| attribute.value.clone().and_then(|value| value.value))
        .collect();
    assert_eq!(
        values,
        [
            Some(Value::ArrayValue(ArrayValue { values: Vec::new() })),
            Some(Value::KvlistValue(KeyValueList { values: Vec::new() })),
            Some(Value::DoubleValue(0.25)),
            None,
            Some(Value::ArrayValue(ArrayValue {
                values: vec![AnyValue {
                    value: Some(Value::KvlistValue(KeyValueList { values: Vec::new() }))
                }]
            })),
        ]
    );
    // A body that is still wrong after the repair keeps its first error.
    let error = decode_logs(br#"{"resourceLogs":[{"scopeLogs":[{"logRecords":[{"attributes":[{"key":"a","value":{"arrayValue":{}}}],"timeUnixNano":"soon"}]}]}]}"#, Encoding::Json).unwrap_err();
    assert!(
        error.to_string().contains("missing field `values`"),
        "{error}"
    );
}

#[test]
fn responses_report_partial_success_only_when_rejecting() {
    let (content_type, body) = encode_response(Signal::Logs, 0, "", Encoding::Protobuf);
    assert_eq!(content_type, "application/x-protobuf");
    assert_eq!(
        ExportLogsServiceResponse::decode(body.as_slice())
            .unwrap()
            .partial_success,
        None
    );

    let (_, body) = encode_response(
        Signal::Logs,
        3,
        "3 events carried prompt text",
        Encoding::Protobuf,
    );
    let partial = ExportLogsServiceResponse::decode(body.as_slice())
        .unwrap()
        .partial_success
        .unwrap();
    assert_eq!(
        (partial.rejected_log_records, partial.error_message.as_str()),
        (3, "3 events carried prompt text")
    );

    let (_, body) = encode_response(Signal::Traces, 2, "dropped", Encoding::Protobuf);
    let partial = ExportTraceServiceResponse::decode(body.as_slice())
        .unwrap()
        .partial_success
        .unwrap();
    assert_eq!(
        (partial.rejected_spans, partial.error_message.as_str()),
        (2, "dropped")
    );

    let (_, body) = encode_response(Signal::Metrics, 1, "bad", Encoding::Protobuf);
    let partial = ExportMetricsServiceResponse::decode(body.as_slice())
        .unwrap()
        .partial_success
        .unwrap();
    assert_eq!(partial.rejected_data_points, 1);

    for signal in [Signal::Logs, Signal::Traces, Signal::Metrics] {
        let (content_type, body) = encode_response(signal, 0, "", Encoding::Json);
        assert_eq!(
            (content_type, body.as_slice()),
            ("application/json", b"{}".as_slice())
        );
        let (_, body) = encode_response(signal, -1, "", Encoding::Protobuf);
        assert!(body.is_empty());
    }
    // A message with nothing rejected is a warning.
    let (_, body) = encode_response(Signal::Logs, 0, "text removed", Encoding::Json);
    assert_eq!(
        body,
        br#"{"partialSuccess":{"errorMessage":"text removed","rejectedLogRecords":0}}"#
    );
    let (_, body) = encode_response(Signal::Traces, 0, "text removed", Encoding::Protobuf);
    let partial = ExportTraceServiceResponse::decode(body.as_slice())
        .unwrap()
        .partial_success
        .unwrap();
    assert_eq!(
        (partial.rejected_spans, partial.error_message.as_str()),
        (0, "text removed")
    );
    let (_, body) = encode_response(
        Signal::Logs,
        3,
        "3 events carried prompt text",
        Encoding::Json,
    );
    let decoded: ExportLogsServiceResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(decoded.partial_success.unwrap().rejected_log_records, 3);
    let (_, body) = encode_response(Signal::Traces, 2, "", Encoding::Json);
    assert_eq!(body, br#"{"partialSuccess":{"rejectedSpans":2}}"#);
    let (_, body) = encode_response(Signal::Metrics, 5, "no", Encoding::Json);
    assert_eq!(
        body,
        br#"{"partialSuccess":{"errorMessage":"no","rejectedDataPoints":5}}"#
    );
}

// Claude Code.

fn claude_code_sonnet_call() -> TelemetryMessage {
    TelemetryMessage {
        harness: TelemetryHarness::ClaudeCode,
        session_id: "6d1f7a2e-3b4c-4d5e-8f90-1a2b3c4d5e6f".to_owned(),
        message_id: "req_011CXa8bnL9xkqW5tHzP3mYe".to_owned(),
        model: "claude-sonnet-5".to_owned(),
        occurred_at: datetime!(2026-10-05 09:14:03.512 UTC),
        tokens: TokenCounts {
            input: 1204,
            output: 1873,
            cache_write: 3170,
            cache_read: 48211,
        },
        reported_cost: Some(usd(42_391_500)),
        person_email: Some("ana.silva@acme.example".to_owned()),
        person_account_id: Some("a1b2c3d4-e5f6-4a7b-8c9d-0e1f2a3b4c5d".to_owned()),
        repository: Some("github.com/acme/storefront".to_owned()),
        attributes: tags(&[
            ("client_request_id", "7c9e6679-7425-40de-944b-e07fc1f90ae7"),
            ("department", "engineering"),
            ("duration_ms", "8123"),
            ("effort", "high"),
            ("host.arch", "arm64"),
            ("organization.id", "0b2c4e6a-8d1f-4a3b-9c5e-7f1a3b5d7e9c"),
            ("os.type", "darwin"),
            ("os.version", "25.0.0"),
            ("prompt.id", "9a7c5e3b-1d2f-4e6a-8b0c-2d4f6a8c0e1b"),
            ("query_source", "repl_main_thread"),
            ("service.name", "claude-code"),
            ("service.version", "2.1.290"),
            ("speed", "normal"),
            ("team", "web"),
            ("terminal.type", "vscode"),
            ("user.account_id", "user_01BWBeN28kQ7fYxR4tLmZ9pS"),
            (
                "user.id",
                "4b8f0a7c2e1d93f65a0c7e2b9d41f8a3c6e05b17d29f84a6c3e1b0d7f5a29c84",
            ),
            ("vcs.owner.name", "acme"),
            ("vcs.provider.name", "github"),
            ("vcs.repository.name", "storefront"),
        ]),
    }
}

fn claude_code_haiku_call() -> TelemetryMessage {
    TelemetryMessage {
        harness: TelemetryHarness::ClaudeCode,
        session_id: "6d1f7a2e-3b4c-4d5e-8f90-1a2b3c4d5e6f".to_owned(),
        message_id: "otel:9461e8185ede62e0ea197cfe723b1d02".to_owned(),
        model: "claude-haiku-4-5".to_owned(),
        occurred_at: datetime!(2026-10-05 09:14:05.020 UTC),
        tokens: TokenCounts {
            input: 312,
            output: 96,
            cache_write: 0,
            cache_read: 0,
        },
        reported_cost: Some(usd(792_000)),
        person_email: Some("ana.silva@acme.example".to_owned()),
        person_account_id: Some("a1b2c3d4-e5f6-4a7b-8c9d-0e1f2a3b4c5d".to_owned()),
        repository: None,
        attributes: tags(&[
            ("department", "engineering"),
            ("duration_ms", "1450"),
            ("host.arch", "arm64"),
            ("os.type", "darwin"),
            ("os.version", "25.0.0"),
            ("service.name", "claude-code"),
            ("service.version", "2.1.290"),
            ("team", "web"),
            (
                "user.id",
                "4b8f0a7c2e1d93f65a0c7e2b9d41f8a3c6e05b17d29f84a6c3e1b0d7f5a29c84",
            ),
        ]),
    }
}

#[test]
fn claude_code_api_requests_become_messages() {
    let request = decode_logs(CLAUDE_CODE_LOGS.as_bytes(), Encoding::Json).unwrap();
    let extracted = extract_from_logs(&request);
    assert_eq!(
        extracted.messages,
        [claude_code_sonnet_call(), claude_code_haiku_call()]
    );
    // A user prompt with its text and a tool result with its parameters.
    assert_eq!(extracted.dropped_with_prompt_text, 2);
    // A redacted user prompt, a tool decision and a redacted response.
    assert_eq!(extracted.ignored, 3);
    assert_eq!(
        extract_from_logs(&through_protobuf_logs(&request).unwrap()),
        extracted
    );
}

#[test]
fn claude_code_from_the_desktop_app_and_without_session_ids() {
    let resource = vec![text("service.name", "claude-code-desktop")];
    let call = |attributes: Vec<KeyValue>| {
        let mut all = vec![
            text("event.name", "api_request"),
            text("model", "claude-opus-5"),
            text("input_tokens", "10"),
            text("output_tokens", "20"),
            int("cost_usd_micros", 1_250),
        ];
        all.extend(attributes);
        log(1_791_191_643_512_000_000, "claude_code.api_request", all)
    };
    // OTEL_METRICS_INCLUDE_SESSION_ID=false leaves the prompt ID, and a
    // trace context leaves the trace ID.
    let mut traced = call(vec![]);
    traced.trace_id = vec![0xab; 16];
    let request = logs_request(
        resource,
        "com.anthropic.claude_code.events",
        vec![
            call(vec![text("prompt.id", "p-1"), text("request_id", "req_1")]),
            traced,
            call(vec![]),
        ],
    );
    let extracted = extract_from_logs(&through_protobuf_logs(&request).unwrap());
    let found: Vec<(&str, &str, Option<Money>)> = extracted
        .messages
        .iter()
        .map(|message| {
            (
                message.session_id.as_str(),
                message.message_id.as_str(),
                message.reported_cost,
            )
        })
        .collect();
    assert_eq!(found.len(), 2);
    assert_eq!(found[0], ("p-1", "req_1", Some(usd(1_250_000))));
    assert_eq!(found[1].0, "abababababababababababababababab");
    assert!(found[1].1.starts_with("otel:"));
    assert_eq!(
        extracted.messages[0].tokens,
        TokenCounts {
            input: 10,
            output: 20,
            cache_write: 0,
            cache_read: 0
        }
    );
    assert_eq!(
        extracted.messages[0].attributes,
        tags(&[("service.name", "claude-code-desktop")])
    );
    // No session, prompt or trace ID: nothing to join it to.
    assert_eq!(
        (extracted.dropped_with_prompt_text, extracted.ignored),
        (0, 1)
    );
}

// Codex CLI.

fn codex_cli_calls() -> [TelemetryMessage; 2] {
    let resource = [
        ("env", "prod"),
        ("host.name", "builder-07"),
        ("service.name", "codex_cli_rs"),
        ("service.version", "0.130.0"),
    ];
    let mut first = resource.to_vec();
    first.extend([
        ("app.version", "0.130.0"),
        ("auth_mode", "Chatgpt"),
        ("originator", "codex_cli_rs"),
        ("slug", "gpt-5-codex"),
        ("terminal.type", "iTerm.app/3.6.2"),
        ("ttft_ms", "740"),
        ("user.account_id", "8d2e4f6a-0b1c-4d3e-9f5a-7b9c1d3e5f7a"),
    ]);
    let message =
        |message_id: &str, occurred_at, tokens, attributes: &[(&str, &str)]| TelemetryMessage {
            harness: TelemetryHarness::CodexCli,
            session_id: "019a1b2c-3d4e-7f60-8a9b-0c1d2e3f4a5b".to_owned(),
            message_id: message_id.to_owned(),
            model: "gpt-5-codex".to_owned(),
            occurred_at,
            tokens,
            reported_cost: None,
            person_email: Some("dev.lee@acme.example".to_owned()),
            person_account_id: None,
            repository: Some("github.com/acme/payments-api".to_owned()),
            attributes: tags(attributes),
        };
    [
        message(
            "otel:14e75c2e78e64804ccbaa3cabd9643e4",
            datetime!(2026-10-05 10:02:11.348 UTC),
            TokenCounts {
                input: 3000,
                output: 950,
                cache_write: 0,
                cache_read: 15000,
            },
            &first,
        ),
        message(
            "otel:b495861eba362a45069666d4c9c7b607",
            datetime!(2026-10-05 10:02:19.901 UTC),
            TokenCounts {
                input: 1290,
                output: 312,
                cache_write: 0,
                cache_read: 17920,
            },
            &resource,
        ),
    ]
}

#[test]
fn codex_cli_completed_responses_become_messages() {
    let request = decode_logs(CODEX_CLI_LOGS.as_bytes(), Encoding::Json).unwrap();
    let extracted = extract_from_logs(&request);
    assert_eq!(extracted.messages, codex_cli_calls());
    // A tool result always carries its arguments and output.
    assert_eq!(extracted.dropped_with_prompt_text, 1);
    // Another stream event, a redacted prompt and a failed completion.
    assert_eq!(extracted.ignored, 3);
}

#[test]
fn codex_cli_input_counts_include_cache_reads_and_writes() {
    let request = logs_request(
        vec![text("service.name", "codex-tui")],
        "codex_otel",
        vec![LogRecord {
            attributes: vec![
                text("event.name", "codex.sse_event"),
                text("event.kind", "response.completed"),
                text("input_token_count", "5000"),
                text("output_token_count", "70"),
                int("cached_token_count", 3000),
                int("cache_write_token_count", 1000),
                text("event.timestamp", "2026-10-05T10:02:11.348Z"),
                text("conversation.id", "c-1"),
                text("model", "gpt-5"),
            ],
            observed_time_unix_nano: 1_791_194_531_349_000_000,
            ..LogRecord::default()
        }],
    );
    let extracted = extract_from_logs(&through_protobuf_logs(&request).unwrap());
    let message = &extracted.messages[0];
    assert_eq!(
        message.tokens,
        TokenCounts {
            input: 1000,
            output: 70,
            cache_write: 1000,
            cache_read: 3000
        }
    );
    assert_eq!(message.occurred_at, datetime!(2026-10-05 10:02:11.348 UTC));
    assert_eq!(message.attributes, tags(&[("service.name", "codex-tui")]));
}

// Gemini CLI.

fn gemini_cli_request() -> ExportLogsServiceRequest {
    let common = || {
        vec![
            text("session.id", "c0ffee00-1111-4222-8333-444455556666"),
            text("installation.id", "1d2c3b4a-5e6f-4a7b-8c9d-0e1f2a3b4c5d"),
            attribute("interactive", Value::BoolValue(true)),
            text("user.email", "Kim.Park@Acme.example"),
            text("auth_type", "oauth-personal"),
        ]
    };
    let mut response = common();
    response.extend([
        text("event.name", "gemini_cli.api_response"),
        text("event.timestamp", "2026-10-05T11:30:00.250Z"),
        text("model", "gemini-2.5-pro"),
        int("duration_ms", 2310),
        int("input_token_count", 9000),
        int("output_token_count", 420),
        int("cached_content_token_count", 6000),
        int("thoughts_token_count", 380),
        int("tool_token_count", 0),
        int("total_token_count", 9800),
        text("prompt_id", "c0ffee00-1111-4222-8333-444455556666########3"),
        int("status_code", 200),
        attribute(
            "finish_reasons",
            Value::ArrayValue(ArrayValue {
                values: vec![AnyValue {
                    value: Some(Value::StringValue("STOP".to_owned())),
                }],
            }),
        ),
        int("http.status_code", 200),
    ]);
    let mut details = common();
    details.extend([
        text("event.name", "gen_ai.client.inference.operation.details"),
        text("gen_ai.response.id", "resp-77"),
        text("gen_ai.request.model", "gemini-2.5-pro"),
        int("gen_ai.usage.input_tokens", 9000),
        int("gen_ai.usage.output_tokens", 420),
    ]);
    let mut tool_call = common();
    tool_call.extend([
        text("event.name", "gemini_cli.tool_call"),
        text("function_name", "read_file"),
        text("function_args", "{\"absolute_path\":\"/src/app.ts\"}"),
    ]);
    let mut prompt = common();
    prompt.extend([
        text("event.name", "gemini_cli.user_prompt"),
        int("prompt_length", 25),
    ]);
    let mut api_request = common();
    api_request.extend([
        text("event.name", "gemini_cli.api_request"),
        text("model", "gemini-2.5-pro"),
        text(
            "request_text",
            "[{\"role\":\"user\",\"parts\":[{\"text\":\"Fix the build\"}]}]",
        ),
    ]);
    let time = 1_791_199_800_250_000_000;
    logs_request(
        vec![
            text("service.name", "gemini-cli"),
            text("service.version", "0.34.0"),
            text("team", "data"),
        ],
        "gemini-cli",
        vec![
            log(
                time,
                "API response from gemini-2.5-pro. Status: 200. Duration: 2310ms.",
                response,
            ),
            log(
                time,
                "GenAI operation details from gemini-2.5-pro. Status: 200. Duration: 2310ms.",
                details,
            ),
            log(
                time,
                "Tool call: read_file. Decision: auto_accept. Success: true. Duration: 3ms.",
                tool_call,
            ),
            log(time, "User prompt. Length: 25.", prompt),
            log(time, "API request to gemini-2.5-pro.", api_request),
        ],
    )
}

#[test]
fn gemini_cli_api_responses_become_messages() {
    let extracted = extract_from_logs(&through_protobuf_logs(&gemini_cli_request()).unwrap());
    let expected = TelemetryMessage {
        harness: TelemetryHarness::GeminiCli,
        session_id: "c0ffee00-1111-4222-8333-444455556666".to_owned(),
        message_id: "otel:a7887ce1eb84fb99468e9c5dc6b2304c".to_owned(),
        model: "gemini-2.5-pro".to_owned(),
        occurred_at: datetime!(2026-10-05 11:30:00.250 UTC),
        // The prompt includes the cached content; thoughts are output.
        tokens: TokenCounts {
            input: 3000,
            output: 800,
            cache_write: 0,
            cache_read: 6000,
        },
        reported_cost: None,
        person_email: Some("kim.park@acme.example".to_owned()),
        person_account_id: None,
        repository: None,
        attributes: tags(&[
            ("auth_type", "oauth-personal"),
            ("duration_ms", "2310"),
            ("http.status_code", "200"),
            ("installation.id", "1d2c3b4a-5e6f-4a7b-8c9d-0e1f2a3b4c5d"),
            ("interactive", "true"),
            ("prompt_id", "c0ffee00-1111-4222-8333-444455556666########3"),
            ("service.name", "gemini-cli"),
            ("service.version", "0.34.0"),
            ("status_code", "200"),
            ("team", "data"),
        ]),
    };
    assert_eq!(extracted.messages, [expected]);
    // The tool call's arguments and the request text.
    assert_eq!(extracted.dropped_with_prompt_text, 2);
    // The repeated GenAI details of the same call and the prompt length.
    assert_eq!(extracted.ignored, 2);
}

#[test]
fn gemini_cli_with_prompt_logging_on_keeps_its_counts() {
    // Gemini CLI logs prompts unless `logPrompts` is false: the response
    // event then carries the response text.
    let mut request = gemini_cli_request();
    let records = &mut request.resource_logs[0].scope_logs[0].log_records;
    records[0]
        .attributes
        .push(text("response_text", "Fixed the build."));
    records[1]
        .attributes
        .push(text("gen_ai.output.messages", "[{\"role\":\"model\"}]"));
    let extracted = extract_from_logs(&through_protobuf_logs(&request).unwrap());
    let plain = extract_from_logs(&through_protobuf_logs(&gemini_cli_request()).unwrap());
    // The same message as without the text, and no text in its attributes.
    assert_eq!(extracted.messages, plain.messages);
    assert_eq!(extracted.stripped_prompt_text, 1);
    // The tool call, the request text and the GenAI details.
    assert_eq!(extracted.dropped_with_prompt_text, 3);
    assert_eq!(extracted.ignored, 1);
}

// Copilot CLI.

fn copilot_cli_calls() -> [TelemetryMessage; 2] {
    let message = |message_id: &str, model: &str, occurred_at, tokens, extra: &[(&str, &str)]| {
        let mut attributes = vec![
            ("service.name", "github-copilot"),
            ("service.version", "1.0.34"),
            ("team", "platform"),
        ];
        attributes.extend_from_slice(extra);
        TelemetryMessage {
            harness: TelemetryHarness::CopilotCli,
            session_id: "5f0c2a8e-91d4-4b6a-a3e7-2c8d0f4b6a19".to_owned(),
            message_id: message_id.to_owned(),
            model: model.to_owned(),
            occurred_at,
            tokens,
            reported_cost: None,
            person_email: Some("sam.ortiz@acme.example".to_owned()),
            person_account_id: None,
            repository: None,
            attributes: tags(&attributes),
        }
    };
    [
        message(
            "msg_01Hc7TqgKz3nWb5YxPr2VdLm",
            "claude-sonnet-5-20260901",
            datetime!(2026-10-05 13:05:07.777 UTC),
            TokenCounts {
                input: 410,
                output: 1408,
                cache_write: 3900,
                cache_read: 48000,
            },
            &[
                ("gen_ai.provider.name", "github"),
                ("gen_ai.request.stream", "true"),
                ("gen_ai.response.time_to_first_chunk", "1.84"),
                ("github.copilot.cost", "1"),
                ("github.copilot.initiator", "agent"),
                ("github.copilot.nano_aiu", "2710000000"),
                ("github.copilot.turn_id", "turn-3"),
                ("server.address", "api.githubcopilot.com"),
                ("server.port", "443"),
            ],
        ),
        message(
            "otel:dc279ba35342a57cf4b9b94937d64c88",
            "claude-sonnet-5",
            datetime!(2026-10-05 13:05:11.250 UTC),
            TokenCounts {
                input: 1200,
                output: 340,
                cache_write: 0,
                cache_read: 0,
            },
            &[],
        ),
    ]
}

#[test]
fn copilot_cli_chat_spans_become_messages() {
    let request = decode_traces(COPILOT_CLI_TRACES.as_bytes(), Encoding::Json).unwrap();
    let extracted = extract_from_traces(&request);
    let [first, second] = copilot_cli_calls();
    // The chat span with its input messages counts without them.
    let summarize = TelemetryMessage {
        message_id: "otel:c48d506110bf2edffa3a4aca0456f1af".to_owned(),
        model: "gpt-5-mini".to_owned(),
        occurred_at: datetime!(2026-10-05 13:05:13 UTC),
        tokens: TokenCounts {
            input: 900,
            output: 40,
            cache_write: 0,
            cache_read: 0,
        },
        ..second.clone()
    };
    assert_eq!(extracted.messages, [first, second, summarize]);
    assert_eq!(extracted.stripped_prompt_text, 1);
    // The agent span, whose compaction event carries a summary.
    assert_eq!(extracted.dropped_with_prompt_text, 1);
    // The tool span.
    assert_eq!(extracted.ignored, 1);
    assert_eq!(
        extract_from_traces(&through_protobuf_traces(&request).unwrap()),
        extracted
    );
}

#[test]
fn copilot_cli_cache_counts_follow_the_genai_conventions() {
    let chat = |attributes: Vec<KeyValue>| {
        let mut all = vec![
            text("gen_ai.operation.name", "chat"),
            text("gen_ai.request.model", "gpt-5"),
            text("gen_ai.conversation.id", "s-1"),
        ];
        all.extend(attributes);
        span("chat gpt-5", 1_791_205_507_777_000_000, all)
    };
    let request = traces_request(
        vec![text("service.name", "my-renamed-copilot")],
        "github.copilot",
        vec![
            chat(vec![
                int("gen_ai.usage.input_tokens", 1000),
                int("gen_ai.usage.output_tokens", 5),
                int("gen_ai.usage.cache_read.input_tokens", 600),
                int("gen_ai.usage.cache_write.input_tokens", 300),
            ]),
            // Cache counts above the input count were counted apart.
            chat(vec![
                int("gen_ai.usage.input_tokens", 12),
                int("gen_ai.usage.output_tokens", 5),
                int("gen_ai.usage.cache_read.input_tokens", 40_000),
            ]),
            span(
                "invoke_agent",
                1_791_205_507_777_000_000,
                vec![text("gen_ai.operation.name", "invoke_agent")],
            ),
        ],
    );
    let extracted = extract_from_traces(&through_protobuf_traces(&request).unwrap());
    let tokens: Vec<TokenCounts> = extracted
        .messages
        .iter()
        .map(|message| message.tokens)
        .collect();
    assert_eq!(
        tokens,
        [
            TokenCounts {
                input: 100,
                output: 5,
                cache_write: 300,
                cache_read: 600
            },
            TokenCounts {
                input: 12,
                output: 5,
                cache_write: 0,
                cache_read: 40_000
            },
        ]
    );
    assert_eq!(extracted.ignored, 1);
}

// Cursor.

const CURSOR_TIME: u64 = 1_791_204_330_125_000_000;

fn cursor_api_request(
    event_id: &str,
    conversation: Option<&str>,
    model: Option<&str>,
    [input, output, cache_read, cache_creation]: [i64; 4],
) -> LogRecord {
    let mut attributes = vec![
        text("cursor.event.id", event_id),
        text("cursor.source_event.id", "src_91ab"),
        int("cursor.api.request.input_tokens", input),
        int("cursor.api.request.output_tokens", output),
        int("cursor.api.request.cache_read_tokens", cache_read),
        int("cursor.api.request.cache_creation_tokens", cache_creation),
    ];
    attributes.extend(conversation.map(|id| text("cursor.conversation.id", id)));
    attributes.extend(model.map(|name| text("cursor.model.name", name)));
    cursor_event("cursor.api.request", "api_request", attributes)
}

fn cursor_event(event_name: &str, body: &str, attributes: Vec<KeyValue>) -> LogRecord {
    LogRecord {
        event_name: event_name.to_owned(),
        ..log(CURSOR_TIME, body, attributes)
    }
}

fn cursor_request() -> ExportLogsServiceRequest {
    let desktop = vec![
        text("service.name", "cursor"),
        text("service.version", "1.9.2"),
        int("cursor.team.id", 48_213),
        text("cursor.surface", "desktop"),
        text("cursor.entrypoint", "desktop"),
        int("cursor.user.id", 77_120),
        text("cursor.user.account_id", "user_2Wm8Qd7Lk3"),
        text("cursor.user.email", "Lena.Fischer@Acme.example"),
    ];
    let mut first = cursor_api_request(
        "customer-telemetry:v1:7f3a9c21",
        Some("b2f6c1de-7a90-4e1f-8c3b-5d2e9a0f7c41"),
        Some("claude-sonnet-5"),
        [2400, 610, 30_500, 1800],
    );
    first.attributes.extend([
        text("cursor.usage_event.id", "ue_01J9ZK"),
        attribute("cursor.api.billable", Value::BoolValue(true)),
    ]);
    let mut logs = logs_request(
        desktop,
        "cursor.telemetry",
        vec![
            first,
            cursor_event(
                "cursor.api.correction",
                "api_correction_not_billed_errored",
                vec![
                    text("cursor.event.id", "e-2"),
                    text("cursor.usage_event.id", "ue_01J9ZK"),
                    text("cursor.api.correction.kind", "not_billed_errored"),
                ],
            ),
            cursor_event(
                "cursor.conversation.user_message",
                "Why does the checkout test fail?",
                vec![
                    text("cursor.event.id", "e-3"),
                    text(
                        "cursor.conversation.id",
                        "b2f6c1de-7a90-4e1f-8c3b-5d2e9a0f7c41",
                    ),
                ],
            ),
            cursor_event(
                "cursor.skill.activated",
                "skill_activated",
                vec![
                    text("cursor.event.id", "e-4"),
                    text("cursor.skill.name", "pdf"),
                ],
            ),
            // No conversation: nothing to join it to.
            cursor_api_request(
                "customer-telemetry:v1:0000",
                None,
                Some("gpt-5"),
                [1, 1, 0, 0],
            ),
        ],
    );
    let bugbot = logs_request(
        vec![
            text("service.name", "cursor"),
            int("cursor.team.id", 48_213),
            text("cursor.surface", "bugbot"),
        ],
        "cursor.telemetry",
        vec![cursor_api_request(
            "customer-telemetry:v1:5e1f",
            Some("bc-5e1f0a2b"),
            None,
            [5000, 200, 0, 0],
        )],
    );
    logs.resource_logs.extend(bugbot.resource_logs);
    logs
}

#[test]
fn cursor_api_requests_become_messages() {
    let extracted = extract_from_logs(&through_protobuf_logs(&cursor_request()).unwrap());
    let desktop = TelemetryMessage {
        harness: TelemetryHarness::Cursor,
        session_id: "b2f6c1de-7a90-4e1f-8c3b-5d2e9a0f7c41".to_owned(),
        message_id: "customer-telemetry:v1:7f3a9c21".to_owned(),
        model: "claude-sonnet-5".to_owned(),
        occurred_at: datetime!(2026-10-05 12:45:30.125 UTC),
        tokens: TokenCounts {
            input: 2400,
            output: 610,
            cache_write: 1800,
            cache_read: 30_500,
        },
        reported_cost: None,
        person_email: Some("lena.fischer@acme.example".to_owned()),
        person_account_id: Some("user_2Wm8Qd7Lk3".to_owned()),
        repository: None,
        attributes: tags(&[
            ("cursor.api.billable", "true"),
            ("cursor.entrypoint", "desktop"),
            ("cursor.source_event.id", "src_91ab"),
            ("cursor.surface", "desktop"),
            ("cursor.team.id", "48213"),
            ("cursor.usage_event.id", "ue_01J9ZK"),
            ("cursor.user.id", "77120"),
            ("service.name", "cursor"),
            ("service.version", "1.9.2"),
        ]),
    };
    let bugbot = TelemetryMessage {
        harness: TelemetryHarness::Cursor,
        session_id: "bc-5e1f0a2b".to_owned(),
        message_id: "customer-telemetry:v1:5e1f".to_owned(),
        model: "unknown".to_owned(),
        occurred_at: datetime!(2026-10-05 12:45:30.125 UTC),
        tokens: TokenCounts {
            input: 5000,
            output: 200,
            cache_write: 0,
            cache_read: 0,
        },
        reported_cost: None,
        person_email: None,
        person_account_id: None,
        repository: None,
        attributes: tags(&[
            ("cursor.source_event.id", "src_91ab"),
            ("cursor.surface", "bugbot"),
            ("cursor.team.id", "48213"),
            ("service.name", "cursor"),
        ]),
    };
    assert_eq!(extracted.messages, [desktop, bugbot]);
    // The user message, whose body is the prompt.
    assert_eq!(extracted.dropped_with_prompt_text, 1);
    // The billing correction, the skill and the call without a conversation.
    assert_eq!(extracted.ignored, 3);
}

// Every harness at once.

#[test]
fn a_retried_export_gives_the_same_message_ids() {
    let first = extract_from_logs(&decode_logs(CODEX_CLI_LOGS.as_bytes(), Encoding::Json).unwrap());
    let compressed = gzip(CODEX_CLI_LOGS.as_bytes()).unwrap();
    let body = decompress(Some("gzip"), &compressed).unwrap();
    let retried = extract_from_logs(&decode_logs(&body, Encoding::Json).unwrap());
    assert_eq!(retried, first);
    let ids: Vec<&str> = first
        .messages
        .iter()
        .map(|message| message.message_id.as_str())
        .collect();
    assert_eq!(
        ids,
        [
            "otel:14e75c2e78e64804ccbaa3cabd9643e4",
            "otel:b495861eba362a45069666d4c9c7b607"
        ]
    );

    // Any change to the call is another call.
    let mut changed = decode_logs(CODEX_CLI_LOGS.as_bytes(), Encoding::Json).unwrap();
    changed.resource_logs[0].scope_logs[0].log_records[0].attributes[3] =
        text("output_token_count", "951");
    let changed = extract_from_logs(&changed);
    assert_ne!(changed.messages[0].message_id, first.messages[0].message_id);
    assert_eq!(changed.messages[1].message_id, first.messages[1].message_id);
}

#[test]
fn a_mixed_batch_keeps_only_model_calls_without_prompt_text() {
    let mut batch = decode_logs(CLAUDE_CODE_LOGS.as_bytes(), Encoding::Json).unwrap();
    batch.resource_logs.extend(
        decode_logs(CODEX_CLI_LOGS.as_bytes(), Encoding::Json)
            .unwrap()
            .resource_logs,
    );
    batch
        .resource_logs
        .extend(gemini_cli_request().resource_logs);
    batch.resource_logs.extend(cursor_request().resource_logs);
    // Telemetry from a service that is no agent harness.
    batch.resource_logs.extend(
        logs_request(
            vec![text("service.name", "checkout-api")],
            "io.opentelemetry",
            vec![log(1, "GET /cart", vec![int("http.status_code", 200)])],
        )
        .resource_logs,
    );
    // GenAI events with the message in the body, and a map body.
    let mut genai = log(
        1,
        "Fix the flaky test",
        vec![text("event.name", "gen_ai.user.message")],
    );
    genai.event_name = "gen_ai.user.message".to_owned();
    let mut choice = log(1, "", vec![]);
    choice.event_name = "gen_ai.choice".to_owned();
    choice.body = Some(any(Value::KvlistValue(KeyValueList {
        values: vec![int("index", 0), text("message", "Done.")],
    })));
    batch.resource_logs.extend(
        logs_request(
            vec![text("service.name", "claude-code")],
            "x",
            vec![genai, choice],
        )
        .resource_logs,
    );

    let body = gzip(&batch.encode_to_vec()).unwrap();
    let encoding = Encoding::from_content_type(Some("application/x-protobuf")).unwrap();
    let extracted = extract_from_logs(
        &decode_logs(&decompress(Some("gzip"), &body).unwrap(), encoding).unwrap(),
    );

    let harnesses: Vec<TelemetryHarness> = extracted
        .messages
        .iter()
        .map(|message| message.harness)
        .collect();
    assert_eq!(
        harnesses,
        [
            TelemetryHarness::ClaudeCode,
            TelemetryHarness::ClaudeCode,
            TelemetryHarness::CodexCli,
            TelemetryHarness::CodexCli,
            TelemetryHarness::GeminiCli,
            TelemetryHarness::Cursor,
            TelemetryHarness::Cursor,
        ]
    );
    // Two each from `Claude Code`, Gemini CLI and the GenAI events, one each
    // from Codex CLI and `Cursor`.
    assert_eq!(extracted.dropped_with_prompt_text, 8);
    // Three each from `Claude Code`, Codex CLI and `Cursor`, two from Gemini
    // CLI and the request of the other service.
    assert_eq!(extracted.ignored, 12);
}

#[test]
fn each_harness_has_one_door() {
    // `Claude Code` spans repeat its api_request events.
    let claude_span = traces_request(
        vec![text("service.name", "claude-code")],
        "com.anthropic.claude_code.tracing",
        vec![span(
            "claude_code.llm_request",
            2,
            vec![
                text("session.id", "s"),
                text("model", "m"),
                int("input_tokens", 1),
                int("output_tokens", 1),
            ],
        )],
    );
    assert_eq!(extract_from_traces(&claude_span).ignored, 1);
    // Copilot CLI sends no log events; a log shaped like a chat span is not one.
    let copilot_log = logs_request(
        vec![text("service.name", "github-copilot")],
        "github.copilot",
        vec![log(
            2,
            "",
            vec![
                text("gen_ai.operation.name", "chat"),
                text("gen_ai.conversation.id", "s"),
                int("gen_ai.usage.input_tokens", 1),
                int("gen_ai.usage.output_tokens", 1),
            ],
        )],
    );
    assert_eq!(extract_from_logs(&copilot_log).ignored, 1);
    // A span of an unknown service.
    let other = traces_request(
        vec![text("service.name", "storefront")],
        "x",
        vec![span("chat", 2, vec![text("gen_ai.operation.name", "chat")])],
    );
    assert_eq!(extract_from_traces(&other).ignored, 1);
    assert_eq!(
        extract_from_traces(&ExportTraceServiceRequest::default()),
        Extracted::default()
    );
}

#[test]
fn unreadable_token_counts_are_ignored() {
    let call = |input: Value| {
        log(
            1,
            "claude_code.api_request",
            vec![
                text("event.name", "api_request"),
                text("session.id", "s"),
                text("model", "m"),
                attribute("input_tokens", input),
                text("output_tokens", "1"),
            ],
        )
    };
    let request = logs_request(
        vec![],
        "x",
        vec![
            call(Value::StringValue("12.5".to_owned())),
            call(Value::StringValue("lots".to_owned())),
            call(Value::IntValue(-1)),
            call(Value::DoubleValue(7.5)),
            call(Value::BoolValue(true)),
            call(Value::DoubleValue(7.0)),
            call(Value::StringValue(" 8 ".to_owned())),
        ],
    );
    let extracted = extract_from_logs(&request);
    let inputs: Vec<u64> = extracted
        .messages
        .iter()
        .map(|message| message.tokens.input)
        .collect();
    assert_eq!(inputs, [7, 8]);
    assert_eq!(extracted.ignored, 5);
}

fn list(values: Vec<Value>) -> Value {
    Value::ArrayValue(ArrayValue {
        values: values.into_iter().map(any).collect(),
    })
}

fn secret_map() -> Value {
    Value::KvlistValue(KeyValueList {
        values: vec![text("prompt", "secret")],
    })
}

#[test]
fn prompt_text_in_any_attribute_drops_the_event() {
    let dropped = |attributes: Vec<KeyValue>| {
        let record = log(1, "claude_code.api_request", attributes);
        extract_from_logs(&logs_request(vec![], "x", vec![record])).dropped_with_prompt_text
    };
    for (attributes, expected) in [
        (vec![text("gen_ai.prompt.0.content", "hi")], 1),
        (vec![text("gen_ai.completion.0.content", "hi")], 1),
        (vec![text("gen_ai.prompt.variable.city", "Graz")], 1),
        (vec![attribute("metadata", secret_map())], 1),
        (vec![attribute("items", list(vec![secret_map()]))], 1),
        (
            vec![attribute(
                "gen_ai.input.messages",
                Value::BytesValue(b"[]".to_vec()),
            )],
            1,
        ),
        (
            vec![attribute(
                "tool_input",
                list(vec![Value::StringValue("ls".to_owned())]),
            )],
            1,
        ),
        // A template's name and version are no text.
        (
            vec![
                text("gen_ai.prompt.name", "analyze-code"),
                text("gen_ai.prompt.version", "2"),
            ],
            0,
        ),
        // Redacted, blank or not text at all.
        (
            vec![
                text("prompt", "<REDACTED>"),
                text("response", "[REDACTED]"),
                text("output", "  "),
            ],
            0,
        ),
        (
            vec![
                int("prompt", 3),
                attribute("content", Value::BoolValue(true)),
            ],
            0,
        ),
        (
            vec![attribute(
                "prompt",
                list(vec![Value::StringValue(String::new())]),
            )],
            0,
        ),
    ] {
        assert_eq!(dropped(attributes.clone()), expected, "{attributes:?}");
    }
    assert!(PROMPT_TEXT_ATTRIBUTES.contains(&"gen_ai.input.messages"));
}

#[test]
fn a_model_call_loses_only_what_carries_text() {
    let mut record = log(
        1,
        "claude_code.api_request",
        vec![
            text("event.name", "api_request"),
            text("session.id", "s"),
            text("model", "m"),
            int("input_tokens", 10),
            int("output_tokens", 20),
            text("prompt", "Fix the build"),
            attribute("metadata", secret_map()),
            text("team", "web"),
        ],
    );
    record.body = Some(any(list(vec![Value::StringValue("Hi".to_owned())])));
    let claude_code = || vec![text("service.name", "claude-code")];
    let extracted = extract_from_logs(&logs_request(claude_code(), "x", vec![record]));
    assert_eq!(
        (
            extracted.stripped_prompt_text,
            extracted.dropped_with_prompt_text
        ),
        (1, 0)
    );
    let message = &extracted.messages[0];
    assert_eq!((message.tokens.input, message.tokens.output), (10, 20));
    assert_eq!(
        message.attributes,
        tags(&[("service.name", "claude-code"), ("team", "web")])
    );

    // A body that is the message text names nothing, so no call is read
    // from it.
    let mut named_by_text = log(
        1,
        "claude_code.api_request",
        vec![
            text("session.id", "s"),
            text("model", "m"),
            int("input_tokens", 10),
            int("output_tokens", 20),
        ],
    );
    named_by_text.event_name = "gen_ai.user.message".to_owned();
    let extracted = extract_from_logs(&logs_request(claude_code(), "x", vec![named_by_text]));
    assert_eq!(
        (
            extracted.stripped_prompt_text,
            extracted.dropped_with_prompt_text
        ),
        (0, 1)
    );
}

#[test]
fn a_body_with_text_drops_the_event() {
    let dropped = |body: Value, event_name: &str| {
        let mut record = log(1, "", vec![]);
        record.body = Some(any(body));
        record.event_name = event_name.to_owned();
        extract_from_logs(&logs_request(vec![], "x", vec![record])).dropped_with_prompt_text
    };
    let words = |text: &str| Value::StringValue(text.to_owned());
    assert_eq!(
        dropped(words("Hello"), "cursor.conversation.assistant_message"),
        1
    );
    assert_eq!(dropped(words(""), "gen_ai.user.message"), 0);
    // Every agent harness labels its other events with a string body.
    assert_eq!(
        dropped(
            words("API response from gemini-2.5-pro."),
            "gemini_cli.api_response"
        ),
        0
    );
    assert_eq!(dropped(words("claude_code.user_prompt"), ""), 0);
    // A map, list or bytes body is a payload.
    assert_eq!(dropped(Value::BytesValue(vec![1]), "x"), 1);
    assert_eq!(dropped(secret_map(), "x"), 1);
    assert_eq!(dropped(list(vec![words("Hi")]), "x"), 1);
    let no_text = Value::KvlistValue(KeyValueList {
        values: vec![int("index", 0)],
    });
    assert_eq!(dropped(no_text, "x"), 0);
    assert_eq!(dropped(Value::IntValue(4), "x"), 0);
    assert!(PROMPT_TEXT_EVENTS.contains(&"cursor.conversation.user_message"));
}

/// The bodies real agent CLIs sent to the agent harness
/// (`scripts/agent-harness/run.py --record`), scrubbed of names and hosts:
/// each `*-logs-*.json` fixture holds at least one model call of the agent
/// harness its file name starts with, and each metrics fixture decodes.
#[test]
fn recorded_agent_bodies_read_as_their_agent_harness() -> Result<(), Box<dyn Error>> {
    let folder =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/otlp/recorded");
    let mut agents_seen = std::collections::BTreeSet::new();
    for entry in std::fs::read_dir(&folder)? {
        let path = entry?.path();
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or("fixture without a name")?
            .to_owned();
        let body = std::fs::read(&path)?;
        // `<agent>-<version>-<signal>-<index>`, split from the right because
        // agent names carry dashes.
        let mut parts = name.rsplitn(4, '-');
        let _index = parts.next();
        let signal = parts.next().ok_or("fixture name without a signal")?;
        let _version = parts.next();
        let agent = parts.next().ok_or("fixture name without an agent")?;
        match signal {
            "metrics" => {
                decode_metrics(&body, Encoding::Json)?;
            }
            "logs" => {
                let extracted = extract_from_logs(&decode_logs(&body, Encoding::Json)?);
                assert!(!extracted.messages.is_empty(), "{name} holds no model call");
                for message in &extracted.messages {
                    assert_eq!(message.harness.name(), agent, "{name}");
                    assert!(!message.model.is_empty(), "{name}: a call without a model");
                    assert!(
                        !message.session_id.is_empty(),
                        "{name}: a call without a session"
                    );
                    assert!(
                        message.tokens.output > 0,
                        "{name}: a call without output tokens"
                    );
                }
                agents_seen.insert(agent.to_owned());
            }
            "traces" => {
                let extracted = extract_from_traces(&decode_traces(&body, Encoding::Json)?);
                assert!(!extracted.messages.is_empty(), "{name} holds no model call");
                agents_seen.insert(agent.to_owned());
            }
            other => return Err(format!("{name}: unknown signal {other}").into()),
        }
    }
    assert!(
        agents_seen.contains("claude-code"),
        "no recorded Claude Code body"
    );
    assert!(
        agents_seen.contains("codex-cli"),
        "no recorded Codex CLI body"
    );
    Ok(())
}
