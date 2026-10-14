//! The local `SQLite` file: model calls read from session files, the draft
//! token entries built from them, and what was set by hand. No prompt text is
//! ever written here; only the ticket keys found in it.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::error::{Error, Result};
use crate::folders::Scope;
use crate::placing::KeyCount;
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

const SCHEMA_VERSION: i64 = 1;

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

/// Where a token entry stands on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Draft,
    Submitted,
    Rejected,
}

impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Submitted => "submitted",
            Self::Rejected => "rejected",
        }
    }

    fn from_name(name: &str) -> Self {
        match name {
            "submitted" => Self::Submitted,
            "rejected" => Self::Rejected,
            _ => Self::Draft,
        }
    }
}

/// A token entry's row: what was set on it, not what it adds up to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryRow {
    pub local_id: String,
    pub number: i64,
    pub harness: Harness,
    pub session_id: String,
    pub status: Status,
    pub summary: Option<String>,
    pub named_tags: BTreeMap<String, String>,
    /// Shares set with `tc split`: ticket and percent.
    pub hand_split: Option<Vec<(String, u64)>>,
    /// Signals sent with `tc record`, such as a pull request number.
    pub signals: BTreeMap<String, String>,
    pub rejected_reason: Option<String>,
    pub remote_id: Option<String>,
    pub pick: Option<String>,
    pub pick_question: Option<String>,
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

