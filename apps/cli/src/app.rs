//! What every command starts from: the settings, the local database brought
//! up to date with the session files, what is known about the organization,
//! and the log folded as of now.

use std::collections::BTreeMap;
use std::io::{BufRead as _, IsTerminal as _, Write};
use std::path::Path;

use time::{Date, OffsetDateTime};
use tokencontroller_core::{Event, Record, State, Then};

use crate::api::Client;
use crate::context::Context;
use crate::error::{Error, Result};
use crate::folders::{self, Scope};
use crate::home::{Config, Home};
use crate::keys::KeyPattern;
use crate::log::{self, Recorded, Run, SessionView};
use crate::store::Store;
use crate::sync::{Reading, Sources, sync};

/// How long the organization's context is used before it is fetched again.
const CONTEXT_MAX_AGE_MS: i64 = 10 * 60 * 1000;

/// Sessions go back this far on the first run, unless `since` says
/// otherwise.
const FIRST_RUN_DAYS: i64 = 31;

/// What `tc record` was told is kept in `meta` under this prefix.
pub const RECORD: &str = "record:";

pub struct App<'a> {
    pub home: Home,
    pub config: Config,
    pub store: Store,
    pub context: Context,
    pub run: Run,
    pub json: bool,
    pub out: &'a mut dyn Write,
    pub now: OffsetDateTime,
    pub sources: Sources,
}

/// Where submissions go and with which token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    pub service: String,
    pub token: String,
}

impl Connection {
    /// A service token (`tc_svc_…`) reports messages as its creator and
    /// never submits; any other token is a person's.
    pub fn is_service_token(&self) -> bool {
        self.token.starts_with("tc_svc_")
    }
}

impl<'a> App<'a> {
    pub fn open(json: bool, out: &'a mut dyn Write, sources: Sources) -> Result<Self> {
        let home = Home::locate()?;
        let config = home.read_config()?;
        let store = Store::open(&home.database())?;
        let mut app = Self {
            home,
            config,
            store,
            context: Context::default(),
            run: Run::from_env(),
            json,
            out,
            now: OffsetDateTime::now_utc(),
            sources,
        };
        app.context = app.cached_context()?;
        Ok(app)
    }

