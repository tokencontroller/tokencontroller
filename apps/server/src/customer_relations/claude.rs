//! Asking Claude a question about the docs, with the docs as the only source.
//!
//! The docs come as one text file (`/llms-full.txt` on the docs site), kept in
//! memory for ten minutes. Each request sends them as a cached system block,
//! so repeated questions pay for them once every few minutes.

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::sync::Mutex;

use super::settings::{CLAUDE_MODEL, Settings};

const DOCS_FRESH_FOR: Duration = Duration::from_mins(10);
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Why a question could not be answered.
#[derive(Debug, thiserror::Error)]
pub enum ClaudeError {
    #[error("TC_ANTHROPIC_API_KEY is not set")]
    NotConfigured,
    #[error("could not read the docs: {0}")]
    Docs(String),
    #[error("the Claude API did not answer: {0}")]
    Unreachable(String),
    #[error("the Claude API refused the request with status {status}: {body}")]
    Status { status: u16, body: String },
    #[error("Claude declined to answer")]
    Refused,
    #[error("Claude's answer did not match the expected shape: {0}")]
    Shape(String),
}

/// The one HTTP client for outside services, made once.
///
/// # Errors
///
/// Only if the TLS setup fails.
pub fn http() -> anyhow::Result<&'static reqwest::Client> {
    static HTTP: OnceLock<reqwest::Client> = OnceLock::new();
    if let Some(client) = HTTP.get() {
        return Ok(client);
    }
    // The service uses ring for TLS everywhere; a second install is harmless.
    let _already_installed = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_mins(2))
        .user_agent(concat!("tokencontroller/", env!("CARGO_PKG_VERSION")))
        .build()?;
    Ok(HTTP.get_or_init(|| client))
}

/// The docs as read at a time from a URL.
type DocsCopy = (Instant, String, Arc<str>);

/// The docs as one text, read from the docs site at most every ten minutes.
/// When the docs site does not answer, the last copy is used.
///
/// # Errors
///
/// When the docs cannot be read and no earlier copy exists.
pub async fn docs_text(settings: &Settings) -> Result<Arc<str>, ClaudeError> {
    static DOCS: OnceLock<Mutex<Option<DocsCopy>>> = OnceLock::new();
    let mut cached = DOCS.get_or_init(|| Mutex::new(None)).lock().await;
    if let Some((read_at, url, text)) = cached.as_ref()
        && *url == settings.docs_text_url
        && read_at.elapsed() < DOCS_FRESH_FOR
    {
        return Ok(Arc::clone(text));
    }
    match fetch_docs(&settings.docs_text_url).await {
        Ok(text) => {
            let text: Arc<str> = Arc::from(text);
            *cached = Some((
                Instant::now(),
                settings.docs_text_url.clone(),
                Arc::clone(&text),
            ));
            Ok(text)
        }
        Err(error) => match cached.as_ref() {
            Some((_, url, text)) if *url == settings.docs_text_url => {
                tracing::warn!(%error, "could not refresh the docs, using the last copy");
                Ok(Arc::clone(text))
            }
            _ => Err(error),
        },
    }
}

async fn fetch_docs(url: &str) -> Result<String, ClaudeError> {
    let http = http().map_err(|error| ClaudeError::Docs(error.to_string()))?;
    let response = http
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| ClaudeError::Docs(error.to_string()))?;
    let text = response
        .text()
        .await
        .map_err(|error| ClaudeError::Docs(error.to_string()))?;
    if text.trim().is_empty() {
        return Err(ClaudeError::Docs("the docs text is empty".to_owned()));
    }
    Ok(text)
}

/// Who wrote one turn of a conversation sent to Claude.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

/// One question to Claude: instructions, the conversation, and the JSON shape
/// the answer must have.
pub struct Ask<'a> {
    pub instructions: &'a str,
    pub turns: Vec<(Role, String)>,
    pub schema: Value,
    /// `low` for quick answers on the website, `medium` for support mail.
    pub effort: &'a str,
}

/// Asks Claude and reads its answer as `T`.
///
/// # Errors
///
/// A [`ClaudeError`] when the docs or the Claude API are not reachable, Claude
/// declines, or the answer has the wrong shape.
pub async fn ask<T: DeserializeOwned>(settings: &Settings, ask: Ask<'_>) -> Result<T, ClaudeError> {
    let key = settings
        .anthropic_api_key
        .as_deref()
        .ok_or(ClaudeError::NotConfigured)?;
    let docs = docs_text(settings).await?;
    let body = request_body(&ask, &docs);

    let http = http().map_err(|error| ClaudeError::Unreachable(error.to_string()))?;
    let response = http
        .post(format!("{}/v1/messages", settings.anthropic_api_url))
        .header("x-api-key", key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .json(&body)
        .send()
        .await
        .map_err(|error| ClaudeError::Unreachable(error.to_string()))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| ClaudeError::Unreachable(error.to_string()))?;
    if !status.is_success() {
        return Err(ClaudeError::Status {
            status: status.as_u16(),
            body: text.chars().take(500).collect(),
        });
    }
    read_answer(&text)
}

