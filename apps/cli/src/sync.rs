//! Brings the local database up to date with the session files on this
//! machine. Every command runs it first; there is no background process.
//! Files that have not changed since the last run are skipped, and reading a
//! file again changes nothing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::error::Result;
use crate::folders::Scope;
use crate::keys::KeyPattern;
use crate::sources::{self, FileContents, Harness};
use crate::store::Store;

/// Where the session files are; the real folders unless a test says
/// otherwise.
pub struct Sources {
    pub files: Vec<(Harness, PathBuf)>,
}

impl Sources {
    pub fn on_this_machine() -> Self {
        Self {
            files: sources::session_files(),
        }
    }
}

/// What to read and which sessions get a number.
pub struct Reading<'a> {
    /// The pattern feature keys are found with, if any.
    pub pattern: Option<&'a KeyPattern>,
    /// Sessions that started earlier get no number.
    pub since_ms: i64,
    /// Sessions outside these folders get no number.
    pub scope: Scope,
    /// Windows already submitted, in milliseconds: sessions inside them
    /// keep their number wherever they ran.
    pub submitted: Vec<(i64, i64)>,
}

/// Reads new and changed session files and numbers the sessions that
/// count.
pub fn sync(store: &mut Store, found: &Sources, reading: &mut Reading<'_>) -> Result<()> {
    let pattern_text = reading.pattern.map(KeyPattern::as_str).unwrap_or_default();
    // Keys are found while reading, so a new pattern means reading again.
    if store.meta("key_pattern")?.as_deref().unwrap_or_default() != pattern_text {
        store.forget_files()?;
        store.set_meta("key_pattern", pattern_text)?;
    }
    let mut repositories: HashMap<String, Option<String>> = HashMap::new();
    let batch = store.batch()?;
    for (harness, path) in &found.files {
        let Ok(metadata) = std::fs::metadata(path) else {
            continue;
        };
        let modified_ms = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .and_then(|duration| i64::try_from(duration.as_millis()).ok())
            .unwrap_or(0);
        let path_text = path.to_string_lossy().into_owned();
        if batch
            .file_unchanged(&path_text, metadata.len(), modified_ms)
            .unwrap_or(false)
        {
            continue;
        }
        let Ok(mut contents) = sources::read(*harness, path) else {
            continue;
        };
        fill_repositories(&mut contents, &mut repositories);
        let keys = keys_found(&contents, reading.pattern);
        batch.add_file(
            &path_text,
            *harness,
            metadata.len(),
            modified_ms,
            &contents,
            &keys,
        )?;
    }
    batch.commit()?;
    store.create_sessions(reading.since_ms, &mut reading.scope, &reading.submitted)?;
    Ok(())
}

/// Claude Code writes the working directory, not the repository; the
/// repository's name comes from the git config there, if it still exists.
fn fill_repositories(contents: &mut FileContents, cache: &mut HashMap<String, Option<String>>) {
    for message in &mut contents.messages {
        if message.repository.is_some() {
            continue;
        }
        let Some(directory) = message.directory.clone() else {
            continue;
        };
        message.repository.clone_from(
            cache
                .entry(directory)
                .or_insert_with_key(|directory| crate::repo::name_of(Path::new(directory))),
        );
    }
}

/// The feature keys in a file's prompts and titles: session, key and the
/// time named, one row per mention. The text itself is dropped here.
fn keys_found(contents: &FileContents, pattern: Option<&KeyPattern>) -> Vec<(String, String, i64)> {
    let Some(pattern) = pattern else {
        return Vec::new();
    };
    let titles = contents
        .titles
        .iter()
        .map(|(session, title)| (session, 0_i64, title));
    let prompts = contents
        .prompts
        .iter()
        .map(|prompt| (&prompt.session_id, prompt.at_ms, &prompt.text));
    let mut found = Vec::new();
    for (session, at_ms, text) in prompts.chain(titles) {
        for key in pattern.find_all(text) {
            found.push((session.clone(), key, at_ms));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::Prompt;

    fn everything(pattern: Option<&KeyPattern>) -> Reading<'_> {
        Reading {
            pattern,
            since_ms: 0,
            scope: Scope::new(&[]),
            submitted: Vec::new(),
        }
    }

    fn pattern() -> KeyPattern {
        KeyPattern::new(r"^KD-\d+").unwrap()
    }

    #[test]
    fn keys_are_counted_per_session_and_text_is_dropped() {
        let contents = FileContents {
            prompts: vec![
                Prompt {
                    session_id: "s1".into(),
                    at_ms: 10,
                    text: "fix KD-1 and KD-2".into(),
                },
                Prompt {
                    session_id: "s1".into(),
                    at_ms: 20,
                    text: "KD-1 again".into(),
                },
            ],
            titles: vec![("s1".into(), "KD-1 checkout".into())],
            ..FileContents::default()
        };
        let found = keys_found(&contents, Some(&pattern()));
        assert_eq!(found.len(), 4);
        assert_eq!(
            found.first().unwrap(),
            &("s1".to_owned(), "KD-1".to_owned(), 10)
        );
        assert_eq!(
            found.last().unwrap(),
            &("s1".to_owned(), "KD-1".to_owned(), 0)
        );
        assert!(keys_found(&contents, None).is_empty());
    }

    #[test]
    fn session_files_become_numbered_sessions_once() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("s1.jsonl");
        std::fs::write(
            &file,
            concat!(
                r#"{"type":"user","sessionId":"s1","timestamp":"2026-10-11T09:00:00Z","message":{"content":"Fix KD-214"}}"#,
                "\n",
                r#"{"type":"assistant","sessionId":"s1","timestamp":"2026-10-11T09:12:00Z","cwd":"/nowhere","gitBranch":"main","requestId":"r1","message":{"id":"msg_1","model":"claude-sonnet-4-5","usage":{"input_tokens":1000,"output_tokens":100}}}"#,
                "\n"
            ),
        )
        .unwrap();
        let found = Sources {
            files: vec![(Harness::ClaudeCode, file.clone())],
        };
        let mut store = Store::open(&dir.path().join("tc.sqlite")).unwrap();
        let first = pattern();
        sync(&mut store, &found, &mut everything(Some(&first))).unwrap();
        sync(&mut store, &found, &mut everything(Some(&first))).unwrap();
        assert_eq!(store.sessions().unwrap().len(), 1);
        let keys = store.keys().unwrap();
        assert_eq!(keys.get("claude-code:s1").unwrap(), &["KD-214"]);

        // A new pattern: the file is read again and keys follow it.
        let other = KeyPattern::new(r"OPS-\d+").unwrap();
        sync(&mut store, &found, &mut everything(Some(&other))).unwrap();
        assert!(store.keys().unwrap().is_empty());
        assert_eq!(store.sessions().unwrap().len(), 1);

        // A missing file is skipped.
        let gone = Sources {
            files: vec![(Harness::ClaudeCode, dir.path().join("gone.jsonl"))],
        };
        sync(&mut store, &gone, &mut everything(None)).unwrap();

        // Once another folder is tracked, the session no longer counts.
        let mut elsewhere = Reading {
            scope: Scope::new(&["/somewhere".into()]),
            ..everything(None)
        };
        sync(&mut store, &found, &mut elsewhere).unwrap();
        assert!(store.sessions().unwrap().is_empty());
    }
}
