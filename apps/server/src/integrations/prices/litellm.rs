//! Reads `LiteLLM`'s `model_prices_and_context_window.json`: one entry per
//! model name, prices in USD per token.

use rust_decimal::Decimal;
use serde_json::{Map, Value};

use super::names::path_depth;
use super::table::{one_per_model, sorted};
use super::{ModelPrice, PriceError, PriceSource, read_price};

/// The modes of models that are priced per token of text.
const KEPT_MODES: [&str; 3] = ["chat", "completion", "responses"];

/// The entry that documents the file's fields rather than a model.
const SAMPLE_SPEC: &str = "sample_spec";

/// Reads `LiteLLM`'s price file into one row per model, sorted. Keeps chat,
/// completion and responses models with both an input and an output price.
/// Where several entries name one model, such as `gpt-5` and
/// `azure/eu/gpt-5`, the one with the shortest provider path wins: that is
/// the model maker's own price. An entry with an unreadable price is skipped
/// with a warning.
///
/// # Errors
///
/// [`PriceError::BadFile`] if the text is not a JSON object, and
/// [`PriceError::NoPrices`] if no entry has prices.
pub fn parse_litellm(json: &str) -> Result<Vec<ModelPrice>, PriceError> {
    let entries: Map<String, Value> = serde_json::from_str(json)
        .map_err(|error| PriceError::BadFile(PriceSource::LiteLlm, error.to_string()))?;
    let per_token = Decimal::from(1_000_000_u32);
    let mut candidates = Vec::new();
    for (name, entry) in &entries {
        let mode = entry.get("mode").and_then(Value::as_str);
        if name == SAMPLE_SPEC || !mode.is_some_and(|mode| KEPT_MODES.contains(&mode)) {
            continue;
        }
        match read_entry(name, entry, per_token) {
            Ok(Some(price)) => candidates.push(((path_depth(name), name.as_str()), price)),
            Ok(None) => {}
            Err(reason) => tracing::warn!(model = %name, %reason, "skipped a LiteLLM price"),
        }
    }
    let prices = sorted(one_per_model(candidates).into_values());
    if prices.is_empty() {
        return Err(PriceError::NoPrices(PriceSource::LiteLlm));
    }
    Ok(prices)
}

/// One entry's prices, or `None` if it lacks an input or an output price.
fn read_entry(name: &str, entry: &Value, per_token: Decimal) -> Result<Option<ModelPrice>, String> {
    let price = |field: &str| read_price(entry.get(field), per_token);
    let (Some(input), Some(output)) = (
        price("input_cost_per_token")?,
        price("output_cost_per_token")?,
    ) else {
        return Ok(None);
    };
    Ok(Some(ModelPrice {
        model: name.to_owned(),
        provider: entry
            .get("litellm_provider")
            .and_then(Value::as_str)
            .map(str::to_owned),
        input: Some(input),
        output: Some(output),
        cache_write: price("cache_creation_input_token_cost")?,
        cache_read: price("cache_read_input_token_cost")?,
    }))
}

#[cfg(test)]
mod tests {
    // Fixture: cut from LiteLLM's model_prices_and_context_window.json
    // (https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json,
    // fetched 2026-10-12); its `sample_spec` entry documents the fields.
    use super::*;
    use tokencontroller_core::{Currency, Money};

    const FIXTURE: &str = include_str!("../../../tests/fixtures/prices/litellm.json");

    fn usd(nanos: i64) -> Money {
        Money::from_nanos(nanos, Currency::USD)
    }

