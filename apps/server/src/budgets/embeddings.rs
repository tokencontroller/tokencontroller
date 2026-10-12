//! The embeddings model token forecasts compare text with: any
//! `OpenAI`-compatible `POST {base}/embeddings` API. It sits behind
//! [`Embedder`], so tests use a fake that never touches the network.

use std::error::Error as _;
use std::future::Future;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::similarity::Embedding;
use crate::config::EmbeddingsConfig;

/// The longest text sent to be embedded, in characters. A longer summary
/// or description is cut, so its start is what gets compared.
pub const MAX_TEXT_CHARS: usize = 8_000;

/// The longest error text kept from the model's answer.
const MAX_ERROR_CHARS: usize = 300;

/// How long one call to the model may take.
const TIMEOUT: Duration = Duration::from_secs(30);

/// Why texts could not be embedded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EmbedError {
    #[error("the embeddings model could not be reached: {0}")]
    Unreachable(String),
    #[error("the embeddings model answered {status}: {message}")]
    Refused { status: u16, message: String },
    #[error("the embeddings model's answer could not be read: {0}")]
    Unreadable(String),
    #[error("the embeddings model returned {got} embeddings for {sent} texts")]
    Count { sent: usize, got: usize },
}

/// Something that turns texts into embeddings.
pub trait Embedder: Send + Sync {
    /// The model's name, kept with each embedding: embeddings of different
    /// models are never compared.
    fn model(&self) -> &str;

    /// One embedding per text, in the same order.
    fn embed(
        &self,
        texts: &[String],
    ) -> impl Future<Output = Result<Vec<Embedding>, EmbedError>> + Send;
}

/// An `OpenAI`-compatible embeddings API over HTTP.
#[derive(Clone)]
pub struct HttpEmbedder {
    client: reqwest::Client,
    endpoint: String,
    key: Option<String>,
    model: String,
}

/// Never prints the key.
impl std::fmt::Debug for HttpEmbedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpEmbedder")
            .field("endpoint", &self.endpoint)
            .field("key", &self.key.as_ref().map(|_| "<hidden>"))
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}

impl HttpEmbedder {
    /// A client for the configured API.
    ///
    /// # Errors
    ///
    /// [`EmbedError::Unreachable`] when the HTTP client cannot be built.
    pub fn new(config: &EmbeddingsConfig) -> Result<Self, EmbedError> {
        let client = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .build()
            .map_err(|error| EmbedError::Unreachable(describe(&error)))?;
        Ok(Self {
            client,
            endpoint: format!("{}/embeddings", config.url.trim_end_matches('/')),
            key: config.key.clone(),
            model: config.model.clone(),
        })
    }
}

#[derive(Serialize)]
struct EmbeddingsRequest<'a> {
    model: &'a str,
    input: Vec<String>,
}

impl Embedder for HttpEmbedder {
    fn model(&self) -> &str {
        &self.model
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Embedding>, EmbedError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let body = EmbeddingsRequest {
            model: &self.model,
            input: texts.iter().map(|text| cut(text)).collect(),
        };
        let mut request = self.client.post(&self.endpoint).json(&body);
        if let Some(key) = &self.key {
            request = request.bearer_auth(key);
        }
        let response = request
            .send()
            .await
            .map_err(|error| EmbedError::Unreachable(describe(&error)))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|error| EmbedError::Unreachable(describe(&error)))?;
        if !status.is_success() {
            return Err(EmbedError::Refused {
                status: status.as_u16(),
                message: error_message(&bytes),
            });
        }
        parse_response(&bytes, texts.len())
    }
}

/// A text cut to [`MAX_TEXT_CHARS`].
#[must_use]
pub fn cut(text: &str) -> String {
    text.chars().take(MAX_TEXT_CHARS).collect()
}

