//! What the receiver makes of one export request, as JSON and with nothing
//! stored: the account `tokencontroller otlp-decode` prints for the agent
//! harness checks in `scripts/agent-harness/`.

use serde_json::{Value, json};
use time::format_description::well_known::Rfc3339;

use super::extract::{Extracted, extract_from_logs, extract_from_traces};
use super::wire::{
    DecodeError, Encoding, Signal, decode_logs, decode_metrics, decode_traces, decompress,
};
use crate::ingest::model::TelemetryMessage;

/// Reads one export request body the way the receiver does and describes
/// the result: the messages it would store, with their counts, and how many
/// records it dropped, stripped or ignored. Metrics decode to no messages.
///
/// # Errors
///
/// A [`DecodeError`] when the headers or the body cannot be read, exactly as
/// the receiver would refuse the request.
pub fn describe(
    signal: Signal,
    content_type: Option<&str>,
    content_encoding: Option<&str>,
    body: &[u8],
) -> Result<Value, DecodeError> {
    let encoding = Encoding::from_content_type(content_type)?;
    let raw = decompress(content_encoding, body)?;
    let extracted = match signal {
        Signal::Logs => extract_from_logs(&decode_logs(&raw, encoding)?),
        Signal::Traces => extract_from_traces(&decode_traces(&raw, encoding)?),
        Signal::Metrics => {
            decode_metrics(&raw, encoding)?;
            Extracted::default()
        }
    };
    Ok(json!({
        "signal": signal_name(signal),
        "encoding": encoding.content_type(),
        "messages": extracted.messages.iter().map(message_json).collect::<Vec<_>>(),
        "dropped_with_prompt_text": extracted.dropped_with_prompt_text,
        "stripped_prompt_text": extracted.stripped_prompt_text,
        "ignored": extracted.ignored,
    }))
}

const fn signal_name(signal: Signal) -> &'static str {
    match signal {
        Signal::Logs => "logs",
        Signal::Traces => "traces",
        Signal::Metrics => "metrics",
    }
}

fn message_json(message: &TelemetryMessage) -> Value {
    json!({
        "harness": message.harness.name(),
        "session_id": message.session_id,
        "message_id": message.message_id,
        "model": message.model,
        "occurred_at": message.occurred_at.format(&Rfc3339).ok(),
        "tokens": {
            "input": message.tokens.input,
            "output": message.tokens.output,
            "cache_write": message.tokens.cache_write,
            "cache_read": message.tokens.cache_read,
        },
        "reported_cost": message.reported_cost,
        "person_email": message.person_email,
        "person_account_id": message.person_account_id,
        "repository": message.repository,
        "attributes": message.attributes,
    })
}
