//! `tc rules`: list the rules, and add, move and remove personal ones.
//! Connected, personal rules live in the service, so the web app and the mod
//! show the same list; local, they live in the database on this machine.

use serde::Serialize;
use tokencontroller_core::pattern::TextPattern;
use tokencontroller_core::rules::{Action, Condition, SignalKey, Test};
use tokencontroller_core::{Rule, Tags};

use crate::app::App;
use crate::cli::RulesAction;
use crate::context::{Context, describe};
use crate::error::{Error, Result};
use crate::output;

pub fn run(app: &mut App<'_>, action: Option<RulesAction>) -> Result<()> {
    app.refresh_context(false)?;
    match action.unwrap_or(RulesAction::List) {
        RulesAction::List => {}
        RulesAction::Add { words } => {
            let rule = parse_words(&app.context, &words)?;
            let mut personal = app.context.rules.clone();
            personal.push(rule);
            save(app, &personal)?;
        }
        RulesAction::Move { from, to } => {
            let mut personal = app.context.rules.clone();
            let index = position(from, personal.len())?;
            let target = position(to, personal.len())?;
            let rule = personal.remove(index);
            personal.insert(target, rule);
            save(app, &personal)?;
        }
        RulesAction::Remove { number } => {
            let mut personal = app.context.rules.clone();
            let index = position(number, personal.len())?;
            personal.remove(index);
            save(app, &personal)?;
        }
    }
    list(app)
}

/// A rule in JSON: personal rules carry the number `tc rules remove` takes,
/// and every rule its `text` as the list shows it, then the rule as the core
/// keeps it.
#[derive(Debug, Serialize)]
struct RuleView<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    number: Option<usize>,
    text: String,
    #[serde(flatten)]
    rule: &'a Rule,
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

/// The signals a rule can test, as the command line writes them (Rules:
/// "Signals").
const SIGNALS: [(&str, SignalKey); 6] = [
    ("branch", SignalKey::Branch),
    ("directory", SignalKey::WorkingDirectory),
    ("repository", SignalKey::Repository),
    ("pull-request", SignalKey::PullRequest),
    ("subagent", SignalKey::SubagentType),
    ("ticket", SignalKey::Ticket),
];

/// The tests, as the command line writes them.
const TESTS: [&str; 5] = ["is", "starts-with", "contains", "matches", "is-set"];

/// A rule from its words: `branch starts-with review/ purpose=code-review`,
/// as separate arguments or as one, as the mod's pane passes it. An arrow
/// between the test and the tags may be written. Without a test word, a value
/// that looks like a regular expression is one, and any other must match
/// exactly. `is-set` takes no value.
fn parse_words(context: &Context, words: &[String]) -> Result<Rule> {
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
        [signal, "is-set", set @ ..] => (*signal, "is-set", "", set),
        [signal, test, value, set @ ..] if TESTS.contains(test) => (*signal, *test, *value, set),
        [signal, value, set @ ..] => {
            let test = if looks_like_regex(value) {
                "matches"
            } else {
                "is"
            };
            (*signal, test, *value, set)
        }
        _ => return Err(Error::usage(example)),
    };
    if set.is_empty() || set.iter().any(|pair| !pair.contains('=')) {
        return Err(Error::usage(example));
    }
    let key = SIGNALS
        .iter()
        .find(|(word, _)| *word == signal)
        .map(|(_, key)| *key)
        .ok_or_else(|| {
            let words: Vec<&str> = SIGNALS.iter().map(|(word, _)| *word).collect();
            Error::usage(format!(
                "{signal} is not a signal; use one of {}",
                words.join(", ")
            ))
        })?;
    let test = match test {
        "is" => Test::Is(value.to_owned()),
        "starts-with" => Test::StartsWith(value.to_owned()),
        "contains" => Test::Contains(value.to_owned()),
        "is-set" => Test::IsSet,
        _ => Test::Matches(
            TextPattern::new(value)
                .map_err(|_| Error::usage(format!("{value} is not a regular expression")))?,
        ),
    };
    let mut tags = Tags::new();
    for pair in set {
        let parsed = Tags::parse([*pair]).map_err(|error| Error::usage(error.to_string()))?;
        for (name, tag_value) in parsed.iter() {
            if !context.allows_tag(name.as_str()) {
                return Err(Error::usage(format!(
                    "{name} is not on your organization's list of tag names"
                )));
            }
            tags.insert(name.clone(), tag_value.clone());
        }
    }
    let name = match &test {
        Test::IsSet => format!("{signal} is-set"),
        _ => format!("{signal} {} {value}", test_word(&test)),
    };
    Ok(Rule {
        name,
        when: vec![Condition { signal: key, test }],
        then: Action::SetTags(tags),
    })
}

