//! The CLI's offline price list: published prices of the Claude and OpenAI
//! models whose session files it reads, from `LiteLLM` (see
//! `scripts/update_prices.py`). It turns token counts into list value when
//! no service is connected. The service keeps its own versioned price tables
//! and computes list value again when a token entry arrives.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;

use crate::amounts::parse_nanos;
use crate::tokens::Tokens;

/// The snapshot shipped inside the binary.
const SNAPSHOT: &str = include_str!("../prices.json");

/// Token counts above which some models charge more per token, counted over
/// all input of one request.
const LONG_CONTEXT_TOKENS: u64 = 200_000;

/// One model's prices in US dollar nano-units per million tokens.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelPrice {
    pub input: i64,
    pub output: i64,
    pub cache_write: i64,
    pub cache_write_1h: i64,
    pub cache_read: i64,
    pub long_context: Option<Box<Self>>,
}

#[derive(Deserialize)]
struct Snapshot {
    models: HashMap<String, HashMap<String, String>>,
}

/// The price list, read once.
pub fn table() -> &'static HashMap<String, ModelPrice> {
    static TABLE: OnceLock<HashMap<String, ModelPrice>> = OnceLock::new();
    TABLE.get_or_init(|| parse(SNAPSHOT).unwrap_or_default())
}

fn parse(text: &str) -> Option<HashMap<String, ModelPrice>> {
    let snapshot: Snapshot = serde_json::from_str(text).ok()?;
    let mut table = HashMap::new();
    for (model, fields) in snapshot.models {
        let get = |name: &str| fields.get(name).and_then(|value| parse_nanos(value));
        let (Some(input), Some(output)) = (get("input"), get("output")) else {
            continue;
        };
        let cache_write = get("cache_write").unwrap_or(input);
        let cache_read = get("cache_read").unwrap_or(input);
        // One-hour cache writes cost twice the input price where the list
        // has no figure (Anthropic's published rule).
        let cache_write_1h = get("cache_write_1h").unwrap_or(input.saturating_mul(2));
        let long_context = get("input_above_200k").map(|long_input| {
            Box::new(ModelPrice {
                input: long_input,
                output: get("output_above_200k").unwrap_or(output),
                cache_write: get("cache_write_above_200k").unwrap_or(cache_write),
                cache_write_1h,
                cache_read: get("cache_read_above_200k").unwrap_or(cache_read),
                long_context: None,
            })
        });
        table.insert(
            model,
            ModelPrice {
                input,
                output,
                cache_write,
                cache_write_1h,
                cache_read,
                long_context,
            },
        );
    }
    Some(table)
}

/// The price of a model by the name a session file gives it. Tries the name
/// as written, then without a `[1m]`-style suffix, then without a trailing
/// date, then without a provider prefix.
pub fn find(model: &str) -> Option<&'static ModelPrice> {
    let table = table();
    candidates(model)
        .into_iter()
        .find_map(|name| table.get(&name))
}

fn candidates(model: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut name = model.trim().to_ascii_lowercase();
    names.push(name.clone());
    if let Some((base, _)) = name.split_once('[') {
        name = base.to_owned();
        names.push(name.clone());
    }
    if let Some((_, base)) = name.rsplit_once('/') {
        name = base.to_owned();
        names.push(name.clone());
    }
    if let Some(base) = without_date(&name) {
        names.push(base);
    }
    names
}

/// `claude-sonnet-4-5-20250929` → `claude-sonnet-4-5`,
/// `gpt-5-2025-08-07` → `gpt-5`.
fn without_date(name: &str) -> Option<String> {
    if let Some((base, date)) = name.rsplit_once('-')
        && date.len() == 8
        && date.bytes().all(|b| b.is_ascii_digit())
    {
        return Some(base.to_owned());
    }
    let parts: Vec<&str> = name.rsplitn(4, '-').collect();
    if let [day, month, year, base] = parts.as_slice()
        && year.len() == 4
        && month.len() == 2
        && day.len() == 2
        && [day, month, year]
            .iter()
            .all(|part| part.bytes().all(|b| b.is_ascii_digit()))
    {
        return Some((*base).to_owned());
    }
    None
}

/// The list value of one model call in US dollar nano-units, rounded half up
/// to the nano. `None` when the model is not on the list.
pub fn list_value(model: &str, tokens: &Tokens) -> Option<i64> {
    value_at(find(model)?, tokens)
}

