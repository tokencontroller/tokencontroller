//! Brings the local database up to date with the session files on this
//! machine. Every command runs it first; there is no background process.
//! Files that have not changed since the last run are skipped, and reading a
//! file again changes nothing.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::error::Result;
use crate::folders::Scope;
use crate::keys::{KeyPattern, find_all};
use crate::placing::KeyCount;
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

/// What to read and which sessions get a draft token entry.
pub struct Reading<'a> {
    pub patterns: &'a [KeyPattern],
    pub pattern_text: &'a str,
    /// Sessions that started earlier get no draft.
    pub since_ms: i64,
    /// Sessions outside these folders get no draft.
    pub scope: Scope,
}

/// Reads new and changed session files and creates draft token entries for
/// the sessions that count.
pub fn sync(store: &mut Store, found: &Sources, reading: &mut Reading<'_>) -> Result<()> {
    let patterns = reading.patterns;
    let pattern_text = reading.pattern_text;
    // Keys are found while reading, so new patterns mean reading again.
    if store.meta("key_patterns")?.as_deref() != Some(pattern_text) {
        store.forget_files()?;
        store.set_meta("key_patterns", pattern_text)?;
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
        if store_unchanged(&batch, &path_text, metadata.len(), modified_ms) {
            continue;
        }
        let Ok(mut contents) = sources::read(*harness, path) else {
            continue;
        };
        fill_repositories(&mut contents, &mut repositories);
        let keys = key_counts(&contents, patterns);
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
    store.create_entries(reading.since_ms, &mut reading.scope)?;
    Ok(())
}

fn store_unchanged(
    batch: &crate::store::Batch<'_>,
    path: &str,
    size: u64,
    modified_ms: i64,
) -> bool {
    batch
        .file_unchanged(path, size, modified_ms)
        .unwrap_or(false)
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

/// The ticket keys in a file's prompts and titles, per session, counted.
fn key_counts(contents: &FileContents, patterns: &[KeyPattern]) -> Vec<(String, KeyCount)> {
    if patterns.is_empty() {
        return Vec::new();
    }
    let mut counts: BTreeMap<(String, String), KeyCount> = BTreeMap::new();
    let titles = contents
        .titles
        .iter()
        .map(|(session, title)| (session, 0_i64, title));
    let prompts = contents
        .prompts
        .iter()
        .map(|prompt| (&prompt.session_id, prompt.at_ms, &prompt.text));
    for (session, at_ms, text) in prompts.chain(titles) {
        for key in find_all(patterns, text) {
            let count = counts
                .entry((session.clone(), key.clone()))
                .or_insert_with(|| KeyCount {
                    key,
                    count: 0,
                    first_at_ms: at_ms,
                    last_at_ms: at_ms,
                });
            count.count = count.count.saturating_add(1);
            count.first_at_ms = count.first_at_ms.min(at_ms);
            count.last_at_ms = count.last_at_ms.max(at_ms);
        }
    }
    counts
        .into_iter()
        .map(|((session, _), count)| (session, count))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::Prompt;
    use crate::store::Status;

    fn everything<'a>(patterns: &'a [KeyPattern], text: &'a str) -> Reading<'a> {
        Reading {
            patterns,
            pattern_text: text,
            since_ms: 0,
            scope: Scope::new(&[]),
        }
    }

    fn patterns() -> Vec<KeyPattern> {
        vec![KeyPattern::new(Some("storefront".into()), r"^KD-\d+").unwrap()]
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
        let counts = key_counts(&contents, &patterns());
        let kd1 = counts.iter().find(|(_, c)| c.key == "KD-1").unwrap();
        assert_eq!(kd1.1.count, 3);
        assert_eq!(kd1.1.first_at_ms, 0);
        assert_eq!(kd1.1.last_at_ms, 20);
        assert_eq!(counts.len(), 2);
        assert!(key_counts(&contents, &[]).is_empty());
    }

    #[test]
    fn session_files_become_draft_entries_once() {
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
        let first = patterns();
        sync(&mut store, &found, &mut everything(&first, "p1")).unwrap();
        sync(&mut store, &found, &mut everything(&first, "p1")).unwrap();
        let entries = store.entries(&[Status::Draft]).unwrap();
        assert_eq!(entries.len(), 1);
        let keys = store.keys(Harness::ClaudeCode, "s1").unwrap();
        assert_eq!(keys.first().map(|k| k.key.as_str()), Some("KD-214"));

        // New patterns: the file is read again and keys follow them.
        let other = vec![KeyPattern::new(None, r"OPS-\d+").unwrap()];
        sync(&mut store, &found, &mut everything(&other, "p2")).unwrap();
        assert!(store.keys(Harness::ClaudeCode, "s1").unwrap().is_empty());
        assert_eq!(store.entries(&[Status::Draft]).unwrap().len(), 1);

        // A missing file is skipped.
        let gone = Sources {
            files: vec![(Harness::ClaudeCode, dir.path().join("gone.jsonl"))],
        };
        sync(&mut store, &gone, &mut everything(&other, "p2")).unwrap();

        // Once another folder is tracked, the draft goes away.
        let mut elsewhere = Reading {
            scope: Scope::new(&["/somewhere".into()]),
            ..everything(&other, "p2")
        };
        sync(&mut store, &found, &mut elsewhere).unwrap();
        assert!(store.entries(&[Status::Draft]).unwrap().is_empty());
    }
}