    /// The service and token: `TC_URL` and `TC_TOKEN` win over `tc connect`.
    pub fn connection(&self) -> Option<Connection> {
        let from_env = |name: &str| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
        };
        let service = from_env("TC_URL").or_else(|| self.config.service.clone())?;
        let token = from_env("TC_TOKEN").or_else(|| self.config.token.clone())?;
        Some(Connection { service, token })
    }

    pub fn client(&self) -> Option<Client> {
        self.connection()
            .map(|connection| Client::new(&connection.service, Some(connection.token)))
    }

    /// The context as last fetched, or none.
    fn cached_context(&self) -> Result<Context> {
        if self.connection().is_some()
            && let Some(text) = self.store.meta("context")?
            && let Ok(context) = serde_json::from_str::<Context>(&text)
        {
            return Ok(context);
        }
        Ok(Context::default())
    }

    /// The next command reads the context again: after a submit, the
    /// service's list of submissions has changed.
    pub fn context_is_stale(&self) -> Result<()> {
        self.store.set_meta("context_fetched_ms", "0")
    }

    /// Fetches the context from the service when it is older than ten
    /// minutes, or always with `force`. Without a connection, or when the
    /// service cannot be reached and `force` is off, the cached one stays.
    pub fn refresh_context(&mut self, force: bool) -> Result<()> {
        let Some(client) = self.client() else {
            self.context = Context::default();
            return Ok(());
        };
        let fetched: i64 = self
            .store
            .meta("context_fetched_ms")?
            .and_then(|text| text.parse().ok())
            .unwrap_or(0);
        let now_ms = crate::output::millis(self.now);
        if !force && now_ms.saturating_sub(fetched) < CONTEXT_MAX_AGE_MS {
            return Ok(());
        }
        match client.context() {
            Ok(context) => {
                self.store
                    .set_meta("context", &serde_json::to_string(&context)?)?;
                self.store
                    .set_meta("context_fetched_ms", &now_ms.to_string())?;
                self.context = context;
                Ok(())
            }
            Err(error) if force => Err(error),
            // Offline is fine: the cached context folds the log as before.
            Err(_) => Ok(()),
        }
    }

    /// The pattern feature keys are found with: connected, the pattern of
    /// the organization's rules that find a feature; else the one from
    /// `tc config feature-key-pattern`.
    pub fn pattern(&self) -> Option<KeyPattern> {
        if self.connection().is_some() {
            let from_rules = self
                .context
                .rules
                .iter()
                .chain(&self.context.my_rules)
                .find(|rule| matches!(&rule.then, Then::KeyOf { .. }))
                .and_then(|rule| KeyPattern::new(rule.pattern.as_str()));
            if from_rules.is_some() {
                return from_rules;
            }
        }
        self.config
            .feature_key_pattern
            .as_deref()
            .and_then(KeyPattern::new)
    }

    /// Sessions that started before this get no number.
    fn since_ms(&self) -> Result<i64> {
        if let Some(day) = &self.config.since {
            let date = crate::output::parse_date(day)
                .ok_or_else(|| Error::usage(format!("since = {day} is not a day")))?;
            return Ok(crate::output::millis(date.midnight().assume_utc()));
        }
        if let Some(first) = self.store.meta("first_run_ms")?
            && let Ok(first) = first.parse::<i64>()
        {
            return Ok(first);
        }
        let first = crate::output::millis(self.now)
            .saturating_sub(FIRST_RUN_DAYS.saturating_mul(24 * 60 * 60 * 1000));
        self.store.set_meta("first_run_ms", &first.to_string())?;
        Ok(first)
    }

    /// Reads the session files and refreshes the context. Every command
    /// that shows or sends sessions calls this first. At a terminal, it
    /// first asks once which folder to track.
    pub fn prepare(&mut self) -> Result<()> {
        self.ask_about_folders()?;
        self.read_sessions()
    }

    /// Reads the session files and numbers the sessions that count.
    pub fn read_sessions(&mut self) -> Result<()> {
        self.refresh_context(false)?;
        let pattern = self.pattern();
        let submitted = self.submitted_windows()?;
        let mut reading = Reading {
            pattern: pattern.as_ref(),
            since_ms: self.since_ms()?,
            scope: Scope::new(&self.config.folders),
            submitted,
        };
        sync(&mut self.store, &self.sources, &mut reading)
    }

    /// The windows already submitted, from the service and this machine,
    /// in milliseconds.
    fn submitted_windows(&self) -> Result<Vec<(i64, i64)>> {
        let mut windows: Vec<(i64, i64)> = self
            .context
            .submission_events()?
            .into_iter()
            .chain(self.store.events()?)
            .filter_map(|event| match event.record {
                Record::Submit(submit) => Some(submit.window),
                _ => None,
            })
            .map(|window| {
                let from = window.from.midnight().assume_utc();
                let to = window
                    .to
                    .midnight()
                    .assume_utc()
                    .saturating_add(time::Duration::days(1))
                    .saturating_sub(time::Duration::milliseconds(1));
                (crate::output::millis(from), crate::output::millis(to))
            })
            .collect();
        windows.sort_unstable();
        Ok(windows)
    }

    /// While no folder was chosen, a person at a terminal is asked whether
    /// to track only the folder they are in. Scripts, CI and the Claude Code
    /// mod are never asked, and every folder counts for them.
    fn ask_about_folders(&mut self) -> Result<()> {
        let interactive =
            !self.json && std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
        if !folders::should_ask(&self.config, interactive) {
            return Ok(());
        }
        let here = folders::current()?;
        eprintln!("tc reads the Claude Code and Codex sessions of every folder on this machine.");
        eprint!("Track sessions in {}? [Y/n] ", here.display());
        let mut line = String::new();
        let read = std::io::stdin()
            .lock()
            .read_line(&mut line)
            .map_err(|source| Error::file("standard input", source))?;
        match folders::answer(&line).filter(|_| read > 0) {
            Some(true) => {
                folders::track(&mut self.config, &here);
                self.home.write_config(&self.config)?;
                eprintln!(
                    "tc: tracking {}. tc track DIR adds a folder; tc list shows where sessions ran.",
                    here.display()
                );
            }
            Some(false) => {
                self.config.all_folders = true;
                self.home.write_config(&self.config)?;
                eprintln!("tc: counting sessions in every folder. tc track DIR limits it.");
            }
            None => eprintln!("tc: nothing changed; tc asks again next time."),
        }
        Ok(())
    }

    /// What `tc record` was told, by local id.
    pub fn records(&self) -> Result<BTreeMap<String, Recorded>> {
        let mut records = BTreeMap::new();
        for (key, text) in self.store.meta_with_prefix(RECORD)? {
            let Some(local) = key.strip_prefix(RECORD) else {
                continue;
            };
            records.insert(
                local.to_owned(),
                serde_json::from_str(&text).unwrap_or_default(),
            );
        }
        Ok(records)
    }

    /// The log on this machine, folded as of now.
    pub fn fold(&self) -> Result<State> {
        let messages = self.store.counted_messages()?;
        let keys = self.store.keys()?;
        let titles = self.store.titles()?;
        let records = self.records()?;
        let pattern = self.pattern();
        let inputs = log::Inputs {
            messages: &messages,
            keys: &keys,
            titles: &titles,
            records: &records,
            run: &self.run,
            context: &self.context,
            connected: self.connection().is_some(),
            pattern: pattern.as_ref(),
            price_file: self.config.price_table.as_deref().map(Path::new),
            local_events: self.store.events()?,
            now: self.now,
        };
        log::fold(&inputs)
    }

    /// Every session that counts, as the review list shows them.
    pub fn sessions(&self) -> Result<Vec<SessionView>> {
        let state = self.fold()?;
        log::sessions(&state, &self.store.sessions()?)
    }

    /// One session by its number.
    pub fn session(&self, number: i64) -> Result<SessionView> {
        self.sessions()?
            .into_iter()
            .find(|session| session.number == number)
            .ok_or_else(|| {
                Error::usage(format!(
                    "there is no session {number}; tc review lists them"
                ))
            })
    }

    /// Records an event on this machine with the next local id.
    pub fn record(&self, record: Record, subject: Option<&str>) -> Result<Event> {
        let id = self.store.next_event_id()?;
        let person = tokencontroller_core::TagValue::new(crate::context::PERSON)?;
        let event = Event::new(&id, self.now, record)?.by(person);
        self.store.append_event(&event, subject)?;
        Ok(event)
    }

    /// Today's date, in UTC.
    pub fn today(&self) -> Date {
        self.now.date()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_tokens_report_as_their_creator() {
        let service = Connection {
            service: "https://x".into(),
            token: "tc_svc_1".into(),
        };
        assert!(service.is_service_token());
        let person = Connection {
            service: "https://x".into(),
            token: "tc_ses_1".into(),
        };
        assert!(!person.is_service_token());
    }

    #[test]
    fn service_tokens_are_told_from_a_person_s() {
        let service = Connection {
            service: "https://tc.example.com".into(),
            token: "tc_svc_ci".into(),
        };
        assert!(service.is_service_token());
        let person = Connection {
            service: "https://tc.example.com".into(),
            token: "tc_cli_me".into(),
        };
        assert!(!person.is_service_token());
    }
}
