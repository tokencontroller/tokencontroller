//! Reads the `models.dev` catalogue, `api.json`: providers, each with its
//! models, prices in USD per million tokens.

use std::collections::BTreeSet;

use rust_decimal::Decimal;
use serde_json::{Map, Value};

use super::names::{match_key, path_depth};
use super::table::{one_per_model, sorted};
use super::{ModelPrice, PriceError, PriceSource, read_price};

/// Model types that are not priced per token of text.
const SKIPPED_TYPES: [&str; 2] = ["embedding", "reranking"];

/// One provider's listing of a model.
struct Listing<'a> {
    provider: &'a str,
    model: &'a str,
    price: ModelPrice,
}

/// Reads the `models.dev` catalogue into one row per model, sorted. Keeps
/// models that answer in text and have both an input and an output price.
/// Many providers list one model; the model maker's own listing wins, as
/// named by any listing's `canonical_model_id`, then the listing with the
/// shortest provider path, then providers in name order. An entry with an
/// unreadable price is skipped with a warning.
///
/// # Errors
///
/// [`PriceError::BadFile`] if the text is not a JSON object, and
/// [`PriceError::NoPrices`] if no model has prices.
pub fn parse_models_dev(json: &str) -> Result<Vec<ModelPrice>, PriceError> {
    let providers: Map<String, Value> = serde_json::from_str(json)
        .map_err(|error| PriceError::BadFile(PriceSource::ModelsDev, error.to_string()))?;
    let mut makers = BTreeSet::new();
    let mut listings = Vec::new();
    for (provider, entry) in &providers {
        let Some(models) = entry.get("models").and_then(Value::as_object) else {
            continue;
        };
        for (model, entry) in models {
            if let Some(maker) = canonical_maker(entry) {
                makers.insert(maker);
            }
            if !answers_in_text(entry) {
                continue;
            }
            match read_model(provider, model, entry) {
                Ok(Some(price)) => listings.push(Listing {
                    provider,
                    model,
                    price,
                }),
                Ok(None) => {}
                Err(reason) => {
                    tracing::warn!(%provider, %model, %reason, "skipped a models.dev price");
                }
            }
        }
    }
    let candidates = listings.into_iter().map(|listing| {
        let own = (listing.provider.to_lowercase(), match_key(listing.model));
        let rank = (
            !makers.contains(&own),
            path_depth(listing.model),
            listing.provider,
            listing.model,
        );
        (rank, listing.price)
    });
    let prices = sorted(one_per_model(candidates).into_values());
    if prices.is_empty() {
        return Err(PriceError::NoPrices(PriceSource::ModelsDev));
    }
    Ok(prices)
}

/// The maker and model key a listing's `canonical_model_id` names, such as
/// `("anthropic", "claude-sonnet-4-5")` for `anthropic/claude-sonnet-4-5`.
fn canonical_maker(entry: &Value) -> Option<(String, String)> {
    let canonical = entry.get("canonical_model_id").and_then(Value::as_str)?;
    let (maker, model) = canonical.split_once('/')?;
    Some((maker.trim().to_lowercase(), match_key(model)))
}

/// Whether the model answers in text and is no embedding or reranking model.
fn answers_in_text(entry: &Value) -> bool {
    let kind_kept = entry
        .get("type")
        .and_then(Value::as_str)
        .is_none_or(|kind| !SKIPPED_TYPES.contains(&kind));
    let outputs_text = entry
        .pointer("/modalities/output")
        .and_then(Value::as_array)
        .is_none_or(|outputs| outputs.iter().any(|output| output.as_str() == Some("text")));
    kind_kept && outputs_text
}

/// One listing's prices, or `None` if it lacks an input or an output price.
fn read_model(provider: &str, model: &str, entry: &Value) -> Result<Option<ModelPrice>, String> {
    let Some(cost) = entry.get("cost") else {
        return Ok(None);
    };
    let price = |field: &str| read_price(cost.get(field), Decimal::ONE);
    let (Some(input), Some(output)) = (price("input")?, price("output")?) else {
        return Ok(None);
    };
    Ok(Some(ModelPrice {
        model: model.to_owned(),
        provider: Some(provider.to_owned()),
        input: Some(input),
        output: Some(output),
        cache_write: price("cache_write")?,
        cache_read: price("cache_read")?,
    }))
}

#[cfg(test)]
mod tests {
    // Fixture: cut from models.dev's api.json (https://models.dev/api.json,
    // fetched 2026-10-12); the cost fields follow the "Schema Reference" of
    // https://github.com/sst/models.dev, in USD per million tokens.
    use super::*;
    use tokencontroller_core::{Currency, Money};

    const FIXTURE: &str = include_str!("../../../tests/fixtures/prices/models_dev.json");

    fn usd(nanos: i64) -> Money {
        Money::from_nanos(nanos, Currency::USD)
    }

