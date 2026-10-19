//! The local `SQLite` file: model calls read from session files, the
//! numbers `tc review` gives the sessions that count, and the event log of
//! this machine: splits, rules, submits and reversals. No prompt text is ever
//! written here; only the feature keys found in it.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use tokencontroller_core::Event;

use crate::error::{Error, Result};
use crate::folders::Scope;
use crate::sources::{FileContents, Harness};
use crate::tokens::Tokens;

/// The schema. Bump [`SCHEMA_VERSION`] and add a step to [`migrate`] to
/// change it; never edit a released step.
const SCHEMA_V1: &str = "
CREATE TABLE files (
    path TEXT PRIMARY KEY,
    harness TEXT NOT NULL,
    size INTEGER NOT NULL,
    modified_ms INTEGER NOT NULL
);
CREATE TABLE messages (
    harness TEXT NOT NULL,
    message_key TEXT NOT NULL,
    session_id TEXT NOT NULL,
    message_id TEXT NOT NULL,
    request_id TEXT,
    at_ms INTEGER NOT NULL,
    model TEXT NOT NULL,
    input INTEGER NOT NULL,
    output INTEGER NOT NULL,
    cache_write_5m INTEGER NOT NULL,
    cache_write_1h INTEGER NOT NULL,
    cache_read INTEGER NOT NULL,
    reported_cost_nanos INTEGER,
    branch TEXT,
    directory TEXT,
    repository TEXT,
    agent_id TEXT,
    sidechain INTEGER NOT NULL,
    PRIMARY KEY (harness, message_key)
);
CREATE INDEX messages_by_session ON messages (harness, session_id, at_ms);
CREATE TABLE session_keys (
    path TEXT NOT NULL,
    harness TEXT NOT NULL,
    session_id TEXT NOT NULL,
    key TEXT NOT NULL,
    count INTEGER NOT NULL,
    first_at_ms INTEGER NOT NULL,
    last_at_ms INTEGER NOT NULL,
    PRIMARY KEY (path, harness, session_id, key)
);
CREATE TABLE session_titles (
    harness TEXT NOT NULL,
    session_id TEXT NOT NULL,
    title TEXT NOT NULL,
    PRIMARY KEY (harness, session_id)
);
CREATE TABLE subagents (
    harness TEXT NOT NULL,
    session_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    subagent_type TEXT NOT NULL,
    PRIMARY KEY (harness, session_id, agent_id)
);
CREATE TABLE entries (
    local_id TEXT PRIMARY KEY,
    number INTEGER NOT NULL UNIQUE,
    harness TEXT NOT NULL,
    session_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'draft',
    summary TEXT,
    named_tags TEXT NOT NULL DEFAULT '{}',
    hand_split TEXT,
    signals TEXT NOT NULL DEFAULT '{}',
    rejected_reason TEXT,
    remote_id TEXT,
    submitted_at_ms INTEGER,
    submitted_fingerprint TEXT,
    pick TEXT,
    pick_question TEXT
);
CREATE TABLE personal_rules (
    position INTEGER PRIMARY KEY,
    rule TEXT NOT NULL
);
CREATE TABLE meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
";

/// Cost model 2: token entries, hand tags and picks give way to numbered
/// sessions and an event log. What version 1 held by hand is not carried
/// over; the session files are read again.
const SCHEMA_V2: &str = "
DROP TABLE IF EXISTS entries;
DROP TABLE IF EXISTS personal_rules;
DELETE FROM meta;
CREATE TABLE sessions (
    local_id TEXT PRIMARY KEY,
    number INTEGER NOT NULL UNIQUE,
    harness TEXT NOT NULL,
    session_id TEXT NOT NULL
);
CREATE TABLE events (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    subject TEXT,
    event TEXT NOT NULL
);
";

const SCHEMA_VERSION: i64 = 2;

/// Sessions `tc record` was told about are kept in `meta` under this prefix.
const RECORDED: &str = "recorded:";

/// The folder a session ran in: that of its first model call that names
/// one. Used inside a query over `messages m` grouped by session.
const SESSION_FOLDER: &str = "(SELECT d.directory FROM messages d
      WHERE d.harness = m.harness AND d.session_id = m.session_id AND d.directory IS NOT NULL
      ORDER BY d.at_ms, d.message_key LIMIT 1)";

