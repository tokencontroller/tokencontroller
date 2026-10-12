//! One module per command. Each prints a table for people or, with
//! `--json`, exactly one JSON value.

pub mod config;
pub mod connect;
pub mod export;
pub mod record;
pub mod report;
pub mod review;
pub mod rules;
pub mod status;
pub mod submit;

use std::io::Write;

use crate::app::App;
use crate::cli::{Args, Command};
use crate::error::Result;
use crate::sync::Sources;

/// Runs a parsed command line, writing to `out`.
pub fn run(args: Args, out: &mut dyn Write) -> Result<()> {
    let mut app = App::open(args.json, out, Sources::on_this_machine())?;
    match args.command {
        Command::Connect { url } => connect::run(&mut app, &url, &mut std::io::stdin().lock()),
        Command::Status => status::run(&mut app),
        Command::Review { all } => review::run(&mut app, all),
        Command::Tag { number, tags } => review::tag(&mut app, number, &tags),
        Command::Split { number, shares } => review::split(&mut app, number, &shares),
        Command::Submit { tags, signals } => submit::run(&mut app, &tags, &signals),
        Command::Report { by, filter, since } => {
            report::run(&mut app, &by, &filter, since.as_deref())
        }
        Command::Export => export::run(&mut app),
        Command::Rules { action } => rules::run(&mut app, action),
        Command::Config { key, value } => config::run(&mut app, key.as_deref(), value.as_deref()),
        Command::Record => record::run(&mut app, &mut std::io::stdin().lock()),
    }
}

/// `name=value` → (`name`, `value`); the value may be empty.
pub fn name_value(text: &str) -> Option<(String, String)> {
    let (name, value) = text.split_once('=')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    Some((name.to_owned(), value.trim().to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_value_pairs_split_at_the_first_equals_sign() {
        assert_eq!(
            name_value("ticket=KD-1"),
            Some(("ticket".into(), "KD-1".into()))
        );
        assert_eq!(
            name_value("purpose="),
            Some(("purpose".into(), String::new()))
        );
        assert_eq!(name_value("a=b=c"), Some(("a".into(), "b=c".into())));
        assert_eq!(name_value("=x"), None);
        assert_eq!(name_value("plain"), None);
    }
}
