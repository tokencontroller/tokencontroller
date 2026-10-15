//! What every command starts from: the settings, the local database brought
//! up to date with the session files, and what is known about the
//! organization.

use std::io::{BufRead as _, IsTerminal as _, Write};

use tokencontroller_core::Rule;

use crate::api::Client;
use crate::classifier;
use crate::context::Context;
use crate::entries::{self, Entry, Placing, Run};
use crate::error::{Error, Result};
use crate::folders::{self, Scope};
use crate::home::{Config, Home};
use crate::keys::KeyPattern;
use crate::placing::Placer;
use crate::sources::Harness;
use crate::store::{Status, Store};
use crate::sync::{Reading, Sources, sync};

/// How long the organization's context is used before it is fetched again.
const CONTEXT_MAX_AGE_MS: i64 = 10 * 60 * 1000;

/// Draft token entries go back this far on the first run, unless `since`
/// says otherwise.
const FIRST_RUN_DAYS: i64 = 31;

pub struct App<'a> {
    pub home: Home,
    pub config: Config,
    pub store: Store,
    pub context: Context,
    pub run: Run,
    pub json: bool,
    pub out: &'a mut dyn Write,
    pub now_ms: i64,
    pub sources: Sources,
}

/// Where submissions go and with which token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    pub service: String,
    pub token: String,
}

impl Connection {
    /// A service token (`tc_svc_…`) reports automatic token entries; any
    /// other token submits the person's token sheet.
    pub fn is_service_token(&self) -> bool {
        self.token.starts_with("tc_svc_")
    }
}

