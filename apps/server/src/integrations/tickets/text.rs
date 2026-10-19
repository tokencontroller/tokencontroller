//! Plain text from ticket descriptions: HTML and the
//! Atlassian Document Format from Jira Cloud. Tags and markup are dropped,
//! common entities decoded, and whitespace collapsed: a run of whitespace
//! with a line break becomes one line break, any other run one space.

use serde_json::Value;

use super::MAX_DESCRIPTION_CHARS;

/// HTML elements that start a new line.
const BLOCK_TAGS: &[&str] = &[
    "br",
    "p",
    "div",
    "li",
    "tr",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "ul",
    "ol",
    "table",
    "blockquote",
    "pre",
    "hr",
];

/// Atlassian Document Format nodes that end a line.
const ADF_BLOCKS: &[&str] = &[
    "paragraph",
    "heading",
    "codeBlock",
    "blockquote",
    "listItem",
    "taskItem",
    "decisionItem",
    "tableRow",
    "rule",
    "panel",
    "expand",
    "mediaSingle",
];

/// A description as stored: `None` when empty, else cut to at most
/// [`MAX_DESCRIPTION_CHARS`] characters.
pub(super) fn description(mut text: String) -> Option<String> {
    if let Some((cut, _)) = text.char_indices().nth(MAX_DESCRIPTION_CHARS) {
        text.truncate(cut);
        text.truncate(text.trim_end().len());
    }
    (!text.is_empty()).then_some(text)
}

/// Plain text from an HTML fragment.
pub(super) fn from_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find(['<', '&']) {
        let (text, tail) = rest.split_at(at);
        push_source_text(&mut out, text);
        // Both marks are one byte, so this always moves on.
        let after_mark = tail.get(1..).unwrap_or_default();
        rest = if tail.starts_with('&') {
            let (decoded, after) = decode_entity(after_mark).unwrap_or(('&', after_mark));
            out.push(decoded);
            after
        } else {
            skip_markup(after_mark, &mut out)
        };
    }
    push_source_text(&mut out, rest);
    collapse_whitespace(&out)
}

/// Text between tags, where a line break in the source is just a space.
fn push_source_text(out: &mut String, text: &str) {
    out.extend(text.chars().map(|c| if c == '\n' { ' ' } else { c }));
}

/// Skips one tag, comment or `script`/`style` element after its `<`, and
/// returns the rest. A `<` that starts no markup is kept as text.
fn skip_markup<'a>(markup: &'a str, out: &mut String) -> &'a str {
    if let Some(comment) = markup.strip_prefix("!--") {
        return comment.split_once("-->").map_or("", |(_, after)| after);
    }
    let starts_tag = markup
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || matches!(first, '/' | '!' | '?'));
    if !starts_tag {
        out.push('<');
        return markup;
    }
    let Some((tag, after)) = markup.split_once('>') else {
        return "";
    };
    let name: String = tag
        .trim_start_matches('/')
        .chars()
        .take_while(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if BLOCK_TAGS.contains(&name.as_str()) {
        out.push('\n');
    }
    let opens_raw_text = !tag.starts_with('/') && matches!(name.as_str(), "script" | "style");
    if opens_raw_text {
        let closing = after.match_indices("</").find_map(|(at, _)| {
            let rest = after.get(at..)?;
            let closes = rest
                .get(2..)?
                .get(..name.len())?
                .eq_ignore_ascii_case(&name);
            closes.then_some(rest)
        });
        return closing.unwrap_or("");
    }
    after
}

/// Decodes the entity after an `&`, such as `amp;` or `#39;`, and returns
/// the character with the rest of the text. Only the next few characters
/// are searched for the `;`, so a long text without one stays fast.
fn decode_entity(text: &str) -> Option<(char, &str)> {
    let end = text
        .char_indices()
        .take(12)
        .find_map(|(at, c)| (c == ';').then_some(at))?;
    let (name, after) = text.split_at(end);
    let after = after.strip_prefix(';')?;
    let decoded = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        _ => {
            let number = name.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code)?
        }
    };
    Some((decoded, after))
}

/// Plain text from an Atlassian Document Format document.
pub(super) fn from_adf(document: &Value) -> String {
    let mut out = String::new();
    push_adf(document, &mut out);
    collapse_whitespace(&out)
}

