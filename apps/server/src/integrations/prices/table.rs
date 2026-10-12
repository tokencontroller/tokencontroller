//! One row per model: picking a source's best row, merging the two sources,
//! and the digest that tells whether a refresh changed the price table.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use sha2::{Digest as _, Sha256};
use tokencontroller_core::Money;

use super::ModelPrice;
use super::names::{match_key, normalize};

/// Keeps one row per model: the one with the lowest rank, ties broken by the
/// rows' own order, so the result never depends on the order rows come in.
/// Model names are normalized on the way.
pub(super) fn one_per_model<R: Ord>(
    candidates: impl IntoIterator<Item = (R, ModelPrice)>,
) -> BTreeMap<String, ModelPrice> {
    let mut best: BTreeMap<String, (R, ModelPrice)> = BTreeMap::new();
    for (rank, mut price) in candidates {
        price.model = normalize(&price.model);
        match best.entry(match_key(&price.model)) {
            Entry::Vacant(slot) => {
                slot.insert((rank, price));
            }
            Entry::Occupied(mut slot) => {
                let (kept_rank, kept) = slot.get();
                if (&rank, &price) < (kept_rank, kept) {
                    slot.insert((rank, price));
                }
            }
        }
    }
    best.into_iter()
        .map(|(key, (_, price))| (key, price))
        .collect()
}

/// The rows in their sort order.
pub(super) fn sorted(rows: impl IntoIterator<Item = ModelPrice>) -> Vec<ModelPrice> {
    let mut rows: Vec<ModelPrice> = rows.into_iter().collect();
    rows.sort();
    rows
}

/// Merges the two sources into one price table, sorted. `LiteLLM` comes
/// first: `models.dev` adds the models `LiteLLM` lacks and fills the kinds of
/// price `LiteLLM` lacks for a model both know. Models match by name,
/// lowercase, without a provider path and with dots read as dashes.
#[must_use]
pub fn merge(litellm: Vec<ModelPrice>, models_dev: Vec<ModelPrice>) -> Vec<ModelPrice> {
    let mut table = one_per_model(litellm.into_iter().map(|row| ((), row)));
    for (key, extra) in one_per_model(models_dev.into_iter().map(|row| ((), row))) {
        match table.entry(key) {
            Entry::Vacant(slot) => {
                slot.insert(extra);
            }
            Entry::Occupied(mut slot) => {
                let row = slot.get_mut();
                row.input = row.input.or(extra.input);
                row.output = row.output.or(extra.output);
                row.cache_write = row.cache_write.or(extra.cache_write);
                row.cache_read = row.cache_read.or(extra.cache_read);
            }
        }
    }
    sorted(table.into_values())
}

/// A SHA-256 digest of the price table's rows in sort order, as 64 hex
/// digits. Equal tables give equal digests whatever their row order, so a
/// refresh whose digest matches the latest version's needs no new version.
#[must_use]
pub fn table_digest(prices: &[ModelPrice]) -> String {
    let mut rows: Vec<&ModelPrice> = prices.iter().collect();
    rows.sort();
    let mut hasher = Sha256::new();
    hasher.update(b"tokencontroller price table v1\n");
    for row in rows {
        hash_text(&mut hasher, Some(&row.model));
        hash_text(&mut hasher, row.provider.as_deref());
        for price in [row.input, row.output, row.cache_write, row.cache_read] {
            hash_price(&mut hasher, price);
        }
    }
    hex::encode(hasher.finalize())
}

/// `0` for none, else `1`, the length as 8 bytes big-endian and the bytes.
fn hash_text(hasher: &mut Sha256, text: Option<&str>) {
    match text {
        None => hasher.update([0]),
        Some(text) => {
            hasher.update([1]);
            let length = u64::try_from(text.len()).unwrap_or(u64::MAX);
            hasher.update(length.to_be_bytes());
            hasher.update(text.as_bytes());
        }
    }
}