/// An error with its causes, such as `error sending request: connection
/// refused`.
fn describe(error: &reqwest::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

#[derive(Deserialize)]
struct EmbeddingsResponse {
    data: Vec<Datum>,
}

#[derive(Deserialize)]
struct Datum {
    embedding: Vec<serde_json::Number>,
    #[serde(default)]
    index: Option<usize>,
}

/// Reads an embeddings answer, `{"data": [{"embedding": [...], "index":
/// 0}]}`, into one embedding per text sent. The numbers are read as text,
/// so no float is ever computed with.
///
/// # Errors
///
/// [`EmbedError::Unreadable`] for an answer of another shape,
/// [`EmbedError::Count`] when it does not hold one embedding per text.
pub fn parse_response(bytes: &[u8], sent: usize) -> Result<Vec<Embedding>, EmbedError> {
    let mut response: EmbeddingsResponse =
        serde_json::from_slice(bytes).map_err(|error| EmbedError::Unreadable(error.to_string()))?;
    if response.data.len() != sent {
        return Err(EmbedError::Count {
            sent,
            got: response.data.len(),
        });
    }
    response.data.sort_by_key(|datum| datum.index);
    response
        .data
        .iter()
        .map(|datum| {
            Embedding::parse(datum.embedding.iter().map(ToString::to_string))
                .map_err(|error| EmbedError::Unreadable(error.to_string()))
        })
        .collect()
}

/// The message of an error answer: `error.message` as `OpenAI` writes it,
/// else the start of the body.
fn error_message(bytes: &[u8]) -> String {
    let parsed: Option<String> = serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .or_else(|| value.pointer("/error"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        });
    let text = parsed.unwrap_or_else(|| String::from_utf8_lossy(bytes).trim().to_owned());
    let text: String = text.chars().take(MAX_ERROR_CHARS).collect();
    if text.is_empty() {
        "no reason given".to_owned()
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_an_openai_answer_in_index_order() {
        let body = br#"{"object":"list","data":[
            {"object":"embedding","index":1,"embedding":[0.5,-0.25]},
            {"object":"embedding","index":0,"embedding":[-0.0071680755,1.25e-5]}
        ],"model":"text-embedding-3-small"}"#;
        let embeddings = parse_response(body, 2).unwrap();
        assert_eq!(embeddings[0].millionths(), [-7168, 12]);
        assert_eq!(embeddings[1].millionths(), [500_000, -250_000]);
    }

    #[test]
    fn an_answer_without_indexes_keeps_its_order() {
        let body = br#"{"data":[{"embedding":[1]},{"embedding":[0]}]}"#;
        let embeddings = parse_response(body, 2).unwrap();
        assert_eq!(embeddings[0].millionths(), [1_000_000]);
        assert_eq!(embeddings[1].millionths(), [0]);
    }

    #[test]
    fn refuses_answers_of_another_shape() {
        assert!(matches!(
            parse_response(b"not json", 1),
            Err(EmbedError::Unreadable(_))
        ));
        assert!(matches!(
            parse_response(br#"{"data":[{"embedding":[]}]}"#, 1),
            Err(EmbedError::Unreadable(_))
        ));
        assert_eq!(
            parse_response(br#"{"data":[{"embedding":[1]}]}"#, 2),
            Err(EmbedError::Count { sent: 2, got: 1 })
        );
        assert_eq!(
            parse_response(br#"{"data":[]}"#, 0).unwrap(),
            Vec::<Embedding>::new()
        );
    }

    #[test]
    fn error_answers_say_why() {
        assert_eq!(
            error_message(br#"{"error":{"message":"Incorrect API key provided","type":"invalid_request_error"}}"#),
            "Incorrect API key provided"
        );
        assert_eq!(
            error_message(br#"{"error":"model not found"}"#),
            "model not found"
        );
        assert_eq!(error_message(b"  Bad Gateway \n"), "Bad Gateway");
        assert_eq!(error_message(b""), "no reason given");
        let long = "x".repeat(1_000);
        assert_eq!(error_message(long.as_bytes()).len(), MAX_ERROR_CHARS);
    }

    #[test]
    fn long_texts_are_cut() {
        let long = "é".repeat(MAX_TEXT_CHARS.checked_add(5).unwrap());
        assert_eq!(cut(&long).chars().count(), MAX_TEXT_CHARS);
        assert_eq!(cut("short"), "short");
    }

    #[test]
    fn the_endpoint_sits_under_the_base_address() {
        let embedder = HttpEmbedder::new(&EmbeddingsConfig {
            url: "https://api.openai.com/v1/".to_owned(),
            key: None,
            model: "text-embedding-3-small".to_owned(),
        })
        .unwrap();
        assert_eq!(embedder.endpoint, "https://api.openai.com/v1/embeddings");
        assert_eq!(embedder.model(), "text-embedding-3-small");
    }

    #[test]
    fn debug_output_hides_the_key() {
        let embedder = HttpEmbedder::new(&EmbeddingsConfig {
            url: "https://api.openai.com/v1".to_owned(),
            key: Some("sk-test-0123".to_owned()),
            model: "text-embedding-3-small".to_owned(),
        })
        .unwrap();
        let shown = format!("{embedder:?}");
        assert!(!shown.contains("sk-test-0123"));
        assert!(shown.contains("<hidden>"));
    }

    #[tokio::test]
    async fn nothing_to_embed_needs_no_call() {
        let embedder = HttpEmbedder::new(&EmbeddingsConfig {
            url: "http://127.0.0.1:9".to_owned(),
            key: None,
            model: "m".to_owned(),
        })
        .unwrap();
        assert_eq!(embedder.embed(&[]).await.unwrap(), Vec::<Embedding>::new());
    }
}
