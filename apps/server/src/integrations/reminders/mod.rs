//! Late token sheet reminders: one mail to the agent manager when a token
//! sheet's cadence period is over and it is not submitted. Nothing is
//! blocked and nothing is capped (docs page Cadence and token sheets).

use time::Date;

const TEXT: &str = include_str!("late-token-sheet.txt");
const HTML: &str = include_str!("late-token-sheet.html");

/// The subject of every late token sheet reminder.
pub const SUBJECT: &str = "Your token sheet is late";

/// A token sheet whose cadence period is over and that is not submitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LateSheet {
    /// The agent manager's email address.
    pub email: String,
    /// The first day of the cadence period.
    pub period_start: Date,
    /// The last day of the cadence period.
    pub period_end: Date,
    /// How many token entries wait on it.
    pub token_entries: u64,
}

/// A reminder ready to send, with a plain-text and an HTML part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reminder {
    pub to: String,
    pub subject: &'static str,
    pub text: String,
    pub html: String,
}

/// A cadence period in words, such as `5 to 11 October 2026`, or
/// `28 September to 4 October 2026` across a month.
#[must_use]
pub fn period_words(start: Date, end: Date) -> String {
    let end_words = format!("{} {} {}", end.day(), end.month(), end.year());
    if start == end {
        end_words
    } else if start.year() != end.year() {
        format!(
            "{} {} {} to {end_words}",
            start.day(),
            start.month(),
            start.year()
        )
    } else if start.month() == end.month() {
        format!("{} to {end_words}", start.day())
    } else {
        format!("{} {} to {end_words}", start.day(), start.month())
    }
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn fill(template: &str, values: &[(&str, String)]) -> String {
    values
        .iter()
        .fold(template.to_owned(), |text, (name, value)| {
            text.replace(&format!("{{{{{name}}}}}"), value)
        })
}

/// The reminder for one late token sheet. `public_url` is `TC_PUBLIC_URL`;
/// the link opens the person's token sheet in the web app.
#[must_use]
pub fn late_sheet_reminder(sheet: &LateSheet, public_url: &str) -> Reminder {
    let public_url = public_url.trim_end_matches('/');
    let url = format!("{public_url}/token-sheet?period={}", sheet.period_start);
    let host = public_url
        .split_once("://")
        .map_or(public_url, |(_, rest)| rest)
        .to_owned();
    let values = [
        ("period", period_words(sheet.period_start, sheet.period_end)),
        ("token_entries", sheet.token_entries.to_string()),
        ("token_sheet_url", url),
        ("host", host),
    ];
    let escaped: Vec<(&str, String)> = values
        .iter()
        .map(|(name, value)| (*name, escape_html(value)))
        .collect();
    Reminder {
        to: sheet.email.clone(),
        subject: SUBJECT,
        text: fill(TEXT, &values),
        html: fill(HTML, &escaped),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn periods_read_as_words() {
        assert_eq!(
            period_words(date!(2026 - 10 - 05), date!(2026 - 10 - 11)),
            "5 to 11 October 2026"
        );
        assert_eq!(
            period_words(date!(2026 - 09 - 28), date!(2026 - 10 - 04)),
            "28 September to 4 October 2026"
        );
        assert_eq!(
            period_words(date!(2026 - 12 - 28), date!(2027 - 01 - 03)),
            "28 December 2026 to 3 January 2027"
        );
        assert_eq!(
            period_words(date!(2026 - 10 - 05), date!(2026 - 10 - 05)),
            "5 October 2026"
        );
    }

    #[test]
    fn the_reminder_fills_both_parts() {
        let reminder = late_sheet_reminder(
            &LateSheet {
                email: "m.ortner@kestrel.example".to_owned(),
                period_start: date!(2026 - 10 - 05),
                period_end: date!(2026 - 10 - 11),
                token_entries: 14,
            },
            "https://app.tokencontroller.com/",
        );
        assert_eq!(reminder.to, "m.ortner@kestrel.example");
        assert_eq!(reminder.subject, "Your token sheet is late");
        for part in [&reminder.text, &reminder.html] {
            assert!(part.contains("5 to 11 October 2026"));
            assert!(part.contains("14 token entries"));
            assert!(part.contains("https://app.tokencontroller.com/token-sheet?period=2026-10-05"));
            assert!(part.contains("Token Controller · app.tokencontroller.com"));
            assert!(!part.contains("{{"));
        }
    }

    #[test]
    fn html_values_are_escaped() {
        assert_eq!(
            escape_html("<a href=\"x\">&'"),
            "&lt;a href=&quot;x&quot;&gt;&amp;&#39;"
        );
    }
}
