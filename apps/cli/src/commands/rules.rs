//! `tc rules`: list the rules, and add, move and remove personal ones.
//! Connected, personal rules live in the service, so the web app and the mod
//! show the same list; local, they live in the database on this machine.

use serde::Serialize;

use crate::app::App;
use crate::cli::RulesAction;
use crate::context::{Condition, Rule, Scope, Signal, Test};
use crate::error::{Error, Result};
use crate::output;

pub fn run(app: &mut App<'_>, action: Option<RulesAction>) -> Result<()> {
    app.refresh_context(false)?;
    match action.unwrap_or(RulesAction::List) {
        RulesAction::List => {}
        RulesAction::Add { words } => {
            let rule = parse_words(app, &words)?;
            let mut personal = personal_rules(app);
            personal.push(rule);
            save(app, &personal)?;
        }
        RulesAction::Move { from, to } => {
            let mut personal = personal_rules(app);
            let index = position(from, personal.len())?;
            let target = position(to, personal.len())?;
            let rule = personal.remove(index);
            personal.insert(target, rule);
            save(app, &personal)?;
        }
        RulesAction::Remove { number } => {
            let mut personal = personal_rules(app);
            let index = position(number, personal.len())?;
            personal.remove(index);
            save(app, &personal)?;
        }
    }
    list(app)
}

/// A rule in JSON: personal rules carry the number `tc rules remove` takes,
/// and every rule its `text` as the list shows it.
#[derive(Debug, Serialize)]
struct RuleView<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    number: Option<usize>,
    text: String,
    #[serde(flatten)]
    rule: &'a Rule,
}

fn personal_rules(app: &App<'_>) -> Vec<Rule> {
    app.context
        .rules
        .iter()
        .filter(|rule| rule.scope == Scope::Personal)
        .cloned()
        .collect()
}

/// A 1-based position from the list into an index.
fn position(number: usize, len: usize) -> Result<usize> {
    number
        .checked_sub(1)
        .filter(|index| *index < len)
        .ok_or_else(|| {
            Error::usage(format!(
                "there is no personal rule {number}; tc rules lists them, numbered from 1"
            ))
        })
}

/// A rule from its words: `branch starts-with review/ purpose=code-review`,
/// as separate arguments or as one, as the mod's pane passes it. An arrow
/// between the test and the tags may be written. Without a test word, a value
/// that looks like a regular expression is one, and any other must match
/// exactly.
fn parse_words(app: &App<'_>, words: &[String]) -> Result<Rule> {
    let split: Vec<String> = match words {
        [one] => one.split_whitespace().map(str::to_owned).collect(),
        _ => words.to_vec(),
    };
    let words: Vec<&str> = split
        .iter()
        .map(String::as_str)
        .filter(|word| !matches!(*word, "→" | "->" | "=>"))
        .collect();
    let example = "write a rule as SIGNAL TEST VALUE NAME=VALUE, such as branch starts-with review/ purpose=code-review";
    let (signal, test, value, set) = match words.as_slice() {
        [signal, test, value, set @ ..] if Test::from_word(test).is_some() => {
            (*signal, *test, *value, set)
        }
        [signal, value, set @ ..] => {
            let test = if looks_like_regex(value) {
                Test::Matches
            } else {
                Test::Is
            };
            (*signal, test.word(), *value, set)
        }
        _ => return Err(Error::usage(example)),
    };
    if set.is_empty() || set.iter().any(|pair| !pair.contains('=')) {
        return Err(Error::usage(example));
    }
    let set: Vec<String> = set.iter().map(|pair| (*pair).to_owned()).collect();
    parse_rule(app, signal, test, value, &set)
}

fn looks_like_regex(value: &str) -> bool {
    value.contains([
        '^', '$', '*', '+', '?', '(', ')', '[', ']', '{', '}', '|', '\\',
    ])
}