/// The word for a test.
fn test_word(test: &Test) -> &'static str {
    match test {
        Test::Is(_) => "is",
        Test::StartsWith(_) => "starts-with",
        Test::Contains(_) => "contains",
        Test::Matches(_) => "matches",
        Test::IsSet => "is-set",
    }
}

fn looks_like_regex(value: &str) -> bool {
    value.contains([
        '^', '$', '*', '+', '?', '(', ')', '[', ']', '{', '}', '|', '\\',
    ])
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
    let organization = app.context.organization_rules();
    let personal = &app.context.rules;
    if app.json {
        let numbered: Vec<RuleView<'_>> = personal
            .iter()
            .enumerate()
            .map(|(index, rule)| RuleView {
                number: Some(index.saturating_add(1)),
                text: describe(rule),
                rule,
            })
            .collect();
        let organization: Vec<RuleView<'_>> = organization
            .iter()
            .map(|rule| RuleView {
                number: None,
                text: describe(rule),
                rule,
            })
            .collect();
        return output::json(
            app.out,
            &serde_json::json!({ "rules": numbered, "organization": organization }),
        );
    }
    let heading = if app.context.organization.is_some() {
        "Organization rules, run first:"
    } else {
        "Built-in rules, run first:"
    };
    writeln!(app.out, "{heading}")?;
    for rule in &organization {
        writeln!(app.out, "     {}", describe(rule))?;
    }
    if personal.is_empty() {
        writeln!(
            app.out,
            "No personal rules. Add one with: tc rules add branch starts-with review/ purpose=code-review"
        )?;
    } else {
        writeln!(app.out, "Your rules:")?;
        for (index, rule) in personal.iter().enumerate() {
            writeln!(
                app.out,
                "  {:>2} {}",
                index.saturating_add(1),
                describe(rule)
            )?;
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

    fn words(text: &str) -> Vec<String> {
        vec![text.to_owned()]
    }

    #[test]
    fn words_become_a_core_rule() {
        let context = Context::default();
        let rule = parse_words(
            &context,
            &words("branch starts-with review/ purpose=code-review"),
        )
        .unwrap();
        assert_eq!(rule.name, "branch starts-with review/");
        assert_eq!(
            rule.when,
            [Condition {
                signal: SignalKey::Branch,
                test: Test::StartsWith("review/".into())
            }]
        );
        assert_eq!(
            describe(&rule),
            "branch starts-with review/ → purpose=code-review"
        );
        let regex =
            parse_words(&context, &words("branch ^review/ -> purpose=code-review")).unwrap();
        assert_eq!(regex.name, "branch matches ^review/");
        let exact = parse_words(&context, &words("subagent release-notes ticket=OPS-1")).unwrap();
        assert_eq!(
            exact
                .sets_ticket()
                .map(tokencontroller_core::TagValue::as_str),
            Some("OPS-1")
        );
        let set = parse_words(
            &context,
            &[
                "pull-request".into(),
                "is-set".into(),
                "purpose=code-review".into(),
            ],
        )
        .unwrap();
        assert_eq!(set.name, "pull-request is-set");
        for wrong in [
            "weather is sunny x=y",
            "branch starts-with review/",
            "branch matches ( purpose=x",
            "branch is main Purpose=x",
            "branch",
        ] {
            assert!(parse_words(&context, &words(wrong)).is_err(), "{wrong}");
        }
    }

    #[test]
    fn positions_count_from_one() {
        assert_eq!(position(1, 3).unwrap(), 0);
        assert_eq!(position(3, 3).unwrap(), 2);
        assert!(position(0, 3).is_err());
        assert!(position(4, 3).is_err());
    }
}