    fn find<'a>(prices: &'a [ModelPrice], model: &str) -> Option<&'a ModelPrice> {
        prices.iter().find(|price| price.model == model)
    }

    #[test]
    fn per_token_prices_become_exact_prices_per_million() {
        let prices = parse_litellm(FIXTURE).unwrap();
        let sonnet = find(&prices, "claude-sonnet-4-5").unwrap();
        assert_eq!(
            sonnet,
            &ModelPrice {
                model: "claude-sonnet-4-5".to_owned(),
                provider: Some("anthropic".to_owned()),
                input: Some(usd(3_000_000_000)),
                output: Some(usd(15_000_000_000)),
                cache_write: Some(usd(3_750_000_000)),
                cache_read: Some(usd(300_000_000)),
            }
        );
        let haiku = find(&prices, "claude-haiku-4-5").unwrap();
        assert_eq!(haiku.cache_write, Some(usd(1_250_000_000)));
        assert_eq!(haiku.cache_read, Some(usd(100_000_000)));
        let bedrock = find(&prices, "us.anthropic.claude-sonnet-4-5-20250929-v1:0").unwrap();
        assert_eq!(bedrock.provider.as_deref(), Some("bedrock_converse"));
        assert_eq!(bedrock.input, Some(usd(3_300_000_000)));
        assert_eq!(bedrock.cache_write, Some(usd(4_125_000_000)));
        let gpt5 = find(&prices, "gpt-5").unwrap();
        assert_eq!(gpt5.input, Some(usd(1_250_000_000)));
        assert_eq!(gpt5.cache_read, Some(usd(125_000_000)));
        assert_eq!(gpt5.cache_write, None);
    }

    #[test]
    fn only_chat_completion_and_responses_models_with_prices_are_kept() {
        let prices = parse_litellm(FIXTURE).unwrap();
        let names: Vec<&str> = prices.iter().map(|price| price.model.as_str()).collect();
        assert_eq!(
            names,
            [
                "claude-haiku-4-5",
                "claude-opus-4-5",
                "claude-sonnet-4-5",
                "claude-sonnet-4-5-20250929",
                "claude-sonnet-4-5@20250929",
                "gemini-2.5-pro",
                "gpt-3.5-turbo",
                "gpt-3.5-turbo-instruct",
                "gpt-4o",
                "gpt-5",
                "gpt-5-codex",
                "us.anthropic.claude-sonnet-4-5-20250929-v1:0",
            ]
        );
    }

    #[test]
    fn the_makers_own_entry_wins_over_resellers_and_regions() {
        let prices = parse_litellm(FIXTURE).unwrap();
        // gpt-5 beats azure/eu/gpt-5 at 1.375 USD.
        assert_eq!(
            find(&prices, "gpt-5").unwrap().provider.as_deref(),
            Some("openai")
        );
        // claude-sonnet-4-5 beats vertex_ai/ and vercel_ai_gateway/anthropic/claude-sonnet-4.5.
        let sonnet = find(&prices, "claude-sonnet-4-5").unwrap();
        assert_eq!(sonnet.provider.as_deref(), Some("anthropic"));
        assert_eq!(find(&prices, "claude-sonnet-4.5"), None);
        // gemini-2.5-pro beats gemini/gemini-2.5-pro.
        let gemini = find(&prices, "gemini-2.5-pro").unwrap();
        assert_eq!(
            gemini.provider.as_deref(),
            Some("vertex_ai-language-models")
        );
        assert_eq!(gemini.cache_write, None);
        // The Vertex name with its date stamp keeps its own row.
        let vertex = find(&prices, "claude-sonnet-4-5@20250929").unwrap();
        assert_eq!(
            vertex.provider.as_deref(),
            Some("vertex_ai-anthropic_models")
        );
    }

    #[test]
    fn shorter_provider_paths_win_then_names() {
        let json = r#"{
            "openrouter/anthropic/claude-x": {"mode": "chat", "input_cost_per_token": 1e-06, "output_cost_per_token": 1e-06},
            "vertex_ai/claude-x": {"mode": "chat", "input_cost_per_token": 2e-06, "output_cost_per_token": 2e-06},
            "azure/claude-x": {"mode": "chat", "input_cost_per_token": 3e-06, "output_cost_per_token": 3e-06}
        }"#;
        let prices = parse_litellm(json).unwrap();
        assert_eq!(prices.len(), 1);
        assert_eq!(prices[0].model, "claude-x");
        assert_eq!(prices[0].input, Some(usd(3_000_000_000)));
        assert_eq!(prices[0].provider, None);
    }

    #[test]
    fn an_unreadable_price_skips_only_its_entry() {
        let json = r#"{
            "good": {"mode": "chat", "input_cost_per_token": 3e-06, "output_cost_per_token": 1.5e-05, "cache_read_input_token_cost": null},
            "text": {"mode": "chat", "input_cost_per_token": "3e-06", "output_cost_per_token": 1.5e-05},
            "negative": {"mode": "chat", "input_cost_per_token": 3e-06, "output_cost_per_token": -1e-06},
            "huge": {"mode": "chat", "input_cost_per_token": 3e-06, "output_cost_per_token": 1e30},
            "bad_cache": {"mode": "chat", "input_cost_per_token": 3e-06, "output_cost_per_token": 1.5e-05, "cache_read_input_token_cost": true},
            "no_output": {"mode": "chat", "input_cost_per_token": 3e-06},
            "no_mode": {"input_cost_per_token": 3e-06, "output_cost_per_token": 1.5e-05},
            "odd_mode": {"mode": 7, "input_cost_per_token": 3e-06, "output_cost_per_token": 1.5e-05},
            "not_an_object": 12
        }"#;
        let prices = parse_litellm(json).unwrap();
        assert_eq!(
            prices,
            vec![ModelPrice {
                model: "good".to_owned(),
                provider: None,
                input: Some(usd(3_000_000_000)),
                output: Some(usd(15_000_000_000)),
                cache_write: None,
                cache_read: None,
            }]
        );
    }

    #[test]
    fn a_file_without_prices_is_an_error() {
        assert!(matches!(
            parse_litellm("<html>rate limited</html>"),
            Err(PriceError::BadFile(PriceSource::LiteLlm, _))
        ));
        assert!(matches!(
            parse_litellm("[1, 2]"),
            Err(PriceError::BadFile(PriceSource::LiteLlm, _))
        ));
        assert!(matches!(
            parse_litellm("{}"),
            Err(PriceError::NoPrices(PriceSource::LiteLlm))
        ));
        let only_spec = r#"{"sample_spec": {"mode": "chat", "input_cost_per_token": 0.0, "output_cost_per_token": 0.0}}"#;
        assert!(matches!(
            parse_litellm(only_spec),
            Err(PriceError::NoPrices(PriceSource::LiteLlm))
        ));
    }
}