fn parse_rule(
    app: &App<'_>,
    signal: &str,
    test: &str,
    value: &str,
    set: &[String],
) -> Result<Rule> {
    let signal = Signal::from_word(signal).ok_or_else(|| {
        let words: Vec<&str> = Signal::ALL.iter().map(|s| s.word()).collect();
        Error::usage(format!(
            "{signal} is not a signal; use one of {}",
            words.join(", ")
        ))
    })?;
    let test = Test::from_word(test).ok_or_else(|| {
        let words: Vec<&str> = Test::ALL.iter().map(|t| t.word()).collect();
        Error::usage(format!(
            "{test} is not a test; use one of {}",
            words.join(", ")
        ))
    })?;
    if test == Test::Matches && regex::Regex::new(value).is_err() {
        return Err(Error::usage(format!("{value} is not a regular expression")));
    }
    let mut tags = std::collections::BTreeMap::new();
    for pair in set {
        let (name, tag_value) = super::name_value(pair)
            .filter(|(_, tag_value)| !tag_value.is_empty())
            .ok_or_else(|| {
                Error::usage(format!(
                    "{pair} is not NAME=VALUE, such as purpose=code-review"
                ))
            })?;
        if !app.context.allows_tag(&name) {
            return Err(Error::usage(format!(
                "{name} is not on your organization's list of tag names"
            )));
        }
        tags.insert(name, tag_value);
    }
    Ok(Rule {
        scope: Scope::Personal,
        when: Condition {
            signal,
            test,
            value: value.to_owned(),
        },
        set: tags,
    })
}

fn save(app: &mut App<'_>, personal: &[Rule]) -> Result<()> {
    if let Some(client) = app.client() {
        client.put_rules(personal)?;
        app.refresh_context(true)?;
    } else {
        let texts = personal
            .iter()
            .map(serde_json::to_string)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        app.store.set_personal_rules(&texts)?;
        app.refresh_context(false)?;
    }
    Ok(())
}

fn list(app: &mut App<'_>) -> Result<()> {
    let organization: Vec<&Rule> = app
        .context
        .rules
        .iter()
        .filter(|rule| rule.scope == Scope::Organization)
        .collect();
    let personal: Vec<&Rule> = app
        .context
        .rules
        .iter()
        .filter(|rule| rule.scope == Scope::Personal)
        .collect();
    if app.json {
        let numbered: Vec<RuleView<'_>> = personal
            .iter()
            .enumerate()
            .map(|(index, rule)| RuleView {
                number: Some(index.saturating_add(1)),
                text: rule.to_string(),
                rule,
            })
            .collect();
        let organization: Vec<RuleView<'_>> = organization
            .iter()
            .map(|rule| RuleView {
                number: None,
                text: rule.to_string(),
                rule,
            })
            .collect();
        return output::json(
            app.out,
            &serde_json::json!({ "rules": numbered, "organization": organization }),
        );
    }
    writeln!(
        app.out,
        "Built in, in this order: ticket key in branch, in pull request title, in prompts or session title; repository mapped to a project."
    )?;
    if !organization.is_empty() {
        writeln!(app.out, "Organization rules (run first):")?;
        for rule in &organization {
            writeln!(app.out, "     {rule}")?;
        }
    }
    if personal.is_empty() {
        writeln!(
            app.out,
            "No personal rules. Add one with: tc rules add branch starts-with review/ purpose=code-review"
        )?;
    } else {
        writeln!(app.out, "Your rules:")?;
        for (index, rule) in personal.iter().enumerate() {
            writeln!(app.out, "  {:>2} {rule}", index.saturating_add(1))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regular_expressions_are_recognized() {
        assert!(looks_like_regex("^review/"));
        assert!(looks_like_regex(r"KD-\d+"));
        assert!(!looks_like_regex("review/"));
        assert!(!looks_like_regex("main"));
    }

    #[test]
    fn positions_count_from_one() {
        assert_eq!(position(1, 3).unwrap(), 0);
        assert_eq!(position(3, 3).unwrap(), 2);
        assert!(position(0, 3).is_err());
        assert!(position(4, 3).is_err());
    }
}