/// The Messages API request. The instructions and the docs come first and
/// never change between requests, so the cache marker on the docs covers
/// both. The answer is constrained to `schema`.
fn request_body(ask: &Ask<'_>, docs: &str) -> Value {
    let messages: Vec<Value> = ask
        .turns
        .iter()
        .map(|(role, text)| {
            let role = match role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            json!({ "role": role, "content": text })
        })
        .collect();
    json!({
        "model": CLAUDE_MODEL,
        "max_tokens": 16000,
        "system": [
            { "type": "text", "text": ask.instructions },
            {
                "type": "text",
                "text": format!("<docs>\n{docs}\n</docs>"),
                "cache_control": { "type": "ephemeral" }
            }
        ],
        "messages": messages,
        "output_config": {
            "effort": ask.effort,
            "format": { "type": "json_schema", "schema": ask.schema }
        }
    })
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    content: Vec<Block>,
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct Block {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
}

/// Reads the JSON answer from a Messages API response. A refusal is reported
/// as [`ClaudeError::Refused`], so the caller hands the question to a person.
fn read_answer<T: DeserializeOwned>(response: &str) -> Result<T, ClaudeError> {
    let response: Response =
        serde_json::from_str(response).map_err(|error| ClaudeError::Shape(error.to_string()))?;
    match response.stop_reason.as_deref() {
        Some("refusal") => return Err(ClaudeError::Refused),
        Some("max_tokens") => {
            return Err(ClaudeError::Shape("the answer was cut off".to_owned()));
        }
        _ => {}
    }
    let text: String = response
        .content
        .iter()
        .filter(|block| block.kind == "text")
        .map(|block| block.text.as_str())
        .collect();
    serde_json::from_str(&text).map_err(|error| ClaudeError::Shape(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    struct Reply {
        answer: String,
    }

    #[test]
    fn the_text_blocks_hold_the_json_answer() {
        let response = r#"{"content":[{"type":"thinking","thinking":""},{"type":"text","text":"{\"answer\":"},{"type":"text","text":"\"yes\"}"}],"stop_reason":"end_turn"}"#;
        let reply: Reply = read_answer(response).unwrap();
        assert_eq!(reply.answer, "yes");
    }

    #[test]
    fn a_refusal_is_reported_as_such() {
        let response = r#"{"content":[],"stop_reason":"refusal"}"#;
        assert!(matches!(
            read_answer::<Reply>(response),
            Err(ClaudeError::Refused)
        ));
    }

    #[test]
    fn a_cut_off_or_malformed_answer_is_an_error() {
        let cut = r#"{"content":[{"type":"text","text":"{\"ans"}],"stop_reason":"max_tokens"}"#;
        assert!(matches!(
            read_answer::<Reply>(cut),
            Err(ClaudeError::Shape(_))
        ));
        let wrong =
            r#"{"content":[{"type":"text","text":"{\"other\":1}"}],"stop_reason":"end_turn"}"#;
        assert!(matches!(
            read_answer::<Reply>(wrong),
            Err(ClaudeError::Shape(_))
        ));
    }

    #[test]
    fn the_request_caches_the_docs_and_asks_for_json() {
        let ask = Ask {
            instructions: "Answer.",
            turns: vec![
                (Role::User, "Hi".to_owned()),
                (Role::Assistant, "Hello".to_owned()),
                (Role::User, "Price?".to_owned()),
            ],
            schema: json!({"type": "object"}),
            effort: "low",
        };
        let body = request_body(&ask, "THE DOCS");
        assert_eq!(body["model"], CLAUDE_MODEL);
        assert_eq!(body["system"][1]["cache_control"]["type"], "ephemeral");
        assert!(
            body["system"][1]["text"]
                .as_str()
                .unwrap()
                .contains("THE DOCS")
        );
        assert_eq!(body["messages"][1]["role"], "assistant");
        assert_eq!(body["output_config"]["effort"], "low");
        assert_eq!(body["output_config"]["format"]["type"], "json_schema");
    }
}