fn push_adf(node: &Value, out: &mut String) {
    let kind = node.get("type").and_then(Value::as_str).unwrap_or_default();
    let attribute = |name: &str| {
        node.get("attrs")
            .and_then(|attrs| attrs.get(name))
            .and_then(Value::as_str)
    };
    let inline = match kind {
        "text" => node.get("text").and_then(Value::as_str),
        "hardBreak" => Some("\n"),
        "mention" | "status" => attribute("text"),
        "emoji" => attribute("text").or_else(|| attribute("shortName")),
        "inlineCard" | "blockCard" | "embedCard" => attribute("url"),
        _ => None,
    };
    if let Some(text) = inline {
        out.push_str(text);
    }
    if let Some(children) = node.get("content").and_then(Value::as_array) {
        for child in children {
            push_adf(child, out);
        }
    }
    if ADF_BLOCKS.contains(&kind) {
        out.push('\n');
    }
}

/// Collapses whitespace: a run with a line break becomes one line break,
/// any other run one space; none is left at either end.
pub(super) fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut gap: Option<char> = None;
    for c in text.chars() {
        if c == '\n' {
            gap = Some('\n');
        } else if c.is_whitespace() {
            gap.get_or_insert(' ');
        } else {
            if let Some(gap) = gap.take()
                && !out.is_empty()
            {
                out.push(gap);
            }
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_becomes_plain_text() {
        let html = "<div><b>Checkout</b> fails for\n   <i>guest</i> users.</div>\
                    <div><br></div><ul><li>Step&nbsp;1: add &quot;socks&quot; &amp; pay</li>\
                    <li>Total &lt; 0 &#8364; &#x2014; wrong</li></ul>";
        assert_eq!(
            from_html(html),
            "Checkout fails for guest users.\nStep 1: add \"socks\" & pay\nTotal < 0 € — wrong"
        );
    }

    #[test]
    fn scripts_styles_and_comments_leave_no_text() {
        let html = "<style>p { color: red; }</style><!-- note -->Shown<SCRIPT>alert(1)</script> \
                    too<!-- never closed";
        assert_eq!(from_html(html), "Shown too");
        assert_eq!(from_html("<script>alert(1)"), "");
    }

    #[test]
    fn stray_markup_characters_stay_text() {
        assert_eq!(from_html("a < b && c > d"), "a < b && c > d");
        assert_eq!(
            from_html("AT&T; R&D &bogus; &#xZZ; &#99999999;"),
            "AT&T; R&D &bogus; &#xZZ; &#99999999;"
        );
        assert_eq!(from_html("cut off <a href="), "cut off");
        assert_eq!(from_html("&amp"), "&amp");
        assert_eq!(
            from_html("&averyveryverylongname;"),
            "&averyveryverylongname;"
        );
    }

    #[test]
    fn long_texts_without_closing_marks_stay_fast() {
        let ampersands = "&".repeat(200_000);
        assert_eq!(from_html(&ampersands), ampersands);
        let scripts = "<script>x</script>".repeat(20_000);
        assert_eq!(from_html(&scripts), "");
    }

    #[test]
    fn adf_becomes_plain_text() {
        let document: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/tickets/jira/description_adf.json"
        )))
        .unwrap();
        assert_eq!(
            from_adf(&document),
            "Guest checkout fails\nWhen a guest pays with a saved card the order stays pending.\n\
             Open the cart\nPay as guest @Mia Krystof 👍\nSee https://acme.atlassian.net/browse/KD-200\n\
             Status: BLOCKED\nfn pay() {}\nCard\nResult\nVisa\npending"
        );
        assert_eq!(from_adf(&Value::Null), "");
    }

    #[test]
    fn whitespace_collapses_and_keeps_one_line_break() {
        assert_eq!(collapse_whitespace("  a \t b \n\n\n  c  \r\n"), "a b\nc");
        assert_eq!(collapse_whitespace("\n\n"), "");
    }

    #[test]
    fn descriptions_are_cut_at_the_limit() {
        assert_eq!(description(String::new()), None);
        let exact = "é".repeat(MAX_DESCRIPTION_CHARS);
        assert_eq!(description(exact.clone()), Some(exact.clone()));
        let longer = format!("{exact}ü and more");
        let cut = description(longer).unwrap();
        assert_eq!(cut.chars().count(), MAX_DESCRIPTION_CHARS);
        assert_eq!(cut, exact);
        let space_at_cut = format!(
            "{} tail",
            "a".repeat(MAX_DESCRIPTION_CHARS.checked_sub(1).unwrap())
        );
        assert_eq!(
            description(space_at_cut).unwrap().chars().count(),
            MAX_DESCRIPTION_CHARS.checked_sub(1).unwrap()
        );
    }
}