/// A session read from the files, as `tc list` groups them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFolder {
    pub harness: Harness,
    pub folder: Option<String>,
    pub last_ms: i64,
}

/// One stored model call, with the subagent type resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMessage {
    pub harness: Harness,
    pub session_id: String,
    pub message_id: String,
    pub request_id: Option<String>,
    pub at_ms: i64,
    pub model: String,
    pub tokens: Tokens,
    pub reported_cost: Option<i64>,
    pub branch: Option<String>,
    pub directory: Option<String>,
    pub repository: Option<String>,
    pub subagent: Option<String>,
}

/// A session that counts, with the number `tc review` shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRow {
    pub local_id: String,
    pub number: i64,
    pub harness: Harness,
    pub session_id: String,
}

pub struct Store {
    connection: Connection,
}

fn file_unchanged(
    connection: &Connection,
    path: &str,
    size: u64,
    modified_ms: i64,
) -> Result<bool> {
    let seen: Option<(i64, i64)> = connection
        .query_row(
            "SELECT size, modified_ms FROM files WHERE path = ?1",
            [path],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(seen == Some((to_i64(size), modified_ms)))
}

fn to_i64(count: u64) -> i64 {
    i64::try_from(count).unwrap_or(i64::MAX)
}

fn to_u64(count: i64) -> u64 {
    u64::try_from(count).unwrap_or(0)
}

/// The local ID of a session, such as `claude-code:6b1f…`.
pub fn local_id(harness: Harness, session_id: &str) -> String {
    format!("{}:{session_id}", harness.name())
}

impl Store {
    /// Opens the database, creating it and its folder if needed.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).map_err(|source| Error::file(folder, source))?;
        }
        let connection = Connection::open(path)?;
        Self::with(connection)
    }

    /// A database in memory, for tests.
    #[cfg(test)]
    pub fn in_memory() -> Result<Self> {
        Self::with(Connection::open_in_memory()?)
    }

    fn with(connection: Connection) -> Result<Self> {
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        let mut store = Self { connection };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&mut self) -> Result<()> {
        let version: i64 = self
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(Error::usage(
                "this database was written by a newer tc; update tc first",
            ));
        }
        let transaction = self.connection.transaction()?;
        if version < 1 {
            transaction.execute_batch(SCHEMA_V1)?;
        }
        if version < 2 {
            transaction.execute_batch(SCHEMA_V2)?;
            // Every file is read again under the new model.
            transaction.execute_batch("DELETE FROM files; DELETE FROM session_keys;")?;
        }
        transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .connection
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.connection.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Every setting whose key starts with `prefix`.
    pub fn meta_with_prefix(&self, prefix: &str) -> Result<Vec<(String, String)>> {
        let mut statement = self.connection.prepare(
            "SELECT key, value FROM meta WHERE substr(key, 1, length(?1)) = ?1 ORDER BY key",
        )?;
        let rows = statement
            .query_map([prefix], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Whether a file was read before at this size and time.
    #[cfg(test)]
    pub fn file_unchanged(&self, path: &str, size: u64, modified_ms: i64) -> Result<bool> {
        file_unchanged(&self.connection, path, size, modified_ms)
    }

    /// Makes every file be read again, such as after the feature key
    /// pattern changed.
    pub fn forget_files(&self) -> Result<()> {
        self.connection
            .execute_batch("DELETE FROM files; DELETE FROM session_keys;")?;
        Ok(())
    }

    /// Starts a batch of writes that land together.
    pub fn batch(&mut self) -> Result<Batch<'_>> {
        Ok(Batch {
            transaction: self.connection.transaction()?,
        })
    }

    /// The next id for an event recorded on this machine, such as
    /// `local-12`.
    pub fn next_event_id(&self) -> Result<String> {
        let last: u64 = self
            .meta("event_seq")?
            .and_then(|text| text.parse().ok())
            .unwrap_or(0);
        let next = last.saturating_add(1);
        self.set_meta("event_seq", &next.to_string())?;
        Ok(format!("local-{next}"))
    }

    /// Appends an event to the log on this machine. `subject` is the local
    /// id of the session the event is about, for splits, so the event goes
    /// when the session stops counting.
    pub fn append_event(&self, event: &Event, subject: Option<&str>) -> Result<()> {
        self.connection.execute(
            "INSERT INTO events (id, subject, event) VALUES (?1, ?2, ?3)",
            params![event.id.as_str(), subject, serde_json::to_string(event)?],
        )?;
        Ok(())
    }

    /// Every event recorded on this machine, in the order recorded.
    pub fn events(&self) -> Result<Vec<Event>> {
        let mut statement = self
            .connection
            .prepare("SELECT event FROM events ORDER BY seq")?;
        let texts = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut events = Vec::with_capacity(texts.len());
        for text in texts {
            events.push(serde_json::from_str(&text)?);
        }
        Ok(events)
    }

    /// Numbers every session that has no number yet, started at or after
    /// `since_ms` and counting by `scope` or recorded, in the order they
    /// started. A session with no submitted message that stops counting
    /// loses its number and the splits set on it by hand; a session inside
    /// one of the `submitted` windows (milliseconds, closed) keeps both.
    pub fn create_sessions(
        &mut self,
        since_ms: i64,
        scope: &mut Scope,
        submitted: &[(i64, i64)],
    ) -> Result<()> {
        let recorded: HashSet<String> = self
            .meta_with_prefix(RECORDED)?
            .into_iter()
            .filter_map(|(key, _)| key.strip_prefix(RECORDED).map(str::to_owned))
            .collect();
        let transaction = self.connection.transaction()?;
        let sessions: Vec<(String, String, i64, i64, Option<String>)> = {
            let mut statement = transaction.prepare(&format!(
                "SELECT m.harness, m.session_id, MIN(m.at_ms), MAX(m.at_ms), {SESSION_FOLDER}
                 FROM messages m
                 GROUP BY m.harness, m.session_id
                 ORDER BY MIN(m.at_ms), m.harness, m.session_id"
            ))?;
            statement
                .query_map([], |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        let mut next: i64 =
            transaction.query_row("SELECT COALESCE(MAX(number), 0) FROM sessions", [], |row| {
                row.get(0)
            })?;
        for (harness, session_id, started_ms, ended_ms, folder) in sessions {
            let local = format!("{harness}:{session_id}");
            let counts = recorded.contains(&local) || scope.includes(folder.as_deref());
            let kept = submitted
                .iter()
                .any(|(from, to)| started_ms <= *to && ended_ms >= *from);
            if !counts && !kept {
                transaction.execute("DELETE FROM sessions WHERE local_id = ?1", [&local])?;
                transaction.execute("DELETE FROM events WHERE subject = ?1", [&local])?;
                continue;
            }
            let exists: Option<i64> = transaction
                .query_row(
                    "SELECT number FROM sessions WHERE local_id = ?1",
                    [&local],
                    |row| row.get(0),
                )
                .optional()?;
            if exists.is_none() && (started_ms >= since_ms || kept) {
                next = next
                    .checked_add(1)
                    .ok_or_else(|| Error::Data("too many sessions".into()))?;
                transaction.execute(
                    "INSERT INTO sessions (local_id, number, harness, session_id) VALUES (?1, ?2, ?3, ?4)",
                    params![local, next, harness, session_id],
                )?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// Remembers that `tc record` was told about a session, so it counts
    /// whatever folder it ran in.
    pub fn mark_recorded(&self, harness: Harness, session_id: &str) -> Result<()> {
        self.set_meta(&format!("{RECORDED}{}", local_id(harness, session_id)), "1")
    }

    /// Every session read: its agent harness, the folder it ran in and when
    /// it was last active.
    pub fn session_folders(&self) -> Result<Vec<SessionFolder>> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT m.harness, {SESSION_FOLDER}, MAX(m.at_ms)
             FROM messages m
             GROUP BY m.harness, m.session_id"
        ))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|(harness, folder, last_ms)| {
                Some(SessionFolder {
                    harness: Harness::from_name(&harness)?,
                    folder,
                    last_ms,
                })
            })
            .collect())
    }

    /// The sessions that count, by number.
    pub fn sessions(&self) -> Result<Vec<SessionRow>> {
        let mut statement = self.connection.prepare(
            "SELECT local_id, number, harness, session_id FROM sessions ORDER BY number",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|(local_id, number, harness, session_id)| {
                Some(SessionRow {
                    local_id,
                    number,
                    harness: Harness::from_name(&harness)?,
                    session_id,
                })
            })
            .collect())
    }

    /// The model calls of every session that counts, by session number and
    /// time, with subagent types.
    pub fn counted_messages(&self) -> Result<Vec<StoredMessage>> {
        let mut statement = self.connection.prepare(
            "SELECT m.harness, m.session_id, m.message_id, m.request_id, m.at_ms, m.model,
                    m.input, m.output, m.cache_write_5m, m.cache_write_1h, m.cache_read,
                    m.reported_cost_nanos, m.branch, m.directory, m.repository, s.subagent_type
             FROM messages m
             JOIN sessions n ON n.harness = m.harness AND n.session_id = m.session_id
             LEFT JOIN subagents s
               ON s.harness = m.harness AND s.session_id = m.session_id AND s.agent_id = m.agent_id
             ORDER BY n.number, m.at_ms, m.message_key",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    StoredMessage {
                        harness: Harness::ClaudeCode,
                        session_id: row.get(1)?,
                        message_id: row.get(2)?,
                        request_id: row.get(3)?,
                        at_ms: row.get(4)?,
                        model: row.get(5)?,
                        tokens: Tokens {
                            input: to_u64(row.get(6)?),
                            output: to_u64(row.get(7)?),
                            cache_write_5m: to_u64(row.get(8)?),
                            cache_write_1h: to_u64(row.get(9)?),
                            cache_read: to_u64(row.get(10)?),
                        },
                        reported_cost: row.get(11)?,
                        branch: row.get(12)?,
                        directory: row.get(13)?,
                        repository: row.get(14)?,
                        subagent: row.get(15)?,
                    },
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|(harness, message)| {
                Some(StoredMessage {
                    harness: Harness::from_name(&harness)?,
                    ..message
                })
            })
            .collect())
    }

    /// Feature keys found in prompts and titles, per session, in the order
    /// they were first named, each once.
    pub fn keys(&self) -> Result<BTreeMap<String, Vec<String>>> {
        let mut statement = self.connection.prepare(
            "SELECT harness, session_id, key
             FROM session_keys
             GROUP BY harness, session_id, key
             ORDER BY harness, session_id, MIN(first_at_ms), key",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut keys: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (harness, session_id, key) in rows {
            keys.entry(format!("{harness}:{session_id}"))
                .or_default()
                .push(key);
        }
        Ok(keys)
    }

    /// Session titles the agents wrote, by local id.
    pub fn titles(&self) -> Result<BTreeMap<String, String>> {
        let mut statement = self
            .connection
            .prepare("SELECT harness, session_id, title FROM session_titles")?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .map(|(harness, session_id, title)| (format!("{harness}:{session_id}"), title))
            .collect())
    }

    /// How many sessions of each agent harness the database holds.
    pub fn session_counts(&self) -> Result<Vec<(String, u64)>> {
        let mut statement = self.connection.prepare(
            "SELECT harness, COUNT(DISTINCT session_id) FROM messages GROUP BY harness ORDER BY harness",
        )?;
        let counts = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, to_u64(row.get(1)?)))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(counts)
    }
}

