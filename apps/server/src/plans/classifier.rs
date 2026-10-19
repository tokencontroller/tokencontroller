//! The System 1 classifier (docs: Account > Plans and estimates,
//! Self-hosting > Configuration): any `OpenAI`-compatible
//! `POST {base}/chat/completions` API that answers with JSON. It makes an
//! estimate from a description and the history of every tag value, and
//! labels a deviation as scope, trouble or the estimate itself. Without
//! `TC_CLASSIFIER_URL` there is none, and people type estimates.

use std::time::Duration;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use tokencontroller_core::{
    Band, Currency, DeviationLabel, Distribution, Estimate, TagValueHistory, Tags,
};

use crate::config::{ClassifierConfig, Config};
use crate::error::ApiError;
use crate::events::json;

/// How long one call may take.
const TIMEOUT: Duration = Duration::from_mins(1);

/// The longest text sent, in characters: a longer description or context
/// is cut, so its start is what the classifier reads.
pub const MAX_TEXT_CHARS: usize = 8_000;

/// How many tag values of the history go along, newest activity first.
const MAX_HISTORY: usize = 200;

/// The longest error text kept from the classifier's answer.
const MAX_ERROR_CHARS: usize = 300;

/// A deviation label as the classifier gave it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviationLabelled(pub DeviationLabel);

impl DeviationLabelled {
    /// `scope`, `trouble` or `estimate_itself`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self.0 {
            DeviationLabel::Scope => "scope",
            DeviationLabel::Trouble => "trouble",
            DeviationLabel::EstimateItself => "estimate_itself",
        }
    }

    /// The label a text names, in the classifier's or the docs' spelling.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let key: String = text
            .trim()
            .chars()
            .map(|c| match c {
                ' ' | '-' => '_',
                other => other.to_ascii_lowercase(),
            })
            .collect();
        match key.as_str() {
            "scope" => Some(Self(DeviationLabel::Scope)),
            "trouble" => Some(Self(DeviationLabel::Trouble)),
            "estimate_itself" | "estimate" | "the_estimate_itself" => {
                Some(Self(DeviationLabel::EstimateItself))
            }
            _ => None,
        }
    }
}

/// The classifier behind `TC_CLASSIFIER_URL`.
#[derive(Clone)]
pub struct Classifier {
    client: reqwest::Client,
    endpoint: String,
    key: Option<String>,
    model: String,
}

/// Never prints the key.
impl std::fmt::Debug for Classifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Classifier")
            .field("endpoint", &self.endpoint)
            .field("key", &self.key.as_ref().map(|_| "<hidden>"))
            .field("model", &self.model)
            .finish_non_exhaustive()
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    response_format: ResponseFormat,
    temperature: f32,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Deserialize)]
struct ChatReply {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ReplyMessage,
}

#[derive(Deserialize)]
struct ReplyMessage {
    content: String,
}

/// The estimate as the classifier writes it: dollars as decimal strings or
/// numbers.
#[derive(Deserialize)]
struct EstimateReply {
    median: json::Amount,
    #[serde(default)]
    bands: Vec<BandReply>,
}

#[derive(Deserialize)]
struct BandReply {
    #[serde(default)]
    up_to: Option<json::Amount>,
    /// The band's share, in percent.
    p: json::Amount,
}

#[derive(Deserialize)]
struct LabelReply {
    label: String,
}

/// One tag value of the history as the classifier reads it.
#[derive(Serialize)]
struct HistoryLine<'a> {
    name: &'a str,
    key: &'a str,
    title: Option<&'a str>,
    description: Option<&'a str>,
    cost_so_far: String,
    last_activity: Option<String>,
    status: Option<&'a str>,
}

const ESTIMATE_INSTRUCTIONS: &str = "You estimate what a described piece of software work will \
cost in AI agent usage, in US dollars. You are given the work, its tags, optional context, and \
the history of the organization's tag values with the cost each has run up so far. Answer with \
JSON only: {\"median\": \"<dollars>\", \"bands\": [{\"up_to\": \"<dollars>\" or null for the \
last band, \"p\": <percent>}]}. Bands are in rising order and their percentages add up to 100. \
Give no words outside the JSON.";

const LABEL_INSTRUCTIONS: &str = "You explain why AI agent spending on a piece of work deviated \
from its estimate. You are given the estimate, its cost so far and the deviation (plan minus \
cost; negative means over). Answer with JSON only: {\"label\": \"scope\" | \"trouble\" | \
\"estimate_itself\"}. scope: the work grew. trouble: the work ran into trouble. \
estimate_itself: the estimate was off. Give no words outside the JSON.";

