//! `tc rules`: list the rules, and add, move and remove personal ones.
//! Connected, personal rules live in the organization, so the web app and
//! the mod show the same list; local, they are events in the log on this
//! machine.

use serde::Serialize;
use tokencontroller_core::{
    Record, Rule, Scope, Signal, TagName, TagValue, Tags, TextPattern, Then,
};

use crate::app::App;
use crate::cli::RulesAction;
use crate::context::{Context, PERSON};
use crate::error::{Error, Result};
use crate::output;

pub fn run(app: &mut App<'_>, action: Option<RulesAction>) -> Result<()> {
    app.refresh_context(false)?;
    match action.unwrap_or(RulesAction::List) {
        RulesAction::List => {}
        RulesAction::Add { words } => {
            let rule = parse_words(&app.context, &words)?;
            let mut rules = personal(app)?;
            rules.push(rule);
            save(app, &rules)?;
        }
        RulesAction::Move { from, to } => {
            let mut rules = personal(app)?;
            let index = position(from, rules.len())?;
            let target = position(to, rules.len())?;
            let rule = rules.remove(index);
            rules.insert(target, rule);
            save(app, &rules)?;
        }
        RulesAction::Remove { number } => {
            let mut rules = personal(app)?;
            let index = position(number, rules.len())?;
            rules.remove(index);
            save(app, &rules)?;
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
/// "Signals"), and the core's signal each reads.
fn signal_of(word: &str) -> Option<Signal> {
    Some(match word {
        "branch" => Signal::Branch,
        "directory" => Signal::Resource("working_directory".to_owned()),
        "repository" => Signal::Repository,
        "pull-request" => Signal::PullRequestTitle,
        "subagent" => Signal::AgentName,
        "feature" => Signal::PromptKeys,
        _ => {
            let name = word
                .strip_prefix("config:")
                .or_else(|| word.strip_prefix("config."))?;
            Signal::Config(name.to_owned())
        }
    })
}

/// The word for a signal.
fn signal_word(signal: &Signal) -> String {
    match signal {
        Signal::Branch => "branch".to_owned(),
        Signal::WorkingDirectory => "directory".to_owned(),
        Signal::Repository => "repository".to_owned(),
        Signal::SubagentType => "subagent-type".to_owned(),
        Signal::PullRequestTitle => "pull-request".to_owned(),
        Signal::PromptKeys => "feature".to_owned(),
        Signal::AgentName => "subagent".to_owned(),
        Signal::Config(name) => format!("config:{name}"),
        Signal::Resource(name) if name == "working_directory" => "directory".to_owned(),
        Signal::Resource(name) => format!("resource:{name}"),
    }
}

const SIGNAL_WORDS: &str =
    "branch, directory, repository, pull-request, config:NAME, subagent, feature";

/// The tests, as the command line writes them.
const TESTS: [&str; 5] = ["is", "starts-with", "contains", "matches", "is-set"];

/// A rule from its words: `branch starts-with review/ purpose=code-review`,
/// as separate arguments or as one, as the mod's pane passes it. An arrow
/// between the test and the tags may be written. Without a test word, a value
/// that looks like a regular expression is one, and any other must match
/// exactly. `is-set` takes no value. `feature=$1` sets the feature a group
/// in a `matches` test captures.
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
    let signal = signal_of(signal).ok_or_else(|| {
        Error::usage(format!(
            "{signal} is not a signal; use one of {SIGNAL_WORDS}"
        ))
    })?;
    let source = match test {
        "is" => format!("^{}$", regex::escape(value)),
        "starts-with" => format!("^{}", regex::escape(value)),
        "contains" => regex::escape(value),
        "is-set" => ".".to_owned(),
        _ => value.to_owned(),
    };
    let then = then_of(context, set, test)?;
    let source = if matches!(then, Then::KeyOf { .. }) {
        name_first_group(&source)
    } else {
        source
    };
    let pattern = TextPattern::new(&source)
        .map_err(|_| Error::usage(format!("{value} is not a regular expression")))?;
    let name = match test {
        "is-set" => format!("{} is-set", signal_word(&signal)),
        _ => format!("{} {test} {value}", signal_word(&signal)),
    };
    Ok(Rule {
        name,
        scope: Scope::Person(TagValue::new(PERSON)?),
        order: 0,
        signal,
        pattern,
        then,
    })
}

/// What the rule sets, from its `NAME=VALUE` words: one tag, or with
/// `NAME=$1` the key a group in a `matches` test captures.
fn then_of(context: &Context, set: &[&str], test: &str) -> Result<Then> {
    let example = "write a rule as SIGNAL TEST VALUE NAME=VALUE, such as branch starts-with review/ purpose=code-review";
    let pairs: Vec<(String, String)> = set
        .iter()
        .filter_map(|pair| super::name_value(pair))
        .collect();
    if pairs.len() != set.len() {
        return Err(Error::usage(example));
    }
    if pairs.len() > 1 {
        return Err(Error::usage("a rule sets one tag; add a rule per tag name"));
    }
    let (name, value) = pairs.first().cloned().unwrap_or_default();
    if value == "$1" {
        if test != "matches" {
            return Err(Error::usage(
                "$1 needs a matches test with a group, such as branch matches (KD-\\d+) feature=$1",
            ));
        }
        return Ok(Then::KeyOf {
            name: TagName::new(&name)?,
        });
    }
    if !context.allows_tag(&name) {
        return Err(Error::usage(format!(
            "{name} is not on your organization's list of tag names"
        )));
    }
    let tags = Tags::parse([format!("{name}={value}").as_str()])?;
    let Some((name, key)) = tags.iter().next() else {
        return Err(Error::usage(example));
    };
    Ok(Then::Tag {
        name: name.clone(),
        key: key.clone(),
    })
}

/// Names the first unnamed group `key`, so the core finds the captured
/// text: `^(KD-\d+)` becomes `^(?P<key>KD-\d+)`.
fn name_first_group(source: &str) -> String {
    if source.contains("(?P<key>") || source.contains("(?<key>") {
        return source.to_owned();
    }
    let mut out = String::new();
    let mut escaped = false;
    let mut done = false;
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
        } else if c == '(' && !done && chars.peek() != Some(&'?') {
            out.push_str("?P<key>");
            done = true;
        }
    }
    out
}

