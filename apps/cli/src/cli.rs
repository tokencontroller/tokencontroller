//! The command line, as the CLI reference page lists it.
//!
//! Built with clap's builder rather than its derive macros: the derive writes
//! `#[allow(clippy::…)]` into the generated code, which the forbid-level lints
//! of the harness refuse (ADR 0013).

use clap::{Arg, ArgAction, ArgMatches, Command as ClapCommand, value_parser};

use crate::error::{Error, Result};

#[derive(Debug, PartialEq, Eq)]
pub struct Args {
    /// Print one JSON value instead of a table.
    pub json: bool,
    pub command: Command,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Connect {
        url: String,
    },
    Status,
    Review {
        all: bool,
    },
    Tag {
        number: i64,
        tags: Vec<String>,
    },
    Split {
        number: i64,
        shares: Vec<String>,
    },
    Submit {
        tags: Vec<String>,
        signals: Vec<String>,
    },
    Report {
        by: String,
        filter: Vec<String>,
        since: Option<String>,
    },
    Export,
    Rules {
        action: Option<RulesAction>,
    },
    Config {
        key: Option<String>,
        value: Option<String>,
    },
    Record,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RulesAction {
    List,
    /// The rule's words: `branch starts-with review/ purpose=code-review`,
    /// as separate arguments or as one.
    Add {
        words: Vec<String>,
    },
    Move {
        from: usize,
        to: usize,
    },
    Remove {
        number: usize,
    },
}

fn number_arg() -> Arg {
    Arg::new("number")
        .help("The token entry's number from tc review.")
        .required(true)
        .value_parser(value_parser!(i64))
}

fn pairs_arg(name: &'static str, value_name: &'static str, help: &'static str) -> Arg {
    Arg::new(name)
        .help(help)
        .value_name(value_name)
        .required(true)
        .num_args(1..)
}

/// The whole command line, with help text.
pub fn command() -> ClapCommand {
    ClapCommand::new("tc")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Token Controller: reads your AI agents' session files, places token entries with your rules, and submits token sheets.")
        .after_help("Works with no service connected. Docs: https://docs.tokencontroller.com/reference/cli")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .arg(
            Arg::new("json")
                .long("json")
                .global(true)
                .action(ArgAction::SetTrue)
                .help("Print one JSON value instead of a table, for scripts and the Claude Code mod."),
        )
        .subcommand(
            ClapCommand::new("connect")
                .about("Connect to an organization with an invite link, or to a self-hosted instance.")
                .arg(Arg::new("url").required(true).help(
                    "Your invite link, such as https://app.tokencontroller.com/i/kestrel-4f2a, or a self-hosted address.",
                )),
        )
        .subcommand(ClapCommand::new("status").about(
            "Organization, person, agents found, rules, and today's cost and share on tickets.",
        ))
        .subcommand(
            ClapCommand::new("review")
                .about("List draft token entries and their tags.")
                .arg(
                    Arg::new("all")
                        .long("all")
                        .action(ArgAction::SetTrue)
                        .help("Also list token entries already submitted."),
                ),
        )
        .subcommand(
            ClapCommand::new("tag")
                .about("Set tags on token entry N, such as ticket=KD-214 or purpose=learning.")
                .arg(number_arg())
                .arg(pairs_arg(
                    "tags",
                    "NAME=VALUE",
                    "One or more NAME=VALUE. An empty value, such as purpose=, removes the tag.",
                )),
        )
        .subcommand(
            ClapCommand::new("split")
                .about("Split token entry N between tickets, such as KD-231=60 KD-240=40.")
                .arg(number_arg())
                .arg(pairs_arg(
                    "shares",
                    "TICKET=SHARE",
                    "Two or more TICKET=SHARE. Shares are weights and need not add up to 100.",
                )),
        )
        .subcommand(
            ClapCommand::new("submit")
                .about("Submit your token sheet.")
                .arg(
                    Arg::new("tag")
                        .long("tag")
                        .value_name("NAME=VALUE")
                        .action(ArgAction::Append)
                        .help("A tag for every token entry this submit sends, such as ticket=KD-214. It wins over every rule. Repeat for more."),
                )
                .arg(
                    Arg::new("signal")
                        .long("signal")
                        .value_name("NAME=VALUE")
                        .action(ArgAction::Append)
                        .help("A signal the session files lack, as the GitHub Action passes them: repository, branch, pull_request or pull_request_title. Repeat for more."),
                ),
        )
        .subcommand(report_command())
        .subcommand(
            ClapCommand::new("export")
                .about("All token entries as CSV.")
                .arg(
                    Arg::new("csv")
                        .long("csv")
                        .required(true)
                        .action(ArgAction::SetTrue)
                        .help("Write CSV, the only format."),
                ),
        )
        .subcommand(rules_command())
        .subcommand(
            ClapCommand::new("config")
                .about("Settings, such as classifier.")
                .arg(Arg::new("key").help("The setting. Without one, every setting is listed."))
                .arg(Arg::new("value").help("The new value. An empty value removes the setting.")),
        )
        .subcommand(ClapCommand::new("record").about(
            "Add what the Claude Code mod or another tool knows about a session, as JSON on standard input.",
        ))
}

fn report_command() -> ClapCommand {
    ClapCommand::new("report")
        .about("Cost in a table, by any tag name or by day.")
        .arg(
            Arg::new("by")
                .long("by")
                .required(true)
                .value_name("TAG|day")
                .help("A tag name, such as team or ticket, or day."),
        )
        .arg(
            Arg::new("filter")
                .long("filter")
                .value_name("NAME=VALUE")
                .action(ArgAction::Append)
                .help("Only token entries with this tag, such as team=web. Repeat for more."),
        )
        .arg(
            Arg::new("since").long("since").value_name("DAY").help(
                "First day to include, such as 2026-10-01. Default: the first of this month.",
            ),
        )
}

fn rules_command() -> ClapCommand {
    ClapCommand::new("rules")
                .about("List, add, move and remove personal rules.")
                .subcommand(ClapCommand::new("list").about("List the rules: the organization's first, then yours."))
                .subcommand(
                    ClapCommand::new("add")
                        .about("Add a personal rule, such as: tc rules add branch starts-with review/ purpose=code-review")
                        .arg(pairs_arg(
                            "rule",
                            "RULE",
                            "SIGNAL TEST VALUE NAME=VALUE… The signal is branch, directory, repository, pull-request, subagent or agent-harness. The test is is, starts-with, ends-with, contains or matches (a regular expression); without one, a value with ^ $ * + ? ( ) [ ] { } | or \\ is a regular expression and any other must match exactly.",
                        )),
                )
                .subcommand(
                    ClapCommand::new("move")
                        .about("Move personal rule FROM to position TO; earlier rules win.")
                        .arg(Arg::new("from").required(true).value_parser(value_parser!(usize)))
                        .arg(Arg::new("to").required(true).value_parser(value_parser!(usize))),
                )
                .subcommand(
                    ClapCommand::new("remove")
                        .about("Remove personal rule N.")
                        .arg(Arg::new("number").required(true).value_parser(value_parser!(usize))),
                )
}

fn text(matches: &ArgMatches, name: &str) -> Option<String> {
    matches.get_one::<String>(name).cloned()
}

fn texts(matches: &ArgMatches, name: &str) -> Vec<String> {
    matches
        .get_many::<String>(name)
        .map(|values| values.cloned().collect())
        .unwrap_or_default()
}

fn required<T: Clone + Send + Sync + 'static>(matches: &ArgMatches, name: &str) -> Result<T> {
    matches
        .get_one::<T>(name)
        .cloned()
        .ok_or_else(|| Error::usage(format!("{name} is missing")))
}

