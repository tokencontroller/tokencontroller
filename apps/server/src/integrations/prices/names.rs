//! Model names as the price table matches them, and [`lookup`].

use super::ModelPrice;

/// The model name the price table keeps: trimmed, lowercase and without a
/// provider path, so `Bedrock/us-east-1/Foo` and `foo` are one model.
pub(super) fn normalize(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    match lower.rsplit_once('/') {
        Some((_, model)) => model.to_owned(),
        None => lower,
    }
}

/// The key rows of one model share: the normalized name with dots read as
/// dashes, so `claude-sonnet-4.5` and `claude-sonnet-4-5` are one model.
pub(super) fn match_key(name: &str) -> String {
    normalize(name).replace('.', "-")
}

/// How many provider path segments a source's model name has: 0 for
/// `gpt-5`, 2 for `azure/eu/gpt-5`.
pub(super) fn path_depth(name: &str) -> usize {
    name.matches('/').count()
}

/// Finds a model's row in the price table: the exact name first (lowercase,
/// without a provider path), then without a trailing date stamp or `-latest`
/// (`claude-sonnet-4-5-20250929` finds `claude-sonnet-4-5`), then with dots
/// and dashes read alike (`claude-3.5-sonnet` finds `claude-3-5-sonnet`).
/// `None` is an unknown model.
#[must_use]
pub fn lookup<'a>(table: &'a [ModelPrice], model: &str) -> Option<&'a ModelPrice> {
    let wanted = normalize(model);
    let base = without_version(&wanted);
    let find = |matches: &dyn Fn(&str) -> bool| table.iter().find(|row| matches(&row.model));
    find(&|name| name == wanted)
        .or_else(|| find(&|name| name == base))
        .or_else(|| find(&|name| dots_as_dashes_eq(name, &wanted)))
        .or_else(|| find(&|name| dots_as_dashes_eq(name, base)))
}

/// The name without `-latest` and without a trailing date stamp:
/// `-20250929`, `@20250929` or `-2024-08-06`.
fn without_version(name: &str) -> &str {
    let name = name.strip_suffix("-latest").unwrap_or(name);
    if let Some((head, stamp)) = name.rsplit_once(['-', '@'])
        && is_digits(stamp, 8)
        && stamp.starts_with("20")
    {
        return head;
    }
    let mut parts = name.rsplitn(4, '-');
    if let (Some(day), Some(month), Some(year), Some(head)) =
        (parts.next(), parts.next(), parts.next(), parts.next())
        && is_digits(day, 2)
        && is_digits(month, 2)
        && is_digits(year, 4)
        && year.starts_with("20")
    {
        return head;
    }
    name
}