fn looks_like_regex(value: &str) -> bool {
    value.contains([
        '^', '$', '*', '+', '?', '(', ')', '[', ']', '{', '}', '|', '\\',
    ])
}

/// A rule as the list shows it: its name and what it sets.
pub fn describe(rule: &Rule) -> String {
    match &rule.then {
        Then::Tag { name, key } => format!("{} → {name}={key}", rule.name),
        Then::KeyOf { name } => format!("{} → {name}=$1", rule.name),
    }
}

/// The person's own rules in order: from the service when connected, else
/// from the log on this machine.
pub fn personal(app: &App<'_>) -> Result<Vec<Rule>> {
    if app.connection().is_some() {
        return Ok(app.context.my_rules.clone());
    }
    let mut rules: Vec<Rule> = local_rule_events(app)?
        .into_iter()
        .map(|(_, rule)| rule)
        .collect();
    rules.sort_by_key(|rule| rule.order);
    Ok(rules)
}

/// The rule events on this machine still in force, with their ids.
fn local_rule_events(app: &App<'_>) -> Result<Vec<(tokencontroller_core::EventId, Rule)>> {
    let events = app.store.events()?;
    let reversed: std::collections::BTreeSet<&tokencontroller_core::EventId> = events
        .iter()
        .filter_map(|event| match &event.record {
            Record::Reversal(id) => Some(id),
            _ => None,
        })
        .collect();
    Ok(events
        .iter()
        .filter(|event| !reversed.contains(&event.id))
        .filter_map(|event| match &event.record {
            Record::Rule(rule) => Some((event.id.clone(), rule.clone())),
            _ => None,
        })
        .collect())
}

/// Replaces the person's rules: in the organization when connected, else
/// by reversing the old events and recording the new ones in order.
pub fn save(app: &mut App<'_>, rules: &[Rule]) -> Result<()> {
    let ordered: Vec<Rule> = rules
        .iter()
        .enumerate()
        .map(|(index, rule)| Rule {
            order: u32::try_from(index).unwrap_or(u32::MAX),
            ..rule.clone()
        })
        .collect();
    if let Some(client) = app.client() {
        client.put_rules(&ordered)?;
        app.refresh_context(true)?;
        return Ok(());
    }
    for (id, _) in local_rule_events(app)? {
        app.record(Record::Reversal(id), None)?;
    }
    for rule in ordered {
        app.record(Record::Rule(rule), None)?;
    }
    Ok(())
}