/// `0` for none, else `1`, the currency code and the nanos as 8 bytes
/// big-endian.
fn hash_price(hasher: &mut Sha256, price: Option<Money>) {
    match price {
        None => hasher.update([0]),
        Some(price) => {
            hasher.update([1]);
            hasher.update(price.currency().code().as_bytes());
            hasher.update(price.nanos().to_be_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    // Fixtures: cut from LiteLLM's model_prices_and_context_window.json
    // (https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json,
    // fields as its `sample_spec` entry documents them) and models.dev's
    // api.json (https://models.dev/api.json, schema in the "Schema
    // Reference" of https://github.com/sst/models.dev), both fetched
    // 2026-10-12.
    use super::*;
    use crate::integrations::prices::{parse_litellm, parse_models_dev};
    use tokencontroller_core::Currency;

    const LITELLM: &str = include_str!("../../../tests/fixtures/prices/litellm.json");
    const MODELS_DEV: &str = include_str!("../../../tests/fixtures/prices/models_dev.json");

    fn usd(nanos: i64) -> Money {
        Money::from_nanos(nanos, Currency::USD)
    }

    fn row(model: &str, provider: &str, prices: [Option<Money>; 4]) -> ModelPrice {
        let [input, output, cache_write, cache_read] = prices;
        ModelPrice {
            model: model.to_owned(),
            provider: Some(provider.to_owned()),
            input,
            output,
            cache_write,
            cache_read,
        }
    }

    #[test]
    fn litellm_comes_first_and_models_dev_fills_the_gaps() {
        let litellm = vec![
            row(
                "gpt-4o",
                "openai",
                [Some(usd(2_500)), Some(usd(10_000)), None, Some(usd(1_250))],
            ),
            row(
                "claude-sonnet-4-5",
                "anthropic",
                [Some(usd(3_000)), Some(usd(15_000)), None, None],
            ),
        ];
        let models_dev = vec![
            row(
                "gpt-4o",
                "openai",
                [Some(usd(1)), Some(usd(1)), None, Some(usd(1))],
            ),
            row(
                "claude-sonnet-4.5",
                "cloudflare",
                [Some(usd(9)), Some(usd(9)), Some(usd(3_750)), Some(usd(300))],
            ),
            row(
                "gpt-5.3-codex-spark",
                "openai",
                [Some(usd(1_750)), Some(usd(14_000)), None, Some(usd(175))],
            ),
        ];
        assert_eq!(
            merge(litellm, models_dev),
            vec![
                row(
                    "claude-sonnet-4-5",
                    "anthropic",
                    [
                        Some(usd(3_000)),
                        Some(usd(15_000)),
                        Some(usd(3_750)),
                        Some(usd(300))
                    ]
                ),
                row(
                    "gpt-4o",
                    "openai",
                    [Some(usd(2_500)), Some(usd(10_000)), None, Some(usd(1_250))]
                ),
                row(
                    "gpt-5.3-codex-spark",
                    "openai",
                    [Some(usd(1_750)), Some(usd(14_000)), None, Some(usd(175))]
                ),
            ]
        );
    }

    #[test]
    fn either_source_alone_is_a_table() {
        let only = vec![row(
            "gpt-5",
            "openai",
            [Some(usd(1)), Some(usd(2)), None, None],
        )];
        assert_eq!(merge(only.clone(), Vec::new()), only);
        assert_eq!(merge(Vec::new(), only.clone()), only);
        assert_eq!(merge(Vec::new(), Vec::new()), Vec::new());
    }

    #[test]
    fn merging_does_not_depend_on_row_order() {
        let a = row(
            "Anthropic/Claude-Sonnet-4-5",
            "a",
            [Some(usd(1)), Some(usd(1)), None, None],
        );
        let b = row(
            "claude-sonnet-4.5",
            "b",
            [Some(usd(2)), Some(usd(2)), None, None],
        );
        let c = row("gpt-5", "openai", [Some(usd(3)), Some(usd(3)), None, None]);
        let forward = merge(vec![a.clone(), b.clone(), c.clone()], Vec::new());
        let backward = merge(vec![c, b, a], Vec::new());
        assert_eq!(forward, backward);
        assert_eq!(forward.len(), 2);
        assert_eq!(forward[0].model, "claude-sonnet-4-5");
        assert_eq!(forward[0].provider.as_deref(), Some("a"));
    }

    #[test]
    fn a_lower_rank_wins_whatever_the_order() {
        let first = row("gpt-5", "openai", [Some(usd(1)), Some(usd(1)), None, None]);
        let second = row("gpt-5", "azure", [Some(usd(2)), Some(usd(2)), None, None]);
        for candidates in [
            vec![(0, first.clone()), (1, second.clone())],
            vec![(1, second.clone()), (0, first.clone())],
        ] {
            let kept: Vec<ModelPrice> = one_per_model(candidates).into_values().collect();
            assert_eq!(kept, vec![first.clone()]);
        }
    }

    #[test]
    fn the_real_sources_merge_into_one_table() {
        let litellm = parse_litellm(LITELLM).unwrap();
        let models_dev = parse_models_dev(MODELS_DEV).unwrap();
        let table = merge(litellm, models_dev);
        let names: Vec<&str> = table.iter().map(|price| price.model.as_str()).collect();
        assert_eq!(
            names,
            [
                "chatgpt-4o-latest",
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
                "gpt-5.3-codex-spark",
                "us.anthropic.claude-sonnet-4-5-20250929-v1:0",
            ]
        );
        let gpt35 = &table[7];
        assert_eq!(gpt35.provider.as_deref(), Some("openai"));
        assert_eq!(gpt35.input, Some(usd(500_000_000)));
        assert_eq!(gpt35.cache_read, Some(usd(0)));
        let sonnet = &table[3];
        assert_eq!(sonnet.provider.as_deref(), Some("anthropic"));
        assert_eq!(
            [
                sonnet.input,
                sonnet.output,
                sonnet.cache_write,
                sonnet.cache_read
            ],
            [
                Some(usd(3_000_000_000)),
                Some(usd(15_000_000_000)),
                Some(usd(3_750_000_000)),
                Some(usd(300_000_000))
            ]
        );
        let latest = &table[0];
        assert_eq!(latest.provider.as_deref(), Some("helicone"));
        assert_eq!(latest.input, Some(usd(5_000_000_000)));
    }

    #[test]
    fn the_digest_follows_the_rows_not_their_order() {
        let a = row(
            "claude-sonnet-4-5",
            "anthropic",
            [Some(usd(3)), Some(usd(15)), None, Some(usd(0))],
        );
        let b = row("gpt-5", "openai", [Some(usd(1)), Some(usd(10)), None, None]);
        let digest = table_digest(&[a.clone(), b.clone()]);
        assert_eq!(digest.len(), 64);
        assert_eq!(table_digest(&[b.clone(), a.clone()]), digest);

        let mut cheaper = a.clone();
        cheaper.input = Some(usd(2));
        assert_ne!(table_digest(&[cheaper, b.clone()]), digest);

        let mut unpriced = a.clone();
        unpriced.cache_read = None;
        assert_ne!(table_digest(&[unpriced, b.clone()]), digest);

        let mut no_provider = a.clone();
        no_provider.provider = None;
        let mut empty_provider = a.clone();
        empty_provider.provider = Some(String::new());
        assert_ne!(
            table_digest(std::slice::from_ref(&no_provider)),
            table_digest(std::slice::from_ref(&empty_provider))
        );

        let mut euros = a.clone();
        euros.input = Some(Money::from_nanos(3, Currency::EUR));
        assert_ne!(table_digest(&[euros, b.clone()]), digest);

        assert_ne!(table_digest(std::slice::from_ref(&a)), digest);
        assert_ne!(table_digest(&[a.clone(), a, b]), digest);
    }

    #[test]
    fn the_digest_never_changes_between_releases() {
        assert_eq!(
            table_digest(&[]),
            "c94bb4ff09d718f153dc0f60a008449173e86193bc876e869aff9e7458cc3706"
        );
        let table = [
            row(
                "claude-sonnet-4-5",
                "anthropic",
                [
                    Some(usd(3_000_000_000)),
                    Some(usd(15_000_000_000)),
                    Some(usd(3_750_000_000)),
                    Some(usd(300_000_000)),
                ],
            ),
            ModelPrice {
                model: "gpt-5".to_owned(),
                provider: None,
                input: Some(usd(1_250_000_000)),
                output: Some(usd(10_000_000_000)),
                cache_write: None,
                cache_read: Some(usd(125_000_000)),
            },
        ];
        assert_eq!(
            table_digest(&table),
            "03f073a4739c8424384c0eb141331e35e35b92474e8d43611dca6aefe178055b"
        );
    }
}