impl Classifier {
    /// The classifier the configuration names, if any.
    #[must_use]
    pub fn from_config(config: &Config) -> Option<Self> {
        config.classifier.as_ref().and_then(Self::new)
    }

    /// A client for the configured API, or `None` when the HTTP client
    /// cannot be built.
    #[must_use]
    pub fn new(config: &ClassifierConfig) -> Option<Self> {
        let client = crate::integrations::http::client().ok()?;
        Some(Self {
            client,
            endpoint: format!("{}/chat/completions", config.url.trim_end_matches('/')),
            key: config.key.clone(),
            model: config.model.clone(),
        })
    }

    /// An estimate for `work` with `tags`, from the classifier's reading of
    /// the description, the context and the history.
    ///
    /// # Errors
    ///
    /// [`ApiError::Conflict`] when the classifier cannot be reached,
    /// refuses the call or answers with something that is not an estimate.
    pub async fn estimate(
        &self,
        work: &str,
        tags: &Tags,
        context: Option<&str>,
        history: &[TagValueHistory],
    ) -> Result<Distribution, ApiError> {
        let mut recent: Vec<&TagValueHistory> = history.iter().collect();
        recent.sort_by_key(|item| std::cmp::Reverse(item.last_activity));
        let lines: Vec<HistoryLine<'_>> = recent
            .iter()
            .take(MAX_HISTORY)
            .map(|record| HistoryLine {
                name: record.name.as_str(),
                key: record.key.as_str(),
                title: record.title.as_deref(),
                description: record.description.as_deref().map(|text| cut_str(text, 400)),
                cost_so_far: json::money(record.cost_so_far),
                last_activity: record.last_activity.map(crate::dates::text),
                status: record.source_status.as_deref(),
            })
            .collect();
        let prompt = serde_json::json!({
            "work": cut(work),
            "tags": json::tags_map(tags),
            "context": context.map(cut),
            "history": lines,
        })
        .to_string();
        let reply: EstimateReply = self.ask(ESTIMATE_INSTRUCTIONS, &prompt).await?;
        distribution(&reply)
    }

    /// Why the cost of an estimate's tag set deviates from its plan.
    ///
    /// # Errors
    ///
    /// [`ApiError::Conflict`] when the classifier cannot be reached,
    /// refuses the call or answers with no label.
    pub async fn label_deviation(
        &self,
        estimate: &Estimate,
        cost: &str,
        deviation: &str,
    ) -> Result<DeviationLabelled, ApiError> {
        let prompt = serde_json::json!({
            "work": cut(&estimate.description),
            "tags": json::tags_map(&estimate.tags),
            "estimate": json::money(estimate.distribution.median),
            "markup_percent": estimate.markup_basis_points / 100,
            "from": crate::dates::text(estimate.range.from),
            "to": crate::dates::text(estimate.range.to),
            "cost": cost,
            "deviation": deviation,
        })
        .to_string();
        let reply: LabelReply = self.ask(LABEL_INSTRUCTIONS, &prompt).await?;
        DeviationLabelled::parse(&reply.label).ok_or_else(|| {
            ApiError::Conflict(format!(
                "the classifier answered with {:?}, which is not a deviation label",
                cut_str(&reply.label, 40)
            ))
        })
    }

    async fn ask<T: for<'de> Deserialize<'de>>(
        &self,
        instructions: &str,
        prompt: &str,
    ) -> Result<T, ApiError> {
        let body = ChatRequest {
            model: &self.model,
            messages: vec![
                ChatMessage {
                    role: "system",
                    content: instructions,
                },
                ChatMessage {
                    role: "user",
                    content: prompt,
                },
            ],
            response_format: ResponseFormat {
                kind: "json_object",
            },
            temperature: 0.0,
        };
        let mut request = self
            .client
            .post(&self.endpoint)
            .timeout(TIMEOUT)
            .json(&body);
        if let Some(key) = &self.key {
            request = request.bearer_auth(key);
        }
        let response = request
            .send()
            .await
            .map_err(|error| unreachable(&error.to_string()))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|error| unreachable(&error.to_string()))?;
        if !status.is_success() {
            return Err(ApiError::Conflict(format!(
                "the classifier answered {}: {}",
                status.as_u16(),
                cut_str(&text, MAX_ERROR_CHARS)
            )));
        }
        let reply: ChatReply =
            serde_json::from_str(&text).map_err(|error| unreadable(&error.to_string()))?;
        let content = reply
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message.content)
            .ok_or_else(|| unreadable("no choices"))?;
        parse_reply(&content)
    }
}