fn is_digits(text: &str, length: usize) -> bool {
    text.len() == length && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// Whether two names are equal with every `.` read as `-`.
fn dots_as_dashes_eq(a: &str, b: &str) -> bool {
    let dash = |byte: u8| if byte == b'.' { b'-' } else { byte };
    a.len() == b.len() && a.bytes().zip(b.bytes()).all(|(x, y)| dash(x) == dash(y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokencontroller_core::{Currency, Money};

    fn row(model: &str, input_nanos: i64) -> ModelPrice {
        ModelPrice {
            model: model.to_owned(),
            provider: None,
            input: Some(Money::from_nanos(input_nanos, Currency::USD)),
            output: None,
            cache_write: None,
            cache_read: None,
        }
    }

    fn table() -> Vec<ModelPrice> {
        vec![
            row("claude-3-5-sonnet", 1),
            row("claude-3-5-sonnet-20240620", 2),
            row("claude-sonnet-4-5", 3),
            row("claude-sonnet-4-5@20250929", 4),
            row("gemini-2.5-pro", 5),
            row("gpt-4o", 6),
            row("gpt-4", 7),
            row("us.anthropic.claude-sonnet-4-5-20250929-v1:0", 8),
        ]
    }

    fn found(model: &str) -> Option<i64> {
        let table = table();
        lookup(&table, model).and_then(|price| price.input.map(Money::nanos))
    }

    #[test]
    fn names_are_lowercase_without_a_provider_path() {
        assert_eq!(
            normalize(" Anthropic/Claude-Sonnet-4-5 "),
            "claude-sonnet-4-5"
        );
        assert_eq!(
            normalize("bedrock/us-east-1/cohere.command-r"),
            "cohere.command-r"
        );
        assert_eq!(normalize("gpt-5"), "gpt-5");
        assert_eq!(
            match_key("openrouter/anthropic/claude-sonnet-4.5"),
            "claude-sonnet-4-5"
        );
        assert_eq!(path_depth("gpt-5"), 0);
        assert_eq!(path_depth("azure/eu/gpt-5"), 2);
    }

    #[test]
    fn the_exact_name_wins() {
        assert_eq!(found("claude-sonnet-4-5"), Some(3));
        assert_eq!(found("Claude-Sonnet-4-5"), Some(3));
        assert_eq!(found("anthropic/claude-sonnet-4-5"), Some(3));
        assert_eq!(found("vertex_ai/claude-sonnet-4-5@20250929"), Some(4));
        assert_eq!(found("claude-3-5-sonnet-20240620"), Some(2));
        assert_eq!(
            found("us.anthropic.claude-sonnet-4-5-20250929-v1:0"),
            Some(8)
        );
    }

    #[test]
    fn a_date_stamp_or_latest_falls_back_to_the_undated_name() {
        assert_eq!(found("claude-sonnet-4-5-20250929"), Some(3));
        assert_eq!(found("claude-3-5-sonnet-20241022"), Some(1));
        assert_eq!(found("claude-3-5-sonnet-latest"), Some(1));
        assert_eq!(found("claude-sonnet-4-5@20260101"), Some(3));
        assert_eq!(found("gpt-4o-2024-08-06"), Some(6));
        assert_eq!(found("gpt-4o-latest"), Some(6));
    }

    #[test]
    fn only_real_date_stamps_are_dropped() {
        assert_eq!(
            without_version("claude-sonnet-4-5-20250929"),
            "claude-sonnet-4-5"
        );
        assert_eq!(without_version("gpt-4o-2024-08-06"), "gpt-4o");
        assert_eq!(without_version("model-12345678"), "model-12345678");
        assert_eq!(without_version("model-2025092"), "model-2025092");
        assert_eq!(without_version("model-202509290"), "model-202509290");
        assert_eq!(without_version("model-2025092x"), "model-2025092x");
        assert_eq!(without_version("model-1999-08-06"), "model-1999-08-06");
        assert_eq!(without_version("model-2024-8-06"), "model-2024-8-06");
        assert_eq!(without_version("model-2024-08-6"), "model-2024-08-6");
        assert_eq!(without_version("model-2024-0x-06"), "model-2024-0x-06");
        assert_eq!(without_version("2024-08-06"), "2024-08-06");
        assert_eq!(
            without_version("gemini-2.5-pro-preview-05-06"),
            "gemini-2.5-pro-preview-05-06"
        );
        assert_eq!(without_version("gpt-4-0613"), "gpt-4-0613");
        assert_eq!(found("gpt-4-0613"), None);
    }

    #[test]
    fn dots_and_dashes_are_read_alike_last() {
        assert_eq!(found("claude-3.5-sonnet"), Some(1));
        assert_eq!(found("gemini-2-5-pro"), Some(5));
        assert_eq!(found("claude-sonnet-4.5"), Some(3));
        assert_eq!(found("claude-3.5-sonnet-20240620"), Some(2));
        assert_eq!(found("claude-3.5-sonnet-20241022"), Some(1));
        assert!(dots_as_dashes_eq("a.b", "a-b"));
        assert!(!dots_as_dashes_eq("a.b", "a_b"));
        assert!(!dots_as_dashes_eq("a.b", "a.bc"));
    }

    #[test]
    fn an_unknown_model_has_no_row() {
        assert_eq!(found("llama-9000"), None);
        assert_eq!(found(""), None);
        assert_eq!(found("claude-sonnet"), None);
        assert_eq!(lookup(&[], "gpt-4o"), None);
    }
}
