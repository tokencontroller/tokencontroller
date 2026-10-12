//! OTLP over HTTP on the wire: content type and encoding, decompression,
//! decoding of the three export requests and encoding of their responses.
//! The JSON form follows the OTLP rules (camelCase keys, hex trace and span
//! IDs, 64-bit integers as strings or numbers, enums as integers), which the
//! serde support of `opentelemetry-proto` implements.

use std::io::Read as _;

use flate2::read::MultiGzDecoder;
use opentelemetry_proto::tonic::collector::logs::v1::{
    ExportLogsPartialSuccess, ExportLogsServiceRequest, ExportLogsServiceResponse,
};
use opentelemetry_proto::tonic::collector::metrics::v1::{
    ExportMetricsPartialSuccess, ExportMetricsServiceRequest, ExportMetricsServiceResponse,
};
use opentelemetry_proto::tonic::collector::trace::v1::{
    ExportTracePartialSuccess, ExportTraceServiceRequest, ExportTraceServiceResponse,
};
use prost::Message as _;

/// The largest body the receiver reads, after decompression: 32 MiB.
pub const MAX_BODY_BYTES: usize = 33_554_432;

/// Why an export request cannot be read.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("content type {0:?} is not OTLP: send application/x-protobuf or application/json")]
    UnsupportedContentType(String),
    #[error("content encoding {0:?} is not supported: send gzip or no encoding")]
    UnsupportedContentEncoding(String),
    #[error("the body is larger than 32 MiB")]
    TooLarge,
    #[error("the body is not valid gzip: {0}")]
    Gzip(#[source] std::io::Error),
    #[error("the body is not a valid OTLP protobuf message: {0}")]
    Protobuf(#[from] prost::DecodeError),
    #[error("the body is not valid OTLP JSON: {0}")]
    Json(#[from] serde_json::Error),
}

/// How an export request and its response are encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Protobuf,
    Json,
}

impl Encoding {
    /// Reads a `Content-Type` header, ignoring parameters such as `charset`.
    /// A request without one is protobuf, the OTLP default.
    ///
    /// # Errors
    ///
    /// [`DecodeError::UnsupportedContentType`] for any other media type.
    pub fn from_content_type(content_type: Option<&str>) -> Result<Self, DecodeError> {
        let Some(header) = content_type else {
            return Ok(Self::Protobuf);
        };
        let media_type = header.split(';').next().unwrap_or_default().trim();
        if media_type.is_empty() || media_type.eq_ignore_ascii_case("application/x-protobuf") {
            Ok(Self::Protobuf)
        } else if media_type.eq_ignore_ascii_case("application/json") {
            Ok(Self::Json)
        } else {
            Err(DecodeError::UnsupportedContentType(header.to_owned()))
        }
    }

    /// The `Content-Type` of a body in this encoding.
    #[must_use]
    pub const fn content_type(self) -> &'static str {
        match self {
            Self::Protobuf => "application/x-protobuf",
            Self::Json => "application/json",
        }
    }
}

/// The three OTLP signals, each with its own export path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Logs,
    Traces,
    Metrics,
}

/// Undoes the `Content-Encoding` of a body: `gzip`, or none (`identity`).
///
/// # Errors
///
/// [`DecodeError::UnsupportedContentEncoding`] for any other encoding,
/// [`DecodeError::Gzip`] for a broken gzip body and [`DecodeError::TooLarge`]
/// when the body is over [`MAX_BODY_BYTES`] once decompressed.
pub fn decompress(content_encoding: Option<&str>, body: &[u8]) -> Result<Vec<u8>, DecodeError> {
    decompress_up_to(content_encoding, body, MAX_BODY_BYTES)
}

/// [`decompress`] with the size limit as a parameter, so tests stay small.
pub(super) fn decompress_up_to(
    content_encoding: Option<&str>,
    body: &[u8],
    limit: usize,
) -> Result<Vec<u8>, DecodeError> {
    let encoding = content_encoding.map(str::trim).unwrap_or_default();
    if encoding.is_empty() || encoding.eq_ignore_ascii_case("identity") {
        if body.len() > limit {
            return Err(DecodeError::TooLarge);
        }
        return Ok(body.to_vec());
    }
    if !encoding.eq_ignore_ascii_case("gzip") && !encoding.eq_ignore_ascii_case("x-gzip") {
        return Err(DecodeError::UnsupportedContentEncoding(encoding.to_owned()));
    }
    let limit = u64::try_from(limit).map_err(|_| DecodeError::TooLarge)?;
    let mut limited = MultiGzDecoder::new(body).take(limit);
    let mut decompressed = Vec::new();
    limited
        .read_to_end(&mut decompressed)
        .map_err(DecodeError::Gzip)?;
    let mut rest = limited.into_inner();
    let mut one_more = [0_u8; 1];
    if rest.read(&mut one_more).map_err(DecodeError::Gzip)? != 0 {
        return Err(DecodeError::TooLarge);
    }
    Ok(decompressed)
}

/// Reads a logs export request.
///
/// # Errors
///
/// [`DecodeError::Protobuf`] or [`DecodeError::Json`] if the body is not one.
pub fn decode_logs(
    body: &[u8],
    encoding: Encoding,
) -> Result<ExportLogsServiceRequest, DecodeError> {
    decode(body, encoding)
}