/// Reads the JSON in a reply, with or without a code fence around it.
fn parse_reply<T: for<'de> Deserialize<'de>>(content: &str) -> Result<T, ApiError> {
    let trimmed = content.trim();
    let inner = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|rest| rest.strip_suffix("```"))
        .unwrap_or(trimmed);
    serde_json::from_str(inner.trim()).map_err(|error| unreadable(&error.to_string()))
}

/// The core's distribution from the classifier's reply.
fn distribution(reply: &EstimateReply) -> Result<Distribution, ApiError> {
    let median = reply.median.money(Currency::USD)?;
    if median.nanos().is_negative() {
        return Err(unreadable("the median is below zero"));
    }
    let mut bands = Vec::with_capacity(reply.bands.len());
    for band in &reply.bands {
        let up_to = band
            .up_to
            .as_ref()
            .map(|amount| amount.money(Currency::USD))
            .transpose()?;
        let percent = band.p.decimal()?;
        let basis_points = percent
            .checked_mul(Decimal::from(100))
            .and_then(|points| points.round().to_string().parse::<u32>().ok())
            .ok_or_else(|| unreadable("a band's share is not a percentage"))?;
        bands.push(Band {
            up_to,
            basis_points,
        });
    }
    let distribution = Distribution { median, bands };
    distribution
        .check()
        .map_err(|error| unreadable(&error.to_string()))?;
    Ok(distribution)
}

fn unreachable(detail: &str) -> ApiError {
    ApiError::Conflict(format!(
        "the classifier could not be reached: {}",
        cut_str(detail, MAX_ERROR_CHARS)
    ))
}

fn unreadable(detail: &str) -> ApiError {
    ApiError::Conflict(format!(
        "the classifier's answer could not be read: {}",
        cut_str(detail, MAX_ERROR_CHARS)
    ))
}

fn cut(text: &str) -> String {
    cut_str(text, MAX_TEXT_CHARS).to_owned()
}

fn cut_str(text: &str, chars: usize) -> &str {
    match text.char_indices().nth(chars) {
        Some((index, _)) => text.get(..index).unwrap_or(text),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokencontroller_core::Money;

    fn dollars(text: &str) -> Option<Money> {
        Money::parse(text, Currency::USD).ok()
    }

    #[test]
    fn a_reply_becomes_a_distribution() {
        let reply: EstimateReply = parse_reply(
            r#"```json
            {"median": "500", "bands": [{"up_to": "400", "p": 25}, {"up_to": "700", "p": "50"}, {"up_to": null, "p": 25}]}
            ```"#,
        )
        .unwrap();
        let read = distribution(&reply).unwrap();
        assert_eq!(read.median, dollars("500").unwrap());
        assert_eq!(read.bands.len(), 3);
        assert_eq!(read.bands[1].basis_points, 5_000);
        assert_eq!(read.bands[2].up_to, None);

        let off: EstimateReply =
            parse_reply(r#"{"median": "500", "bands": [{"up_to": "400", "p": 70}]}"#).unwrap();
        assert!(distribution(&off).is_err());
        let negative: EstimateReply = parse_reply(r#"{"median": "-1"}"#).unwrap();
        assert!(distribution(&negative).is_err());
        assert!(parse_reply::<EstimateReply>("not json").is_err());
    }

    #[test]
    fn labels_are_read_in_any_spelling() {
        assert_eq!(
            DeviationLabelled::parse("Scope").map(DeviationLabelled::as_str),
            Some("scope")
        );
        assert_eq!(
            DeviationLabelled::parse("the estimate itself").map(DeviationLabelled::as_str),
            Some("estimate_itself")
        );
        assert_eq!(
            DeviationLabelled::parse("trouble").map(DeviationLabelled::as_str),
            Some("trouble")
        );
        assert_eq!(DeviationLabelled::parse("weather"), None);
    }

    #[test]
    fn long_texts_are_cut_on_a_character_boundary() {
        let text = "é".repeat(MAX_TEXT_CHARS + 5);
        assert_eq!(cut(&text).chars().count(), MAX_TEXT_CHARS);
        assert_eq!(cut_str("short", 10), "short");
    }
}
