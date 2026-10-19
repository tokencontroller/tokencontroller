//! `tc export --csv`: every message on this machine as CSV in the token
//! entry schema, one row per message part, with a `tags.<name>` column per
//! tag name. Enough to report by hand when the organization has no service,
//! and what the web app's CSV import reads.

use std::collections::BTreeSet;

use tokencontroller_core::{State, TagName};

use crate::app::App;
use crate::error::Result;
use crate::log::SessionView;
use crate::output::{decimal, utc};

const FIXED: [&str; 10] = [
    "agent_harness",
    "model",
    "started_at",
    "ended_at",
    "tokens.input",
    "tokens.output",
    "tokens.cache_read",
    "tokens.cache_write",
    "summary",
    "feature",
];

const MORE: [&str; 10] = [
    "project",
    "session_id",
    "message_id",
    "fraction",
    "cost",
    "currency",
    "label",
    "status",
    "assigned_by",
    "reported_cost",
];

pub fn run(app: &mut App<'_>) -> Result<()> {
    app.prepare()?;
    let state = app.fold()?;
    let sessions = crate::log::sessions(&state, &app.store.sessions()?)?;
    write_csv(app.out, &state, &sessions)
}

pub fn write_csv(
    out: &mut dyn std::io::Write,
    state: &State,
    sessions: &[SessionView],
) -> Result<()> {
    let currency = state.currency().code();
    let built_in = [TagName::feature(), TagName::project(), TagName::person()];
    let tag_names: BTreeSet<String> = state
        .lines()
        .iter()
        .flat_map(|line| line.tags.iter())
        .filter(|(name, _)| !built_in.contains(name))
        .map(|(name, _)| name.to_string())
        .collect();
    let mut header: Vec<String> = FIXED.iter().map(|name| (*name).to_owned()).collect();
    header.extend(tag_names.iter().map(|name| format!("tags.{name}")));
    header.extend(MORE.iter().map(|name| (*name).to_owned()));
    writeln!(out, "{}", line(&header))?;
    for session in sessions {
        for message in &session.messages {
            let lines: Vec<&tokencontroller_core::state::Line> = state
                .lines()
                .iter()
                .filter(|line| {
                    line.session_id.as_str() == session.session_id
                        && line.message_id.as_str() == message.message_id
                })
                .collect();
            let count = u64::try_from(lines.len()).unwrap_or(1).max(1);
            for part in &lines {
                let fraction = part_fraction(session, &part.assigned, count);
                let mut row = vec![
                    session.harness.name().to_owned(),
                    message.model.model.clone(),
                    utc(message.at),
                    utc(message.at),
                    message.tokens.input.to_string(),
                    message.tokens.output.to_string(),
                    message.tokens.cache_read.to_string(),
                    message.tokens.cache_write.to_string(),
                    session.summary.clone().unwrap_or_default(),
                    part.tags
                        .get(&TagName::feature())
                        .map(ToString::to_string)
                        .unwrap_or_default(),
                ];
                row.extend(tag_names.iter().map(|name| {
                    part.tags
                        .get_str(name)
                        .map(ToString::to_string)
                        .unwrap_or_default()
                }));
                row.extend([
                    part.tags
                        .get(&TagName::project())
                        .map(ToString::to_string)
                        .unwrap_or_default(),
                    session.session_id.clone(),
                    message.message_id.clone(),
                    fraction,
                    if message.priced {
                        decimal(part.amount)
                    } else {
                        String::new()
                    },
                    currency.clone(),
                    part.label.as_str().to_owned(),
                    session.status.as_str().to_owned(),
                    session.assigned_by.clone(),
                    message.reported_cost.map(decimal).unwrap_or_default(),
                ]);
                writeln!(out, "{}", line(&row))?;
            }
        }
    }
    Ok(())
}