/// Reads a traces export request.
///
/// # Errors
///
/// [`DecodeError::Protobuf`] or [`DecodeError::Json`] if the body is not one.
pub fn decode_traces(
    body: &[u8],
    encoding: Encoding,
) -> Result<ExportTraceServiceRequest, DecodeError> {
    decode(body, encoding)
}

/// Reads a metrics export request. Metrics are acknowledged but never become
/// messages: the per-call events already count every model call.
///
/// # Errors
///
/// [`DecodeError::Protobuf`] or [`DecodeError::Json`] if the body is not one.
pub fn decode_metrics(
    body: &[u8],
    encoding: Encoding,
) -> Result<ExportMetricsServiceRequest, DecodeError> {
    decode(body, encoding)
}

fn decode<T>(body: &[u8], encoding: Encoding) -> Result<T, DecodeError>
where
    T: prost::Message + Default + serde::de::DeserializeOwned,
{
    match encoding {
        Encoding::Protobuf => Ok(T::decode(body)?),
        Encoding::Json => serde_json::from_slice(body).or_else(|error| {
            // The slow path, only for bodies the fast one refuses.
            let mut value: serde_json::Value = serde_json::from_slice(body)?;
            if !repair_json(&mut value) {
                return Err(DecodeError::Json(error));
            }
            serde_json::from_value(value).map_err(|_| DecodeError::Json(error))
        }),
    }
}

/// Fields that hold a double, which OTLP JSON may also write as a string.
const DOUBLE_FIELDS: &[&str] = &["doubleValue", "asDouble", "sum", "min", "max"];

/// Rewrites the valid OTLP JSON that the serde support of
/// `opentelemetry-proto` refuses: an empty `arrayValue` or `kvlistValue`
/// written as `{}`, as the OpenTelemetry Collector does, and doubles written
/// as strings. `"NaN"` and `"Infinity"` cannot be held and are left out.
/// Returns whether anything changed.
fn repair_json(value: &mut serde_json::Value) -> bool {
    match value {
        serde_json::Value::Array(items) => repair_all(items.iter_mut()),
        serde_json::Value::Object(object) => {
            let mut changed = false;
            for key in ["arrayValue", "kvlistValue"] {
                if let Some(serde_json::Value::Object(list)) = object.get_mut(key)
                    && !list.contains_key("values")
                {
                    list.insert("values".to_owned(), serde_json::Value::Array(Vec::new()));
                    changed = true;
                }
            }
            for key in DOUBLE_FIELDS {
                let Some(serde_json::Value::String(text)) = object.get(*key) else {
                    continue;
                };
                match text
                    .parse::<f64>()
                    .ok()
                    .and_then(serde_json::Number::from_f64)
                {
                    Some(number) => {
                        object.insert((*key).to_owned(), serde_json::Value::Number(number));
                    }
                    None => {
                        object.remove(*key);
                    }
                }
                changed = true;
            }
            repair_all(object.values_mut()) || changed
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => false,
    }
}

/// Repairs every value, not just up to the first that changed.
fn repair_all<'a>(values: impl Iterator<Item = &'a mut serde_json::Value>) -> bool {
    let mut changed = false;
    for value in values {
        if repair_json(value) {
            changed = true;
        }
    }
    changed
}

/// The response to an export request of `signal`, in the request's encoding:
/// its content type and body. When `rejected` is above zero the response
/// reports a partial success with that count and `message`.
#[must_use]
pub fn encode_response(
    signal: Signal,
    rejected: i64,
    message: &str,
    encoding: Encoding,
) -> (&'static str, Vec<u8>) {
    let partial = rejected.is_positive();
    let body = match encoding {
        Encoding::Protobuf => match signal {
            Signal::Logs => ExportLogsServiceResponse {
                partial_success: partial.then(|| ExportLogsPartialSuccess {
                    rejected_log_records: rejected,
                    error_message: message.to_owned(),
                }),
            }
            .encode_to_vec(),
            Signal::Traces => ExportTraceServiceResponse {
                partial_success: partial.then(|| ExportTracePartialSuccess {
                    rejected_spans: rejected,
                    error_message: message.to_owned(),
                }),
            }
            .encode_to_vec(),
            Signal::Metrics => ExportMetricsServiceResponse {
                partial_success: partial.then(|| ExportMetricsPartialSuccess {
                    rejected_data_points: rejected,
                    error_message: message.to_owned(),
                }),
            }
            .encode_to_vec(),
        },
        Encoding::Json => json_response(signal, partial.then_some(rejected), message),
    };
    (encoding.content_type(), body)
}

/// The JSON response, written by hand so a full success is `{}` rather than
/// a `null` partial success.
fn json_response(signal: Signal, rejected: Option<i64>, message: &str) -> Vec<u8> {
    let Some(rejected) = rejected else {
        return b"{}".to_vec();
    };
    let count_key = match signal {
        Signal::Logs => "rejectedLogRecords",
        Signal::Traces => "rejectedSpans",
        Signal::Metrics => "rejectedDataPoints",
    };
    let mut partial = serde_json::Map::new();
    partial.insert(count_key.to_owned(), serde_json::Value::from(rejected));
    if !message.is_empty() {
        partial.insert("errorMessage".to_owned(), serde_json::Value::from(message));
    }
    serde_json::json!({ "partialSuccess": partial })
        .to_string()
        .into_bytes()
}
