//! The OpenTelemetry receiver's decoding: OTLP over HTTP, as protobuf or
//! JSON, turned into one [`TelemetryMessage`](super::model::TelemetryMessage)
//! per model call, per agent harness as the Collect pages describe. Prompt
//! text is dropped here, before anything is stored.
//!
//! A request goes through [`Encoding::from_content_type`], [`decompress`],
//! one of the `decode_*` functions and, for logs and traces, `extract_from_*`;
//! [`encode_response`] writes the answer. Metrics are decoded and
//! acknowledged only: the per-call events already count every model call.

mod extract;
mod fields;
mod harness;
mod privacy;
mod wire;

#[cfg(test)]
mod tests;

pub use extract::{Extracted, extract_from_logs, extract_from_traces};
pub use privacy::{PROMPT_TEXT_ATTRIBUTES, PROMPT_TEXT_EVENTS};
pub use wire::{
    DecodeError, Encoding, MAX_BODY_BYTES, Signal, decode_logs, decode_metrics, decode_traces,
    decompress, encode_response,
};