    fn find<'a>(prices: &'a [ModelPrice], model: &str) -> Option<&'a ModelPrice> {
        prices.iter().find(|price| price.model == model)
    }

    #[test]
    fn prices_per_million_are_read_exactly() {
        let prices = parse_models_dev(FIXTURE).unwrap();
        assert_eq!(
            find(&prices, "claude-sonnet-4-5").unwrap(),
            &ModelPrice {
                model: "claude-sonnet-4-5".to_owned(),
                provider: Some("anthropic".to_owned()),
                input: Some(usd(3_000_000_000)),
                output: Some(usd(15_000_000_000)),
                cache_write: Some(usd(3_750_000_000)),
                cache_read: Some(usd(300_000_000)),
            }
        );
        let gemini = find(&prices, "gemini-2.5-pro").unwrap();
        assert_eq!(gemini.provider.as_deref(), Some("google"));
        assert_eq!(gemini.input, Some(usd(1_250_000_000)));
        assert_eq!(gemini.cache_read, Some(usd(125_000_000)));
        assert_eq!(gemini.cache_write, None);
        let spark = find(&prices, "gpt-5.3-codex-spark").unwrap();
        assert_eq!(spark.cache_read, Some(usd(175_000_000)));
        assert_eq!(
            find(&prices, "gpt-3.5-turbo").unwrap().cache_read,
            Some(usd(0))
        );
    }

    #[test]
    fn only_priced_text_models_are_kept() {
        let prices = parse_models_dev(FIXTURE).unwrap();
        let names: Vec<&str> = prices.iter().map(|price| price.model.as_str()).collect();
        assert_eq!(
            names,
            [
                "chatgpt-4o-latest",
                "claude-haiku-4-5",
                "claude-sonnet-4-5",
                "claude-sonnet-4-5-20250929",
                "claude-sonnet-4-5@20250929",
                "gemini-2.5-pro",
                "gpt-3.5-turbo",
                "gpt-4o",
                "gpt-5",
                "gpt-5.3-codex-spark",
            ]
        );
    }

    #[test]
    fn the_makers_own_listing_wins() {
        let prices = parse_models_dev(FIXTURE).unwrap();
        // Beats aihubmix at 3.3 USD and cloudflare-ai-gateway's claude-sonnet-4.5.
        assert_eq!(
            find(&prices, "claude-sonnet-4-5")
                .unwrap()
                .provider
                .as_deref(),
            Some("anthropic")
        );
        // Beats 302ai, which sorts first and lacks cache prices; only the
        // google-vertex-anthropic listing names Anthropic as the maker.
        let dated = find(&prices, "claude-sonnet-4-5-20250929").unwrap();
        assert_eq!(dated.provider.as_deref(), Some("anthropic"));
        assert_eq!(dated.cache_write, Some(usd(3_750_000_000)));
        // Beats cloudflare-ai-gateway at 1.25 USD.
        let gpt4o = find(&prices, "gpt-4o").unwrap();
        assert_eq!(gpt4o.provider.as_deref(), Some("openai"));
        assert_eq!(gpt4o.input, Some(usd(2_500_000_000)));
        // No maker named: the shortest provider path wins over poe's openai/.
        let latest = find(&prices, "chatgpt-4o-latest").unwrap();
        assert_eq!(latest.provider.as_deref(), Some("helicone"));
    }

    #[test]
    fn without_a_maker_providers_go_in_name_order() {
        let json = r#"{
            "zeta": {"models": {"m-1": {"cost": {"input": 2, "output": 2}}}},
            "alpha": {"models": {"m.1": {"cost": {"input": 1, "output": 1}}}},
            "beta": {"models": {"m-1": {"cost": {"input": 0.30000000000000004, "output": 3}}}}
        }"#;
        let prices = parse_models_dev(json).unwrap();
        assert_eq!(
            prices,
            vec![ModelPrice {
                model: "m.1".to_owned(),
                provider: Some("alpha".to_owned()),
                input: Some(usd(1_000_000_000)),
                output: Some(usd(1_000_000_000)),
                cache_write: None,
                cache_read: None,
            }]
        );
        let json =
            r#"{"beta": {"models": {"m": {"cost": {"input": 0.30000000000000004, "output": 3}}}}}"#;
        assert_eq!(
            parse_models_dev(json).unwrap()[0].input,
            Some(usd(300_000_000))
        );
    }

    #[test]
    fn an_unreadable_price_skips_only_its_model() {
        let json = r#"{
            "p": {"models": {
                "good": {"modalities": {"output": ["text"]}, "cost": {"input": 3, "output": 15, "cache_write": null}},
                "text": {"cost": {"input": "3", "output": 15}},
                "negative": {"cost": {"input": 3, "output": -15}},
                "bad_cache": {"cost": {"input": 3, "output": 15, "cache_read": [1]}},
                "no_cost": {"modalities": {"output": ["text"]}},
                "no_output": {"cost": {"input": 3}},
                "image": {"modalities": {"output": ["image"]}, "cost": {"input": 3, "output": 15}},
                "embedding": {"type": "embedding", "cost": {"input": 3, "output": 0}},
                "chat_type": {"type": "chat", "cost": {"input": 1, "output": 2}}
            }},
            "no_models": {"id": "no_models"},
            "odd": 12
        }"#;
        let prices = parse_models_dev(json).unwrap();
        let names: Vec<&str> = prices.iter().map(|price| price.model.as_str()).collect();
        assert_eq!(names, ["chat_type", "good"]);
    }

    #[test]
    fn a_file_without_prices_is_an_error() {
        assert!(matches!(
            parse_models_dev("not json"),
            Err(PriceError::BadFile(PriceSource::ModelsDev, _))
        ));
        assert!(matches!(
            parse_models_dev("\"text\""),
            Err(PriceError::BadFile(PriceSource::ModelsDev, _))
        ));
        assert!(matches!(
            parse_models_dev(r#"{"anthropic": {"models": {}}}"#),
            Err(PriceError::NoPrices(PriceSource::ModelsDev))
        ));
    }
}