/// Reads parsed arguments into [`Args`].
pub fn from_matches(matches: &ArgMatches) -> Result<Args> {
    let json = matches.get_flag("json");
    let Some((name, sub)) = matches.subcommand() else {
        return Err(Error::usage(
            "say which command to run; tc --help lists them",
        ));
    };
    let command = match name {
        "connect" => Command::Connect {
            url: required(sub, "url")?,
        },
        "status" => Command::Status,
        "review" => Command::Review {
            all: sub.get_flag("all"),
        },
        "tag" => Command::Tag {
            number: required(sub, "number")?,
            tags: texts(sub, "tags"),
        },
        "split" => Command::Split {
            number: required(sub, "number")?,
            shares: texts(sub, "shares"),
        },
        "submit" => Command::Submit {
            tags: texts(sub, "tag"),
            signals: texts(sub, "signal"),
        },
        "report" => Command::Report {
            by: required(sub, "by")?,
            filter: texts(sub, "filter"),
            since: text(sub, "since"),
        },
        "export" => Command::Export,
        "rules" => Command::Rules {
            action: match sub.subcommand() {
                Some(("add", add)) => Some(RulesAction::Add {
                    words: texts(add, "rule"),
                }),
                Some(("move", moving)) => Some(RulesAction::Move {
                    from: required(moving, "from")?,
                    to: required(moving, "to")?,
                }),
                Some(("remove", remove)) => Some(RulesAction::Remove {
                    number: required(remove, "number")?,
                }),
                Some(("list", _)) => Some(RulesAction::List),
                _ => None,
            },
        },
        "config" => Command::Config {
            key: text(sub, "key"),
            value: text(sub, "value"),
        },
        "record" => Command::Record,
        other => return Err(Error::usage(format!("there is no command {other}"))),
    };
    Ok(Args { json, command })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Result<Args> {
        let words: Vec<&str> = line.split(' ').collect();
        let matches = command()
            .try_get_matches_from(words)
            .map_err(|error| Error::usage(error.to_string()))?;
        from_matches(&matches)
    }

    #[test]
    fn the_command_line_is_well_formed() {
        command().debug_assert();
    }

    #[test]
    fn the_documented_examples_parse() {
        assert_eq!(
            parse("tc connect https://app.tokencontroller.com/i/kestrel-4f2a")
                .unwrap()
                .command,
            Command::Connect {
                url: "https://app.tokencontroller.com/i/kestrel-4f2a".into()
            }
        );
        assert_eq!(parse("tc status").unwrap().command, Command::Status);
        assert_eq!(
            parse("tc review").unwrap().command,
            Command::Review { all: false }
        );
        assert_eq!(
            parse("tc review --all").unwrap().command,
            Command::Review { all: true }
        );
        assert_eq!(
            parse("tc tag 5 ticket=KD-231").unwrap().command,
            Command::Tag {
                number: 5,
                tags: vec!["ticket=KD-231".into()]
            }
        );
        assert_eq!(
            parse("tc split 3 KD-231=60 KD-240=40").unwrap().command,
            Command::Split {
                number: 3,
                shares: vec!["KD-231=60".into(), "KD-240=40".into()]
            }
        );
        assert_eq!(
            parse("tc submit").unwrap().command,
            Command::Submit {
                tags: Vec::new(),
                signals: Vec::new()
            }
        );
        assert_eq!(
            parse("tc submit --tag project=storefront --tag purpose=code-review --signal pull_request=12")
                .unwrap()
                .command,
            Command::Submit {
                tags: vec!["project=storefront".into(), "purpose=code-review".into()],
                signals: vec!["pull_request=12".into()]
            }
        );
        assert_eq!(
            parse("tc report --by day --filter team=web --filter client=acme --since 2026-10-01")
                .unwrap()
                .command,
            Command::Report {
                by: "day".into(),
                filter: vec!["team=web".into(), "client=acme".into()],
                since: Some("2026-10-01".into())
            }
        );
        assert_eq!(parse("tc export --csv").unwrap().command, Command::Export);
    }

    #[test]
    fn the_documented_rules_and_settings_parse() {
        assert_eq!(
            parse("tc rules").unwrap().command,
            Command::Rules { action: None }
        );
        assert_eq!(
            parse("tc rules list").unwrap().command,
            Command::Rules {
                action: Some(RulesAction::List)
            }
        );
        assert_eq!(
            parse("tc rules add branch starts-with review/ purpose=code-review")
                .unwrap()
                .command,
            Command::Rules {
                action: Some(RulesAction::Add {
                    words: vec![
                        "branch".into(),
                        "starts-with".into(),
                        "review/".into(),
                        "purpose=code-review".into()
                    ]
                })
            }
        );
        assert_eq!(
            parse("tc rules move 2 1").unwrap().command,
            Command::Rules {
                action: Some(RulesAction::Move { from: 2, to: 1 })
            }
        );
        assert_eq!(
            parse("tc rules remove 1").unwrap().command,
            Command::Rules {
                action: Some(RulesAction::Remove { number: 1 })
            }
        );
        assert_eq!(
            parse("tc config classifier ~/bin/pick-ticket")
                .unwrap()
                .command,
            Command::Config {
                key: Some("classifier".into()),
                value: Some("~/bin/pick-ticket".into())
            }
        );
        assert_eq!(parse("tc record").unwrap().command, Command::Record);
        assert!(parse("tc --json review").unwrap().json);
        assert!(parse("tc review --json").unwrap().json);
        assert!(!parse("tc review").unwrap().json);
    }

    #[test]
    fn mistakes_are_refused() {
        assert!(parse("tc tag 5").is_err());
        assert!(parse("tc tag five ticket=KD-1").is_err());
        assert!(parse("tc export").is_err());
        assert!(parse("tc report").is_err());
        assert!(parse("tc fly").is_err());
    }
}