fn list(app: &mut App<'_>) -> Result<()> {
    let organization: Vec<Rule> = if app.connection().is_some() {
        app.context.rules.clone()
    } else {
        app.pattern()
            .map(|pattern| crate::log::local_rules(&pattern))
            .unwrap_or_default()
    };
    let personal = personal(app)?;
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
    let heading = if app.connection().is_some() {
        "Organization rules, run first:"
    } else {
        "Default rules, run first:"
    };
    writeln!(app.out, "{heading}")?;
    if organization.is_empty() {
        writeln!(
            app.out,
            "     none: set tc config feature-key-pattern to find features in branches"
        )?;
    }
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
        assert_eq!(rule.signal, Signal::Branch);
        assert_eq!(rule.pattern.as_str(), "^review/");
        assert_eq!(
            describe(&rule),
            "branch starts-with review/ → purpose=code-review"
        );
        let regex =
            parse_words(&context, &words("branch ^review/ -> purpose=code-review")).unwrap();
        assert_eq!(regex.name, "branch matches ^review/");
        let exact = parse_words(&context, &words("subagent release-notes feature=OPS-1")).unwrap();
        assert_eq!(exact.signal, Signal::AgentName);
        assert_eq!(exact.pattern.as_str(), "^release\\-notes$");
        assert!(exact.pattern.is_match("release-notes"));
        assert!(!exact.pattern.is_match("release-notes-2"));
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
        let captured =
            parse_words(&context, &words(r"branch matches ^(KD-\d+) feature=$1")).unwrap();
        assert_eq!(captured.pattern.as_str(), r"^(?P<key>KD-\d+)");
        assert_eq!(captured.pattern.key_in("KD-12-x"), Some("KD-12"));
        assert_eq!(
            describe(&captured),
            r"branch matches ^(KD-\d+) → feature=$1"
        );
        let config = parse_words(&context, &words("config:client is acme client=acme")).unwrap();
        assert_eq!(config.signal, Signal::Config("client".into()));
        assert_eq!(signal_word(&config.signal), "config:client");
        assert_eq!(
            signal_word(&Signal::Resource("working_directory".into())),
            "directory"
        );
        for wrong in [
            "weather is sunny x=y",
            "branch starts-with review/",
            "branch matches ( purpose=x",
            "branch is main Purpose=x",
            "branch is main a=1 b=2",
            "branch is main feature=$1",
            "branch",
        ] {
            assert!(parse_words(&context, &words(wrong)).is_err(), "{wrong}");
        }
    }

    #[test]
    fn the_first_group_is_named_key() {
        assert_eq!(name_first_group(r"^(KD-\d+)"), r"^(?P<key>KD-\d+)");
        assert_eq!(name_first_group(r"(?:x)(y)"), r"(?:x)(?P<key>y)");
        assert_eq!(name_first_group(r"\((a)"), r"\((?P<key>a)");
        assert_eq!(name_first_group(r"(?P<key>a)"), r"(?P<key>a)");
        assert_eq!(name_first_group("plain"), "plain");
    }

    #[test]
    fn positions_count_from_one() {
        assert_eq!(position(1, 3).unwrap(), 0);
        assert_eq!(position(3, 3).unwrap(), 2);
        assert!(position(0, 3).is_err());
        assert!(position(4, 3).is_err());
    }

    #[test]
    fn every_signal_word_is_understood() {
        let context = Context::default();
        let repository = parse_words(
            &context,
            &words("repository is kestrel/ops purpose=operations"),
        )
        .unwrap();
        assert_eq!(repository.signal, Signal::Repository);
        assert_eq!(
            describe(&repository),
            "repository is kestrel/ops → purpose=operations"
        );
        let directory = parse_words(
            &context,
            &words("directory contains /ops purpose=operations"),
        )
        .unwrap();
        assert_eq!(
            directory.signal,
            Signal::Resource("working_directory".into())
        );
        assert!(directory.pattern.is_match("/home/me/ops/x"));
        let title = parse_words(
            &context,
            &words(r"pull-request matches (KD-\d+) feature=$1"),
        )
        .unwrap();
        assert_eq!(title.signal, Signal::PullRequestTitle);
        assert_eq!(
            describe(&title),
            r"pull-request matches (KD-\d+) → feature=$1"
        );
        let prompts = parse_words(&context, &words("feature is KD-1 purpose=spike")).unwrap();
        assert_eq!(prompts.signal, Signal::PromptKeys);
        assert_eq!(signal_word(&prompts.signal), "feature");
        assert_eq!(signal_word(&Signal::PullRequestTitle), "pull-request");
        assert_eq!(signal_word(&Signal::AgentName), "subagent");
    }

    #[test]
    fn tag_names_follow_the_organization_once_connected() {
        let context: Context = serde_json::from_str(
            r#"{"organization": {"name": "Kestrel", "currency": "EUR"}, "tag_names": ["team"]}"#,
        )
        .unwrap();
        assert!(parse_words(&context, &words("branch is main team=web")).is_ok());
        assert!(parse_words(&context, &words("branch is main feature=KD-1")).is_ok());
        let refused = parse_words(&context, &words("branch is main purpose=spike")).unwrap_err();
        assert!(refused.to_string().contains("purpose"), "{refused}");
    }
}