/// Writes that land together: everything read in one pass over the files.
pub struct Batch<'a> {
    transaction: Transaction<'a>,
}

impl Batch<'_> {
    /// Whether a file was read before at this size and time.
    pub fn file_unchanged(&self, path: &str, size: u64, modified_ms: i64) -> Result<bool> {
        file_unchanged(&self.transaction, path, size, modified_ms)
    }

    /// Stores what was read from one file. `keys` are the feature keys
    /// found in its prompts and titles, per session with the time first
    /// named; the file's earlier keys are replaced.
    pub fn add_file(
        &self,
        path: &str,
        harness: Harness,
        size: u64,
        modified_ms: i64,
        contents: &FileContents,
        keys: &[(String, String, i64)],
    ) -> Result<()> {
        let t = &self.transaction;
        let harness_name = harness.name();
        for message in &contents.messages {
            // A main copy replaces a side conversation's copy; otherwise the
            // first copy stays.
            t.execute(
                "INSERT INTO messages (harness, message_key, session_id, message_id, request_id, at_ms,
                    model, input, output, cache_write_5m, cache_write_1h, cache_read,
                    reported_cost_nanos, branch, directory, repository, agent_id, sidechain)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
                 ON CONFLICT (harness, message_key) DO UPDATE SET
                    session_id = excluded.session_id, message_id = excluded.message_id,
                    request_id = excluded.request_id, at_ms = excluded.at_ms, model = excluded.model,
                    input = excluded.input, output = excluded.output,
                    cache_write_5m = excluded.cache_write_5m, cache_write_1h = excluded.cache_write_1h,
                    cache_read = excluded.cache_read, reported_cost_nanos = excluded.reported_cost_nanos,
                    branch = excluded.branch, directory = excluded.directory,
                    repository = excluded.repository, agent_id = excluded.agent_id, sidechain = 0
                 WHERE messages.sidechain = 1 AND excluded.sidechain = 0",
                params![
                    harness_name,
                    message.key,
                    message.session_id,
                    message.id,
                    message.request_id,
                    message.at_ms,
                    message.model,
                    to_i64(message.tokens.input),
                    to_i64(message.tokens.output),
                    to_i64(message.tokens.cache_write_5m),
                    to_i64(message.tokens.cache_write_1h),
                    to_i64(message.tokens.cache_read),
                    message.reported_cost,
                    message.branch,
                    message.directory,
                    message.repository,
                    message.agent_id,
                    message.sidechain,
                ],
            )?;
        }
        for (session_id, title) in &contents.titles {
            t.execute(
                "INSERT INTO session_titles (harness, session_id, title) VALUES (?1, ?2, ?3)
                 ON CONFLICT (harness, session_id) DO UPDATE SET title = excluded.title",
                params![harness_name, session_id, title],
            )?;
        }
        for (session_id, agent_id, kind) in &contents.subagents {
            t.execute(
                "INSERT INTO subagents (harness, session_id, agent_id, subagent_type) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (harness, session_id, agent_id) DO UPDATE SET subagent_type = excluded.subagent_type",
                params![harness_name, session_id, agent_id, kind],
            )?;
        }
        t.execute("DELETE FROM session_keys WHERE path = ?1", [path])?;
        for (session_id, key, first_at_ms) in keys {
            t.execute(
                "INSERT INTO session_keys (path, harness, session_id, key, count, first_at_ms, last_at_ms)
                 VALUES (?1, ?2, ?3, ?4, 1, ?5, ?5)
                 ON CONFLICT (path, harness, session_id, key) DO UPDATE SET
                    count = session_keys.count + 1,
                    first_at_ms = MIN(session_keys.first_at_ms, excluded.first_at_ms),
                    last_at_ms = MAX(session_keys.last_at_ms, excluded.last_at_ms)",
                params![path, harness_name, session_id, key, first_at_ms],
            )?;
        }
        t.execute(
            "INSERT INTO files (path, harness, size, modified_ms) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (path) DO UPDATE SET size = excluded.size, modified_ms = excluded.modified_ms",
            params![path, harness_name, to_i64(size), modified_ms],
        )?;
        Ok(())
    }

    pub fn commit(self) -> Result<()> {
        self.transaction.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::Message;
    use time::macros::datetime;
    use tokencontroller_core::{Record, Split, SplitTarget, Tags};

    fn message(key: &str, session: &str, at_ms: i64, sidechain: bool, input: u64) -> Message {
        Message {
            session_id: session.into(),
            key: key.into(),
            id: key.into(),
            request_id: Some("req".into()),
            at_ms,
            model: "claude-sonnet-4-5".into(),
            tokens: Tokens {
                input,
                ..Tokens::default()
            },
            reported_cost: None,
            branch: Some("KD-1".into()),
            directory: Some("/w".into()),
            repository: None,
            agent_id: None,
            sidechain,
        }
    }

    fn add(store: &mut Store, path: &str, contents: &FileContents, keys: &[(String, String, i64)]) {
        let batch = store.batch().unwrap();
        batch
            .add_file(path, Harness::ClaudeCode, 10, 20, contents, keys)
            .unwrap();
        batch.commit().unwrap();
    }

    fn split_event(id: &str, session: &str) -> Event {
        let split = Split::whole(
            SplitTarget::Session(tokencontroller_core::SessionId::new(session).unwrap()),
            Tags::parse(["feature=KD-9"]).unwrap(),
        );
        Event::new(id, datetime!(2026-10-11 09:00 UTC), Record::Split(split)).unwrap()
    }

    #[test]
    fn a_response_counts_once_and_the_main_copy_wins() {
        let mut store = Store::in_memory().unwrap();
        let contents = FileContents {
            messages: vec![
                message("msg_1", "s1", 1, true, 5),
                message("msg_1", "s1", 1, false, 7),
                message("msg_1", "s1", 1, true, 9),
                message("msg_2", "s1", 2, false, 1),
            ],
            ..FileContents::default()
        };
        add(&mut store, "/f", &contents, &[]);
        store.create_sessions(0, &mut Scope::new(&[]), &[]).unwrap();
        let messages = store.counted_messages().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages.first().unwrap().tokens.input, 7);
        assert_eq!(messages.first().unwrap().harness, Harness::ClaudeCode);
        // Reading the same file again changes nothing.
        add(&mut store, "/f", &contents, &[]);
        assert_eq!(store.counted_messages().unwrap().len(), 2);
        assert!(store.file_unchanged("/f", 10, 20).unwrap());
        assert!(!store.file_unchanged("/f", 11, 20).unwrap());
        store.forget_files().unwrap();
        assert!(!store.file_unchanged("/f", 10, 20).unwrap());
    }

    #[test]
    fn sessions_are_numbered_in_order_and_only_from_since() {
        let mut store = Store::in_memory().unwrap();
        let contents = FileContents {
            messages: vec![
                message("a", "late", 300, false, 1),
                message("b", "early", 100, false, 1),
                message("c", "old", 10, false, 1),
            ],
            ..FileContents::default()
        };
        add(&mut store, "/f", &contents, &[]);
        store
            .create_sessions(50, &mut Scope::new(&[]), &[])
            .unwrap();
        let sessions: Vec<(i64, String)> = store
            .sessions()
            .unwrap()
            .into_iter()
            .map(|s| (s.number, s.session_id))
            .collect();
        assert_eq!(sessions, [(1, "early".to_owned()), (2, "late".to_owned())]);
        store
            .create_sessions(50, &mut Scope::new(&[]), &[])
            .unwrap();
        assert_eq!(store.sessions().unwrap().len(), 2);
        // An old session inside a submitted window still counts.
        store
            .create_sessions(50, &mut Scope::new(&[]), &[(0, 20)])
            .unwrap();
        assert_eq!(store.sessions().unwrap().len(), 3);
    }

    #[test]
    fn keys_titles_and_subagents_are_kept_per_session() {
        let mut store = Store::in_memory().unwrap();
        let mut sub = message("b", "s1", 2, true, 1);
        sub.agent_id = Some("a1".into());
        let contents = FileContents {
            messages: vec![message("a", "s1", 1, false, 1), sub],
            titles: vec![("s1".into(), "Fix checkout".into())],
            subagents: vec![("s1".into(), "a1".into(), "reviewer".into())],
            ..FileContents::default()
        };
        let keys = vec![
            ("s1".to_owned(), "KD-2".to_owned(), 5),
            ("s1".to_owned(), "KD-1".to_owned(), 9),
            ("s1".to_owned(), "KD-2".to_owned(), 12),
        ];
        add(&mut store, "/f", &contents, &keys);
        add(&mut store, "/f", &contents, &keys);
        store.create_sessions(0, &mut Scope::new(&[]), &[]).unwrap();
        let found = store.keys().unwrap();
        assert_eq!(found.get("claude-code:s1").unwrap(), &["KD-2", "KD-1"]);
        assert_eq!(
            store
                .titles()
                .unwrap()
                .get("claude-code:s1")
                .map(String::as_str),
            Some("Fix checkout")
        );
        let messages = store.counted_messages().unwrap();
        let subagents: Vec<Option<&str>> = messages.iter().map(|m| m.subagent.as_deref()).collect();
        assert_eq!(subagents, [None, Some("reviewer")]);
        assert_eq!(
            store.session_counts().unwrap(),
            [("claude-code".to_owned(), 1)]
        );
    }

    #[test]
    fn only_sessions_in_tracked_folders_or_recorded_ones_count() {
        let mut store = Store::in_memory().unwrap();
        let mut elsewhere = message("b", "other", 200, false, 1);
        elsewhere.directory = Some("/elsewhere".into());
        let mut sent = message("c", "sent", 300, false, 1);
        sent.directory = Some("/elsewhere".into());
        let mut nowhere = message("d", "nowhere", 400, false, 1);
        nowhere.directory = None;
        let contents = FileContents {
            messages: vec![
                message("a", "here", 100, false, 1),
                elsewhere,
                sent,
                nowhere,
            ],
            ..FileContents::default()
        };
        add(&mut store, "/f", &contents, &[]);
        store.create_sessions(0, &mut Scope::new(&[]), &[]).unwrap();
        assert_eq!(store.sessions().unwrap().len(), 4);
        store
            .append_event(&split_event("e1", "other"), Some("claude-code:other"))
            .unwrap();
        store
            .append_event(&split_event("e2", "here"), Some("claude-code:here"))
            .unwrap();

        // Tracking /w: sessions elsewhere go with their splits, the
        // submitted one stays.
        let mut tracked = Scope::new(&["/w".into()]);
        store
            .create_sessions(0, &mut tracked, &[(250, 350)])
            .unwrap();
        let counted: Vec<String> = store
            .sessions()
            .unwrap()
            .into_iter()
            .map(|s| s.session_id)
            .collect();
        assert_eq!(counted, ["here", "sent"]);
        let ids: Vec<String> = store
            .events()
            .unwrap()
            .into_iter()
            .map(|event| event.id.to_string())
            .collect();
        assert_eq!(ids, ["e2"]);

        // A recorded session counts wherever it ran.
        store.mark_recorded(Harness::ClaudeCode, "other").unwrap();
        store
            .create_sessions(0, &mut tracked, &[(250, 350)])
            .unwrap();
        assert_eq!(store.sessions().unwrap().len(), 3);

        let mut folders = store.session_folders().unwrap();
        folders.sort_by_key(|f| f.last_ms);
        assert_eq!(folders.len(), 4);
        assert_eq!(folders.first().unwrap().folder.as_deref(), Some("/w"));
        assert_eq!(folders.last().unwrap().folder, None);
    }

    #[test]
    fn events_and_meta_are_kept_in_order() {
        let store = Store::in_memory().unwrap();
        assert_eq!(store.next_event_id().unwrap(), "local-1");
        assert_eq!(store.next_event_id().unwrap(), "local-2");
        store.append_event(&split_event("b", "s1"), None).unwrap();
        store.append_event(&split_event("a", "s1"), None).unwrap();
        assert!(store.append_event(&split_event("a", "s1"), None).is_err());
        let ids: Vec<String> = store
            .events()
            .unwrap()
            .into_iter()
            .map(|event| event.id.to_string())
            .collect();
        assert_eq!(ids, ["b", "a"]);
        assert_eq!(store.meta("x").unwrap(), None);
        store.set_meta("x", "1").unwrap();
        store.set_meta("x", "2").unwrap();
        assert_eq!(store.meta("x").unwrap().as_deref(), Some("2"));
        store.set_meta("p:a", "1").unwrap();
        store.set_meta("p:b", "2").unwrap();
        store.set_meta("q", "3").unwrap();
        assert_eq!(store.meta_with_prefix("p:").unwrap().len(), 2);
        assert_eq!(local_id(Harness::Codex, "cx-1"), "codex:cx-1");
    }

    #[test]
    fn a_version_one_database_is_brought_to_version_two() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(SCHEMA_V1).unwrap();
        connection
            .execute_batch("INSERT INTO meta (key, value) VALUES ('context', '{}');
                            INSERT INTO files (path, harness, size, modified_ms) VALUES ('/f', 'codex', 1, 2);")
            .unwrap();
        connection.pragma_update(None, "user_version", 1).unwrap();
        let store = Store::with(connection).unwrap();
        assert_eq!(store.meta("context").unwrap(), None);
        assert!(!store.file_unchanged("/f", 1, 2).unwrap());
        assert!(store.sessions().unwrap().is_empty());
        assert!(store.events().unwrap().is_empty());
    }

    #[test]
    fn a_database_from_a_newer_tc_is_refused() {
        let connection = Connection::open_in_memory().unwrap();
        connection.pragma_update(None, "user_version", 99).unwrap();
        assert!(matches!(Store::with(connection), Err(Error::Usage(_))));
    }
}
