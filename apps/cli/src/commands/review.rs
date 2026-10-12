//! `tc review`, `tc tag` and `tc split`: the draft token entries, and fixing
//! the exceptions by hand (Attribute: Review and submit).

use std::collections::BTreeMap;

use crate::app::App;
use crate::entries::Entry;
use crate::error::{Error, Result};
use crate::output::{self, EntryView};
use crate::store::Status;

pub fn run(app: &mut App<'_>, all: bool) -> Result<()> {
    app.prepare()?;
    let statuses: &[Status] = if all {
        &[Status::Draft, Status::Rejected, Status::Submitted]
    } else {
        &[Status::Draft, Status::Rejected]
    };
    let entries = app.entries(statuses)?;
    if app.json {
        let views: Vec<EntryView> = entries.iter().map(EntryView::of).collect();
        return output::json(app.out, &serde_json::json!({ "token_entries": views }));
    }
    if entries.is_empty() {
        writeln!(
            app.out,
            "No draft token entries. New sessions show up here after they start."
        )?;
        return Ok(());
    }
    print_entries(app, &entries)
}

/// The review list as the docs show it, with each summary under its entry.
pub fn print_entries(app: &mut App<'_>, entries: &[Entry]) -> Result<()> {
    let today = app.today();
    let mut rows = vec![vec![
        "#".to_owned(),
        "when".to_owned(),
        "cost".to_owned(),
        "tags".to_owned(),
        "by".to_owned(),
    ]];
    let mut summaries = Vec::new();
    for entry in entries {
        rows.push(vec![
            entry.row.number.to_string(),
            output::when(entry.started_ms, today),
            output::cost(entry),
            tags_cell(entry),
            output::reason(entry),
        ]);
        summaries.push(entry.summary.clone());
    }
    // Render the table, then put each summary under its row.
    let mut table = Vec::new();
    output::table(&mut table, &rows, &[0, 2])?;
    let text = String::from_utf8_lossy(&table).into_owned();
    let mut lines = text.lines();
    if let Some(header) = lines.next() {
        writeln!(app.out, "{header}")?;
    }
    for (line, summary) in lines.zip(summaries) {
        writeln!(app.out, "{line}")?;
        if let Some(summary) = summary {
            writeln!(app.out, "        {}", first_line(&summary))?;
        }
    }
    Ok(())
}

fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default().trim();
    let shown: String = line.chars().take(100).collect();
    if shown.chars().count() < line.chars().count() {
        format!("{shown}…")
    } else {
        shown
    }
}

fn tags_cell(entry: &Entry) -> String {
    match entry.splits.as_slice() {
        [] => String::new(),
        [only] => output::tag_line(&only.tags),
        splits => splits
            .iter()
            .map(|split| format!("{}% {}", split.share_percent, output::tag_line(&split.tags)))
            .collect::<Vec<_>>()
            .join(" | "),
    }
}

/// `tc tag N name=value…`
pub fn tag(app: &mut App<'_>, number: i64, pairs: &[String]) -> Result<()> {
    app.prepare()?;
    let entry = editable(app, number)?;
    let mut named: BTreeMap<String, String> = entry.row.named_tags.clone();
    for pair in pairs {
        let (name, value) = super::name_value(pair).ok_or_else(|| {
            Error::usage(format!("{pair} is not NAME=VALUE, such as ticket=KD-214"))
        })?;
        if !app.context.allows_tag(&name) {
            let list = app
                .context
                .organization
                .as_ref()
                .map(|organization| organization.tag_names.join(", "))
                .unwrap_or_default();
            return Err(Error::usage(format!(
                "{name} is not on your organization's list of tag names ({list})"
            )));
        }
        if value.is_empty() {
            named.remove(&name);
        } else {
            named.insert(name, value);
        }
    }
    app.store.set_named_tags(&entry.row.local_id, &named)?;
    // A ticket set by hand replaces a split by hand.
    if named.contains_key("ticket") && entry.row.hand_split.is_some() {
        app.store.set_hand_split(&entry.row.local_id, None)?;
    }
    show_one(app, number)
}

/// `tc split N TICKET=SHARE…`
pub fn split(app: &mut App<'_>, number: i64, pairs: &[String]) -> Result<()> {
    app.prepare()?;
    let entry = editable(app, number)?;
    let mut shares: Vec<(String, u64)> = Vec::new();
    for pair in pairs {
        let (ticket, share) = super::name_value(pair).ok_or_else(|| {
            Error::usage(format!("{pair} is not TICKET=SHARE, such as KD-231=60"))
        })?;
        let share: u64 = share
            .parse()
            .ok()
            .filter(|share| *share > 0)
            .ok_or_else(|| {
                Error::usage(format!(
                    "the share in {pair} must be a whole number above 0"
                ))
            })?;
        if shares.iter().any(|(existing, _)| *existing == ticket) {
            return Err(Error::usage(format!("{ticket} is named twice")));
        }
        shares.push((ticket, share));
    }
    if shares.len() < 2 {
        return Err(Error::usage(
            "a split needs two tickets or more; to set one ticket use tc tag N ticket=KEY",
        ));
    }
    app.store
        .set_hand_split(&entry.row.local_id, Some(&shares))?;
    let mut named = entry.row.named_tags.clone();
    if named.remove("ticket").is_some() {
        app.store.set_named_tags(&entry.row.local_id, &named)?;
    }
    show_one(app, number)
}

fn editable(app: &App<'_>, number: i64) -> Result<Entry> {
    let entry = app.entry(number)?;
    if entry.row.status == Status::Submitted {
        return Err(Error::usage(format!(
            "token entry {number} is submitted; ask your people manager to reject it if it needs a change"
        )));
    }
    Ok(entry)
}

fn show_one(app: &mut App<'_>, number: i64) -> Result<()> {
    let entry = app.entry(number)?;
    if app.json {
        return output::json(
            app.out,
            &serde_json::json!({ "token_entry": EntryView::of(&entry) }),
        );
    }
    print_entries(app, &[entry])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_summaries_are_cut_to_one_line() {
        assert_eq!(first_line("Fixed it\nmore"), "Fixed it");
        let long = "x".repeat(150);
        let cut = first_line(&long);
        assert_eq!(cut.chars().count(), 101);
        assert!(cut.ends_with('…'));
        assert_eq!(first_line(&"y".repeat(100)).chars().count(), 100);
    }
}