/// The local ID of a session's token entry, such as `claude-code:6b1f…`.
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
        if version < 1 {
            let transaction = self.connection.transaction()?;
            transaction.execute_batch(SCHEMA_V1)?;
            transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            transaction.commit()?;
        }
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

    pub fn delete_meta(&self, key: &str) -> Result<()> {
        self.connection
            .execute("DELETE FROM meta WHERE key = ?1", [key])?;
        Ok(())
    }

    /// Whether a file was read before at this size and time.
    #[cfg(test)]
    pub fn file_unchanged(&self, path: &str, size: u64, modified_ms: i64) -> Result<bool> {
        file_unchanged(&self.connection, path, size, modified_ms)
    }

    /// Makes every file be read again, such as after the ticket key patterns
    /// changed.
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

    /// Personal rules kept on this machine, in order, as JSON.
    pub fn personal_rules(&self) -> Result<Vec<String>> {
        let mut statement = self
            .connection
            .prepare("SELECT rule FROM personal_rules ORDER BY position")?;
        let rules = statement
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        Ok(rules)
    }

    pub fn set_personal_rules(&mut self, rules: &[String]) -> Result<()> {
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM personal_rules", [])?;
        for (position, rule) in rules.iter().enumerate() {
            transaction.execute(
                "INSERT INTO personal_rules (position, rule) VALUES (?1, ?2)",
                params![to_i64(u64::try_from(position).unwrap_or(u64::MAX)), rule],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Creates a draft token entry for every session that has none, started
    /// at or after `since_ms` and counts by `scope` or was recorded,
    /// numbered in the order they started. A submitted entry whose session
    /// went on after it was submitted becomes a draft again. A draft that
    /// was never sent goes away once its session no longer counts; a sent
    /// entry is never touched.
    pub fn create_entries(&mut self, since_ms: i64, scope: &mut Scope) -> Result<()> {
        let recorded: HashSet<String> = self
            .meta_with_prefix(RECORDED)?
            .into_iter()
            .filter_map(|(key, _)| key.strip_prefix(RECORDED).map(str::to_owned))
            .collect();
        let transaction = self.connection.transaction()?;
        let sessions: Vec<(String, String, i64, String, Option<String>)> = {
            let mut statement = transaction.prepare(&format!(
                "SELECT m.harness, m.session_id, MIN(m.at_ms),
                        COUNT(*) || ':' || MAX(m.at_ms), {SESSION_FOLDER}
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
            transaction.query_row("SELECT COALESCE(MAX(number), 0) FROM entries", [], |row| {
                row.get(0)
            })?;
        for (harness, session_id, started_ms, fingerprint, folder) in sessions {
            let local = format!("{harness}:{session_id}");
            if !recorded.contains(&local) && !scope.includes(folder.as_deref()) {
                transaction.execute(
                    "DELETE FROM entries
                     WHERE local_id = ?1 AND status = 'draft' AND remote_id IS NULL",
                    [&local],
                )?;
                continue;
            }
            let existing: Option<(String, Option<String>)> = transaction
                .query_row(
                    "SELECT status, submitted_fingerprint FROM entries WHERE local_id = ?1",
                    [&local],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            match existing {
                None if started_ms >= since_ms => {
                    next = next
                        .checked_add(1)
                        .ok_or_else(|| Error::Data("too many entries".into()))?;
                    transaction.execute(
                        "INSERT INTO entries (local_id, number, harness, session_id) VALUES (?1, ?2, ?3, ?4)",
                        params![local, next, harness, session_id],
                    )?;
                }
                Some((status, submitted))
                    if status == "submitted"
                        && submitted.as_deref() != Some(fingerprint.as_str()) =>
                {
                    transaction.execute(
                        "UPDATE entries SET status = 'draft' WHERE local_id = ?1",
                        [&local],
                    )?;
                }
                _ => {}
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

    /// Token entries with one of the given statuses, oldest first.
    pub fn entries(&self, statuses: &[Status]) -> Result<Vec<EntryRow>> {
        let mut statement = self.connection.prepare(
            "SELECT local_id, number, harness, session_id, status, summary, named_tags,
                    hand_split, signals, rejected_reason, remote_id, pick, pick_question
             FROM entries ORDER BY number",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, Option<String>>(12)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut entries = Vec::new();
        for (
            local_id,
            number,
            harness,
            session_id,
            status,
            summary,
            named,
            split,
            signals,
            rejected_reason,
            remote_id,
            pick,
            pick_question,
        ) in rows
        {
            let status = Status::from_name(&status);
            if !statuses.contains(&status) {
                continue;
            }
            let Some(harness) = Harness::from_name(&harness) else {
                continue;
            };
            entries.push(EntryRow {
                local_id,
                number,
                harness,
                session_id,
                status,
                summary,
                named_tags: serde_json::from_str(&named).unwrap_or_default(),
                hand_split: split.and_then(|split| serde_json::from_str(&split).ok()),
                signals: serde_json::from_str(&signals).unwrap_or_default(),
                rejected_reason,
                remote_id,
                pick,
                pick_question,
            });
        }
        Ok(entries)
    }

    /// The entry with a number, whatever its status.
    pub fn entry(&self, number: i64) -> Result<Option<EntryRow>> {
        let all = self.entries(&[Status::Draft, Status::Rejected, Status::Submitted])?;
        Ok(all.into_iter().find(|entry| entry.number == number))
    }

    pub fn entry_by_session(&self, harness: Harness, session_id: &str) -> Result<Option<EntryRow>> {
        let local = local_id(harness, session_id);
        let all = self.entries(&[Status::Draft, Status::Rejected, Status::Submitted])?;
        Ok(all.into_iter().find(|entry| entry.local_id == local))
    }

    pub fn set_named_tags(&self, local_id: &str, tags: &BTreeMap<String, String>) -> Result<()> {
        self.update_text(local_id, "named_tags", Some(&serde_json::to_string(tags)?))
    }

    pub fn set_signals(&self, local_id: &str, signals: &BTreeMap<String, String>) -> Result<()> {
        self.update_text(local_id, "signals", Some(&serde_json::to_string(signals)?))
    }

    pub fn set_hand_split(&self, local_id: &str, split: Option<&[(String, u64)]>) -> Result<()> {
        let text = split.map(serde_json::to_string).transpose()?;
        self.update_text(local_id, "hand_split", text.as_deref())
    }

    pub fn set_summary(&self, local_id: &str, summary: Option<&str>) -> Result<()> {
        self.update_text(local_id, "summary", summary)
    }

    /// Remembers the classifier's answer and the question it answered, so it
    /// is asked again only when the question changes.
    pub fn set_pick(&self, local_id: &str, pick: &str, question: &str) -> Result<()> {
        self.connection.execute(
            "UPDATE entries SET pick = ?2, pick_question = ?3 WHERE local_id = ?1",
            params![local_id, pick, question],
        )?;
        Ok(())
    }

    fn update_text(&self, local_id: &str, column: &str, value: Option<&str>) -> Result<()> {
        // The column comes from this file only, never from input.
        let sql = format!("UPDATE entries SET {column} = ?2 WHERE local_id = ?1");
        self.connection.execute(&sql, params![local_id, value])?;
        Ok(())
    }

    /// Marks entries submitted, with the service's IDs.
    pub fn mark_submitted(
        &mut self,
        submitted: &[(String, Option<String>)],
        at_ms: i64,
    ) -> Result<()> {
        let transaction = self.connection.transaction()?;
        for (local, remote_id) in submitted {
            let fingerprint: Option<String> = transaction
                .query_row(
                    "SELECT COUNT(*) || ':' || MAX(m.at_ms) FROM messages m
                     JOIN entries e ON e.harness = m.harness AND e.session_id = m.session_id
                     WHERE e.local_id = ?1",
                    [local],
                    |row| row.get(0),
                )
                .optional()?;
            transaction.execute(
                "UPDATE entries SET status = 'submitted', remote_id = COALESCE(?2, remote_id),
                        submitted_at_ms = ?3, submitted_fingerprint = ?4, rejected_reason = NULL
                 WHERE local_id = ?1",
                params![local, remote_id, at_ms, fingerprint],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Marks entries the service sent back, with the reason.
    pub fn mark_rejected(&self, local_id: &str, reason: &str) -> Result<()> {
        self.connection.execute(
            "UPDATE entries SET status = 'rejected', rejected_reason = ?2
             WHERE local_id = ?1 AND status = 'submitted'",
            params![local_id, reason],
        )?;
        Ok(())
    }

    /// The model calls of a session, oldest first, with subagent types.
    pub fn messages(&self, harness: Harness, session_id: &str) -> Result<Vec<StoredMessage>> {
        let mut statement = self.connection.prepare(
            "SELECT m.message_id, m.request_id, m.at_ms, m.model, m.input, m.output,
                    m.cache_write_5m, m.cache_write_1h, m.cache_read, m.reported_cost_nanos,
                    m.branch, m.directory, m.repository, s.subagent_type
             FROM messages m
             LEFT JOIN subagents s
               ON s.harness = m.harness AND s.session_id = m.session_id AND s.agent_id = m.agent_id
             WHERE m.harness = ?1 AND m.session_id = ?2
             ORDER BY m.at_ms, m.message_key",
        )?;
        let messages = statement
            .query_map(params![harness.name(), session_id], |row| {
                Ok(StoredMessage {
                    message_id: row.get(0)?,
                    request_id: row.get(1)?,
                    at_ms: row.get(2)?,
                    model: row.get(3)?,
                    tokens: Tokens {
                        input: to_u64(row.get(4)?),
                        output: to_u64(row.get(5)?),
                        cache_write_5m: to_u64(row.get(6)?),
                        cache_write_1h: to_u64(row.get(7)?),
                        cache_read: to_u64(row.get(8)?),
                    },
                    reported_cost: row.get(9)?,
                    branch: row.get(10)?,
                    directory: row.get(11)?,
                    repository: row.get(12)?,
                    subagent: row.get(13)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(messages)
    }

    /// The files in which a session named ticket keys.
    pub fn key_files(&self, harness: Harness, session_id: &str) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT path FROM session_keys WHERE harness = ?1 AND session_id = ?2 ORDER BY path",
        )?;
        let paths = statement
            .query_map(params![harness.name(), session_id], |row| row.get(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        Ok(paths)
    }

    /// Ticket keys found in a session's prompts and title, over all its
    /// files, in the order they were first named.
    pub fn keys(&self, harness: Harness, session_id: &str) -> Result<Vec<KeyCount>> {
        let mut statement = self.connection.prepare(
            "SELECT key, SUM(count), MIN(first_at_ms), MAX(last_at_ms)
             FROM session_keys WHERE harness = ?1 AND session_id = ?2
             GROUP BY key ORDER BY MIN(first_at_ms), key",
        )?;
        let keys = statement
            .query_map(params![harness.name(), session_id], |row| {
                Ok(KeyCount {
                    key: row.get(0)?,
                    count: to_u64(row.get(1)?),
                    first_at_ms: row.get(2)?,
                    last_at_ms: row.get(3)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(keys)
    }

    pub fn title(&self, harness: Harness, session_id: &str) -> Result<Option<String>> {
        Ok(self
            .connection
            .query_row(
                "SELECT title FROM session_titles WHERE harness = ?1 AND session_id = ?2",
                params![harness.name(), session_id],
                |row| row.get(0),
            )
            .optional()?)
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

    /// Stores what was read from one file. `keys` are the ticket keys found
    /// in its prompts and titles, per session; the file's earlier keys are
    /// replaced.
    pub fn add_file(
        &self,
        path: &str,
        harness: Harness,
        size: u64,
        modified_ms: i64,
        contents: &FileContents,
        keys: &[(String, KeyCount)],
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
        for (session_id, key) in keys {
            t.execute(
                "INSERT INTO session_keys (path, harness, session_id, key, count, first_at_ms, last_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (path, harness, session_id, key) DO UPDATE SET
                    count = session_keys.count + excluded.count,
                    first_at_ms = MIN(session_keys.first_at_ms, excluded.first_at_ms),
                    last_at_ms = MAX(session_keys.last_at_ms, excluded.last_at_ms)",
                params![
                    path,
                    harness_name,
                    session_id,
                    key.key,
                    to_i64(key.count),
                    key.first_at_ms,
                    key.last_at_ms
                ],
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

    fn add(store: &mut Store, path: &str, contents: &FileContents, keys: &[(String, KeyCount)]) {
        let batch = store.batch().unwrap();
        batch
            .add_file(path, Harness::ClaudeCode, 10, 20, contents, keys)
            .unwrap();
        batch.commit().unwrap();
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
        let messages = store.messages(Harness::ClaudeCode, "s1").unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages.first().unwrap().tokens.input, 7);
        // Reading the same file again changes nothing.
        add(&mut store, "/f", &contents, &[]);
        assert_eq!(store.messages(Harness::ClaudeCode, "s1").unwrap().len(), 2);
        assert!(store.file_unchanged("/f", 10, 20).unwrap());
        assert!(!store.file_unchanged("/f", 11, 20).unwrap());
        store.forget_files().unwrap();
        assert!(!store.file_unchanged("/f", 10, 20).unwrap());
    }

    #[test]
    fn entries_are_numbered_in_order_and_only_from_since() {
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
        store.create_entries(50, &mut Scope::new(&[])).unwrap();
        let entries = store.entries(&[Status::Draft]).unwrap();
        let sessions: Vec<(i64, &str)> = entries
            .iter()
            .map(|e| (e.number, e.session_id.as_str()))
            .collect();
        assert_eq!(sessions, [(1, "early"), (2, "late")]);
        store.create_entries(50, &mut Scope::new(&[])).unwrap();
        assert_eq!(store.entries(&[Status::Draft]).unwrap().len(), 2);
    }

    #[test]
    fn a_session_that_goes_on_after_submit_is_a_draft_again() {
        let mut store = Store::in_memory().unwrap();
        let mut contents = FileContents {
            messages: vec![message("a", "s1", 100, false, 1)],
            ..FileContents::default()
        };
        add(&mut store, "/f", &contents, &[]);
        store.create_entries(0, &mut Scope::new(&[])).unwrap();
        let local = local_id(Harness::ClaudeCode, "s1");
        store
            .mark_submitted(&[(local.clone(), Some("te_1".into()))], 500)
            .unwrap();
        store.create_entries(0, &mut Scope::new(&[])).unwrap();
        let entry = store.entry(1).unwrap().unwrap();
        assert_eq!(entry.status, Status::Submitted);
        assert_eq!(entry.remote_id.as_deref(), Some("te_1"));

        store.mark_rejected(&local, "wrong client").unwrap();
        let rejected = store.entry(1).unwrap().unwrap();
        assert_eq!(rejected.status, Status::Rejected);
        assert_eq!(rejected.rejected_reason.as_deref(), Some("wrong client"));
        store.mark_submitted(&[(local.clone(), None)], 600).unwrap();

        contents.messages.push(message("b", "s1", 200, false, 1));
        add(&mut store, "/f", &contents, &[]);
        store.create_entries(0, &mut Scope::new(&[])).unwrap();
        let entry = store.entry(1).unwrap().unwrap();
        assert_eq!(entry.status, Status::Draft);
        assert_eq!(entry.remote_id.as_deref(), Some("te_1"));
    }

    #[test]
    fn what_is_set_by_hand_is_kept() {
        let mut store = Store::in_memory().unwrap();
        let contents = FileContents {
            messages: vec![message("a", "s1", 100, false, 1)],
            titles: vec![("s1".into(), "Fix checkout".into())],
            subagents: vec![("s1".into(), "a1".into(), "reviewer".into())],
            ..FileContents::default()
        };
        let keys = vec![(
            "s1".to_owned(),
            KeyCount {
                key: "KD-2".into(),
                count: 2,
                first_at_ms: 5,
                last_at_ms: 9,
            },
        )];
        add(&mut store, "/f", &contents, &keys);
        add(&mut store, "/f", &contents, &keys);
        store.create_entries(0, &mut Scope::new(&[])).unwrap();
        let local = local_id(Harness::ClaudeCode, "s1");
        store
            .set_named_tags(&local, &BTreeMap::from([("ticket".into(), "KD-3".into())]))
            .unwrap();
        store
            .set_hand_split(&local, Some(&[("KD-3".into(), 60), ("KD-4".into(), 40)]))
            .unwrap();
        store
            .set_signals(
                &local,
                &BTreeMap::from([("pull_request".into(), "42".into())]),
            )
            .unwrap();
        store.set_summary(&local, Some("Fixed it")).unwrap();
        store.set_pick(&local, "KD-3", "q1").unwrap();
        let entry = store
            .entry_by_session(Harness::ClaudeCode, "s1")
            .unwrap()
            .unwrap();
        assert_eq!(
            entry.named_tags.get("ticket").map(String::as_str),
            Some("KD-3")
        );
        assert_eq!(entry.hand_split.as_ref().map(Vec::len), Some(2));
        assert_eq!(
            entry.signals.get("pull_request").map(String::as_str),
            Some("42")
        );
        assert_eq!(entry.summary.as_deref(), Some("Fixed it"));
        assert_eq!(entry.pick.as_deref(), Some("KD-3"));
        assert_eq!(entry.pick_question.as_deref(), Some("q1"));
        store.set_hand_split(&local, None).unwrap();
        assert!(store.entry(1).unwrap().unwrap().hand_split.is_none());

        // Keys from one file are replaced, not added, when it is read again.
        let found = store.keys(Harness::ClaudeCode, "s1").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found.first().unwrap().count, 2);
        assert_eq!(
            store.title(Harness::ClaudeCode, "s1").unwrap().as_deref(),
            Some("Fix checkout")
        );
        assert_eq!(
            store.session_counts().unwrap(),
            [("claude-code".to_owned(), 1)]
        );
        assert!(store.entry(99).unwrap().is_none());
    }

    #[test]
    fn only_sessions_in_tracked_folders_or_recorded_ones_become_entries() {
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
        store.create_entries(0, &mut Scope::new(&[])).unwrap();
        assert_eq!(store.entries(&[Status::Draft]).unwrap().len(), 4);
        let sent_id = local_id(Harness::ClaudeCode, "sent");
        store.mark_submitted(&[(sent_id, None)], 500).unwrap();

        // Tracking /w: drafts elsewhere go, the submitted entry stays.
        let mut tracked = Scope::new(&["/w".into()]);
        store.create_entries(0, &mut tracked).unwrap();
        let drafts: Vec<String> = store
            .entries(&[Status::Draft])
            .unwrap()
            .into_iter()
            .map(|e| e.session_id)
            .collect();
        assert_eq!(drafts, ["here"]);
        assert_eq!(store.entries(&[Status::Submitted]).unwrap().len(), 1);

        // A recorded session counts wherever it ran.
        store.mark_recorded(Harness::ClaudeCode, "other").unwrap();
        store.create_entries(0, &mut tracked).unwrap();
        assert_eq!(store.entries(&[Status::Draft]).unwrap().len(), 2);

        let mut folders = store.session_folders().unwrap();
        folders.sort_by_key(|f| f.last_ms);
        assert_eq!(folders.len(), 4);
        assert_eq!(folders.first().unwrap().folder.as_deref(), Some("/w"));
        assert_eq!(folders.last().unwrap().folder, None);
    }

    #[test]
    fn subagent_types_are_joined_to_their_messages() {
        let mut store = Store::in_memory().unwrap();
        let mut sub = message("b", "s1", 2, true, 1);
        sub.agent_id = Some("a1".into());
        let contents = FileContents {
            messages: vec![message("a", "s1", 1, false, 1), sub],
            subagents: vec![("s1".into(), "a1".into(), "reviewer".into())],
            ..FileContents::default()
        };
        add(&mut store, "/f", &contents, &[]);
        let messages = store.messages(Harness::ClaudeCode, "s1").unwrap();
        let subagents: Vec<Option<&str>> = messages.iter().map(|m| m.subagent.as_deref()).collect();
        assert_eq!(subagents, [None, Some("reviewer")]);
    }

    #[test]
    fn personal_rules_and_meta_are_kept_in_order() {
        let mut store = Store::in_memory().unwrap();
        store
            .set_personal_rules(&["one".into(), "two".into()])
            .unwrap();
        assert_eq!(store.personal_rules().unwrap(), ["one", "two"]);
        store.set_personal_rules(&["three".into()]).unwrap();
        assert_eq!(store.personal_rules().unwrap(), ["three"]);
        assert_eq!(store.meta("x").unwrap(), None);
        store.set_meta("x", "1").unwrap();
        store.set_meta("x", "2").unwrap();
        assert_eq!(store.meta("x").unwrap().as_deref(), Some("2"));
        store.set_meta("p:a", "1").unwrap();
        store.set_meta("p:b", "2").unwrap();
        store.set_meta("q", "3").unwrap();
        assert_eq!(store.meta_with_prefix("p:").unwrap().len(), 2);
        store.delete_meta("p:a").unwrap();
        assert_eq!(
            store.meta_with_prefix("p:").unwrap(),
            [("p:b".to_owned(), "2".to_owned())]
        );
    }

    #[test]
    fn a_database_from_a_newer_tc_is_refused() {
        let connection = Connection::open_in_memory().unwrap();
        connection.pragma_update(None, "user_version", 99).unwrap();
        assert!(matches!(Store::with(connection), Err(Error::Usage(_))));
    }
}