/// The list value of token counts at one model's prices.
fn value_at(price: &ModelPrice, tokens: &Tokens) -> Option<i64> {
    let long_input = tokens
        .input
        .checked_add(tokens.cache_write_5m)?
        .checked_add(tokens.cache_write_1h)?
        .checked_add(tokens.cache_read)?;
    let price = match &price.long_context {
        Some(long) if long_input > LONG_CONTEXT_TOKENS => long,
        _ => price,
    };
    let parts = [
        (tokens.input, price.input),
        (tokens.output, price.output),
        (tokens.cache_write_5m, price.cache_write),
        (tokens.cache_write_1h, price.cache_write_1h),
        (tokens.cache_read, price.cache_read),
    ];
    let mut total: i128 = 0;
    for (count, per_million) in parts {
        let part = i128::from(count).checked_mul(i128::from(per_million))?;
        total = total.checked_add(part)?;
    }
    // Per million tokens, rounded half up.
    let rounded = total.checked_add(500_000)?.checked_div(1_000_000)?;
    i64::try_from(rounded).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(input: u64, output: u64, cache_write_5m: u64, cache_read: u64) -> Tokens {
        Tokens {
            input,
            output,
            cache_write_5m,
            cache_write_1h: 0,
            cache_read,
        }
    }

    #[test]
    fn the_snapshot_reads() {
        assert!(table().len() > 50);
        let sonnet = find("claude-sonnet-4-5").unwrap();
        assert_eq!(sonnet.input, 3_000_000_000);
        assert_eq!(sonnet.output, 15_000_000_000);
    }

    #[test]
    fn model_names_are_found_in_the_forms_agents_write() {
        assert!(find("claude-sonnet-4-5-20250929").is_some());
        assert!(find("claude-opus-5-5[1m]").is_some());
        assert!(find("anthropic/claude-sonnet-4-5").is_some());
        assert!(find("gpt-5-2025-08-07").is_some());
        assert!(find("CLAUDE-SONNET-4-5").is_some());
        assert!(find("no-such-model").is_none());
    }

    #[test]
    fn dates_are_only_stripped_when_they_are_dates() {
        assert_eq!(without_date("gpt-5-2025-08-07").as_deref(), Some("gpt-5"));
        assert_eq!(
            without_date("claude-x-20250929").as_deref(),
            Some("claude-x")
        );
        assert_eq!(without_date("claude-x-2025092"), None);
        assert_eq!(without_date("gpt-5-mini"), None);
        assert_eq!(without_date("a-20x5-08-07"), None);
    }

    #[test]
    fn list_value_adds_every_kind_of_token() {
        let table = parse(
            r#"{"models":{"m":{"input":"3","output":"15","cache_write":"3.75","cache_read":"0.3"}}}"#,
        )
        .unwrap();
        let price = table.get("m").unwrap();
        assert_eq!(price.cache_write_1h, 6_000_000_000);
        // 1M input at $3 and 1M output at $15.
        let value = list_value("claude-sonnet-4-5", &tokens(1_000_000, 1_000_000, 0, 0));
        assert_eq!(value, Some(18_000_000_000));
        let small = list_value("claude-sonnet-4-5", &tokens(1, 0, 0, 0));
        assert_eq!(small, Some(3_000));
        assert_eq!(list_value("no-such-model", &tokens(1, 1, 1, 1)), None);
    }

    #[test]
    fn rounding_is_half_up_at_the_nano() {
        // 0.0004 nano per token: 1,249 tokens round down, 1,250 round up.
        let table = parse(r#"{"models":{"m":{"input":"0.0000004","output":"0"}}}"#).unwrap();
        let price = table.get("m").unwrap();
        assert_eq!(price.input, 400);
        assert_eq!(value_at(price, &tokens(1_249, 0, 0, 0)), Some(0));
        assert_eq!(value_at(price, &tokens(1_250, 0, 0, 0)), Some(1));
        // Real prices are whole nanos per token: $0.10 per million is 100.
        let haiku = find("claude-haiku-4-5").unwrap();
        assert_eq!(value_at(haiku, &tokens(0, 0, 0, 4)), Some(400));
    }

    #[test]
    fn one_hour_cache_writes_have_their_own_price() {
        let mut counts = tokens(0, 0, 0, 0);
        counts.cache_write_1h = 1_000_000;
        assert_eq!(
            list_value("claude-sonnet-4-5", &counts),
            Some(6_000_000_000)
        );
    }

    #[test]
    fn long_requests_use_the_long_context_price() {
        let table = parse(
            r#"{"models":{"m":{"input":"3","output":"15","input_above_200k":"6","output_above_200k":"22.5"}}}"#,
        )
        .unwrap();
        let price = table.get("m").unwrap();
        let long = price.long_context.as_ref().unwrap();
        assert_eq!(long.input, 6_000_000_000);
        assert_eq!(long.output, 22_500_000_000);
        assert_eq!(long.cache_read, 3_000_000_000);
    }
}