/// The fraction of a message's part: the session's part with the same
/// assigned tags, or an equal share.
fn part_fraction(
    session: &SessionView,
    assigned: &tokencontroller_core::Tags,
    count: u64,
) -> String {
    session
        .hand_split
        .as_ref()
        .and_then(|split| {
            let sum = split
                .parts
                .iter()
                .fold(0_u64, |sum, part| sum.saturating_add(part.weight))
                .max(1);
            split
                .parts
                .iter()
                .find(|part| part.tags == *assigned)
                .map(|part| {
                    part.weight
                        .saturating_mul(10_000)
                        .checked_div(sum)
                        .unwrap_or(0)
                })
        })
        .map_or_else(
            || crate::log::fraction_text(10_000_u64.checked_div(count).unwrap_or(10_000)),
            crate::log::fraction_text,
        )
}

/// One CSV line: fields with a comma, quote or line break are quoted, and a
/// field a spreadsheet would run as a formula is prefixed with a quote mark.
fn line(fields: &[String]) -> String {
    fields
        .iter()
        .map(|field| field_text(field))
        .collect::<Vec<_>>()
        .join(",")
}

fn field_text(field: &str) -> String {
    let safe = if field.starts_with(['=', '+', '-', '@'])
        && crate::amounts::parse_nanos(field).is_none()
    {
        format!("'{field}")
    } else {
        field.to_owned()
    };
    if safe.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", safe.replace('"', "\"\""))
    } else {
        safe
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_are_quoted_when_they_must_be() {
        assert_eq!(field_text("plain"), "plain");
        assert_eq!(field_text("a,b"), "\"a,b\"");
        assert_eq!(field_text("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(field_text("two\nlines"), "\"two\nlines\"");
        assert_eq!(field_text("=SUM(A1)"), "'=SUM(A1)");
        assert_eq!(field_text("-1.5"), "-1.5");
        assert_eq!(line(&["a".into(), "b,c".into()]), "a,\"b,c\"");
    }

    #[test]
    fn the_header_starts_as_the_import_expects() {
        assert_eq!(FIXED.first(), Some(&"agent_harness"));
        assert_eq!(FIXED.get(7), Some(&"tokens.cache_write"));
    }

    #[test]
    fn one_row_per_message_part_and_a_column_per_tag_name() {
        let run = crate::log::Run {
            tags: tokencontroller_core::Tags::parse(["purpose=code-review"]).unwrap(),
            ..crate::log::Run::default()
        };
        let (_dir, state, views) =
            crate::log::fixtures::folded(&run, &crate::context::Context::default(), false);
        let mut out = Vec::new();
        write_csv(&mut out, &state, &views).unwrap();
        let text = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 7, "{text}");
        assert_eq!(
            lines[0],
            "agent_harness,model,started_at,ended_at,tokens.input,tokens.output,tokens.cache_read,tokens.cache_write,summary,feature,tags.purpose,project,session_id,message_id,fraction,cost,currency,label,status,assigned_by,reported_cost"
        );
        let first: Vec<&str> = lines[1].split(',').collect();
        assert_eq!(first[0], "claude-code");
        assert_eq!(first[1], "claude-sonnet-4-5");
        assert_eq!(first[8], "Fix checkout");
        assert_eq!(first[9], "KD-214");
        assert_eq!(first[10], "code-review");
        assert_eq!(first[14], "1");
        assert_eq!(first[15], "4.50");
        assert_eq!(first[16], "USD");
        assert_eq!(first[17], "direct");
        assert_eq!(first[18], "open");
        assert_eq!(first[20], "0.50");
        let split: Vec<&str> = lines[5].split(',').collect();
        assert_eq!(split[14], "0.6");
        assert_eq!(split[15], "2.70");
    }

    #[test]
    fn with_no_tag_names_the_header_is_the_fixed_columns() {
        let (_dir, state, views) = crate::log::fixtures::folded(
            &crate::log::Run::default(),
            &crate::context::Context::default(),
            false,
        );
        let mut out = Vec::new();
        write_csv(&mut out, &state, &views).unwrap();
        let text = String::from_utf8(out).unwrap();
        let header: Vec<&str> = text.lines().next().unwrap().split(',').collect();
        let expected: Vec<&str> = FIXED.iter().chain(MORE.iter()).copied().collect();
        assert_eq!(header, expected);
    }
}