impl<'a> App<'a> {
    pub fn open(json: bool, out: &'a mut dyn Write, sources: Sources) -> Result<Self> {
        let home = Home::locate()?;
        let config = home.read_config()?;
        let store = Store::open(&home.database())?;
        let now_ms = jiff::Timestamp::now().as_millisecond();
        let mut app = Self {
            home,
            config,
            store,
            context: Context::default(),
            run: Run::from_env(),
            json,
            out,
            now_ms,
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

    /// The context as last fetched, or the local one.
    fn cached_context(&self) -> Result<Context> {
        if self.connection().is_some()
            && let Some(text) = self.store.meta("context")?
            && let Ok(context) = serde_json::from_str::<Context>(&text)
        {
            return Ok(context);
        }
        self.local_context()
    }

    /// With no service: only the personal rules kept on this machine.
    fn local_context(&self) -> Result<Context> {
        let rules = self
            .store
            .personal_rules()?
            .iter()
            .filter_map(|text| serde_json::from_str::<Rule>(text).ok())
            .collect();
        Ok(Context {
            rules,
            ..Context::default()
        })
    }

    /// Fetches the context from the service when it is older than ten
    /// minutes, or always with `force`. Without a connection, or when the
    /// service cannot be reached and `force` is off, the cached one stays.
    /// The next command reads the context again: after a submit, the
    /// service's word on late token sheets has changed.
    pub fn context_is_stale(&self) -> Result<()> {
        self.store.set_meta("context_fetched_ms", "0")
    }

    pub fn refresh_context(&mut self, force: bool) -> Result<()> {
        let Some(client) = self.client() else {
            self.context = self.local_context()?;
            return Ok(());
        };
        let fetched: i64 = self
            .store
            .meta("context_fetched_ms")?
            .and_then(|text| text.parse().ok())
            .unwrap_or(0);
        if !force && self.now_ms.saturating_sub(fetched) < CONTEXT_MAX_AGE_MS {
            return Ok(());
        }
        match client.context() {
            Ok(context) => {
                self.store
                    .set_meta("context", &serde_json::to_string(&context)?)?;
                self.store
                    .set_meta("context_fetched_ms", &self.now_ms.to_string())?;
                for rejection in &context.rejected {
                    self.store
                        .mark_rejected(&rejection.local_id, &rejection.reason)?;
                }
                self.context = context;
                Ok(())
            }
            Err(error) if force => Err(error),
            // Offline is fine: the cached context places entries as before.
            Err(_) => Ok(()),
        }
    }

    pub fn patterns(&self) -> Vec<KeyPattern> {
        self.context
            .key_patterns(self.config.ticket_key_pattern.as_deref())
    }

    /// The text of every ticket key pattern, to notice when they change.
    fn pattern_text(&self) -> String {
        let mut patterns: Vec<String> = self
            .context
            .projects
            .iter()
            .filter_map(|project| project.ticket_key_pattern.clone())
            .collect();
        patterns.extend(self.config.ticket_key_pattern.clone());
        patterns.join("\n")
    }

    /// Sessions that started before this get no draft token entry.
    fn since_ms(&self) -> Result<i64> {
        if let Some(day) = &self.config.since {
            let date: jiff::civil::Date = day
                .parse()
                .map_err(|_| Error::usage(format!("since = {day} is not a day")))?;
            return Ok(start_of_day_ms(date));
        }
        if let Some(first) = self.store.meta("first_run_ms")?
            && let Ok(first) = first.parse::<i64>()
        {
            return Ok(first);
        }
        let first = self
            .now_ms
            .saturating_sub(FIRST_RUN_DAYS.saturating_mul(24 * 60 * 60 * 1000));
        self.store.set_meta("first_run_ms", &first.to_string())?;
        Ok(first)
    }

    /// Reads the session files, refreshes the context, and lets the
    /// classifier decide where it may. Every command that shows or sends
    /// entries calls this first. At a terminal, it first asks once which
    /// folder to track.
    pub fn prepare(&mut self) -> Result<()> {
        self.ask_about_folders()?;
        self.prepare_without_asking()
    }

    /// [`Self::prepare`] for input that is not a person's answer, such as
    /// `tc record`'s.
    pub fn prepare_without_asking(&mut self) -> Result<()> {
        self.read_sessions()?;
        self.classify()
    }

    /// Reads the session files and brings the drafts up to date with them.
    pub fn read_sessions(&mut self) -> Result<()> {
        self.refresh_context(false)?;
        let patterns = self.patterns();
        let pattern_text = self.pattern_text();
        let mut reading = Reading {
            patterns: &patterns,
            pattern_text: &pattern_text,
            since_ms: self.since_ms()?,
            scope: Scope::new(&self.config.folders),
        };
        sync(&mut self.store, &self.sources, &mut reading)?;
        crate::commands::record::apply_pending(&self.store)
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

    pub fn placer(&self) -> Placer {
        Placer::new(&self.context, self.config.ticket_key_pattern.as_deref())
    }

    /// Entries with the given statuses, built and placed.
    pub fn entries(&self, statuses: &[Status]) -> Result<Vec<Entry>> {
        let patterns = self.patterns();
        let placer = self.placer();
        let placing = Placing {
            placer: &placer,
            patterns: &patterns,
            run: &self.run,
        };
        entries::load(&self.store, statuses, &placing)
    }

    /// One entry by its number.
    pub fn entry(&self, number: i64) -> Result<Entry> {
        let row = self.store.entry(number)?.ok_or_else(|| {
            Error::usage(format!(
                "there is no token entry {number}; tc review lists them"
            ))
        })?;
        let patterns = self.patterns();
        let placer = self.placer();
        let placing = Placing {
            placer: &placer,
            patterns: &patterns,
            run: &self.run,
        };
        entries::build(&self.store, row, &placing)
    }

    /// Asks the classifier about drafts it may decide, when one is set and
    /// the question changed since it last answered.
    fn classify(&mut self) -> Result<()> {
        let Some(program) = self.config.classifier.clone() else {
            return Ok(());
        };
        let patterns = self.patterns();
        for entry in self.entries(&[Status::Draft, Status::Rejected])? {
            if !entry.asks_classifier
                || entry.row.named_tags.contains_key("ticket")
                || entry.row.hand_split.is_some()
            {
                continue;
            }
            let Some(mut digest) = classifier::digest(&entry, &patterns) else {
                continue;
            };
            // The question is remembered without the prompt sentences, which
            // are never stored.
            let question = serde_json::to_string(&digest)?;
            if entry.row.pick_question.as_deref() == Some(question.as_str()) {
                continue;
            }
            let prompts = self.prompts(entry.row.harness, &entry.row.session_id)?;
            classifier::add_mentions(&mut digest, &prompts, &patterns);
            match classifier::ask(&program, &digest) {
                Ok(pick) => self.store.set_pick(&entry.row.local_id, &pick, &question)?,
                Err(error) => {
                    // The classifier is optional: say so once and go on.
                    eprintln!("tc: the classifier failed and was skipped: {error}");
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    /// A session's prompts, read again from its files for the classifier.
    fn prompts(&self, harness: Harness, session_id: &str) -> Result<Vec<String>> {
        let mut prompts = Vec::new();
        for path in self.store.key_files(harness, session_id)? {
            // A file that went away since it was read holds nothing to add.
            let Ok(contents) = crate::sources::read(harness, std::path::Path::new(&path)) else {
                continue;
            };
            prompts.extend(
                contents
                    .prompts
                    .into_iter()
                    .filter(|prompt| prompt.session_id == session_id)
                    .map(|prompt| prompt.text),
            );
        }
        Ok(prompts)
    }

    /// Today's date in the local time zone.
    pub fn today(&self) -> jiff::civil::Date {
        jiff::Timestamp::from_millisecond(self.now_ms)
            .map(|now| now.to_zoned(jiff::tz::TimeZone::system()).date())
            .unwrap_or_default()
    }
}

/// The first millisecond of a day in the local time zone.
pub fn start_of_day_ms(date: jiff::civil::Date) -> i64 {
    date.to_zoned(jiff::tz::TimeZone::system())
        .map_or(0, |zoned| zoned.timestamp().as_millisecond())
}

/// The local day a time falls on.
pub fn day_of(at_ms: i64) -> jiff::civil::Date {
    jiff::Timestamp::from_millisecond(at_ms)
        .map(|timestamp| timestamp.to_zoned(jiff::tz::TimeZone::system()).date())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_tokens_report_automatic_entries() {
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
    fn days_start_at_local_midnight() {
        let day = jiff::civil::date(2026, 10, 11);
        assert_eq!(day_of(start_of_day_ms(day)), day);
        let before = start_of_day_ms(day).saturating_sub(1);
        assert_eq!(day_of(before), jiff::civil::date(2026, 10, 10));
    }
}
