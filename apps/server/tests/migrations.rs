//! Database changes come in two releases (ADR 0027): a migration runs while the
//! version before it still serves, so it may not break that version.

use std::fs;
use std::path::Path;

/// Migrations from this one on are checked; the ones before ran under the
/// earlier rules and are never edited.
const CHECKED_FROM: &str = "20261109000000";

/// A migration that takes away what the version before it used marks itself
/// as the second step with a line that starts like this.
const SECOND_STEP: &str = "-- second step:";

/// What the version still serving can trip over, in plain words.
fn breaking_changes(sql: &str) -> Vec<&'static str> {
    if sql
        .lines()
        .any(|line| line.trim_start().starts_with(SECOND_STEP))
    {
        return Vec::new();
    }
    let code: String = sql
        .lines()
        .map(|line| line.split("--").next().unwrap_or_default())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let mut found = Vec::new();
    for statement in code.split(';') {
        let words: Vec<&str> = statement.split_whitespace().collect();
        let text = format!(" {} ", words.join(" "));
        let checks: [(bool, &'static str); 8] = [
            (text.contains(" drop table "), "drops a table"),
            (text.contains(" drop column "), "drops a column"),
            (
                text.contains(" drop view ") || text.contains(" drop function "),
                "drops a view or function",
            ),
            (text.contains(" rename "), "renames something"),
            (
                text.contains(" alter column ") && text.contains(" type "),
                "changes a column's type",
            ),
            (text.contains(" set not null "), "makes a column required"),
            (
                text.contains(" add column ")
                    && text.contains(" not null ")
                    && !text.contains(" default "),
                "adds a required column without a default",
            ),
            (
                text.contains(" add constraint "),
                "adds a constraint to an existing table",
            ),
        ];
        for (hit, what) in checks {
            if hit && !found.contains(&what) {
                found.push(what);
            }
        }
    }
    found
}

#[test]
fn new_migrations_keep_the_running_version_working() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let mut problems = Vec::new();
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name.as_str() < CHECKED_FROM {
            continue;
        }
        let sql = fs::read_to_string(&path).unwrap();
        for what in breaking_changes(&sql) {
            problems.push(format!("{name} {what}"));
        }
    }
    assert!(
        problems.is_empty(),
        "A migration runs while the version before it still serves (ADR 0027). \
         Split the change: this release adds and stops using, the next one takes away \
         and starts with a line `{SECOND_STEP} <what no released version uses any more>`.\n{}",
        problems.join("\n")
    );
}

#[test]
fn adding_is_fine() {
    let sql = "CREATE TABLE notes (id uuid PRIMARY KEY, body text NOT NULL);\n\
               ALTER TABLE events ADD COLUMN source text;\n\
               ALTER TABLE events ADD COLUMN weight bigint NOT NULL DEFAULT 0;\n\
               CREATE INDEX events_source ON events (source);";
    assert!(breaking_changes(sql).is_empty());
}

#[test]
fn taking_away_is_caught() {
    let sql = "ALTER TABLE events DROP COLUMN source;\n\
               ALTER TABLE events RENAME COLUMN kind TO type;\n\
               ALTER TABLE events ALTER COLUMN weight TYPE numeric;\n\
               ALTER TABLE events ALTER COLUMN source SET NOT NULL;\n\
               ALTER TABLE events ADD COLUMN owner text NOT NULL;\n\
               DROP TABLE notes;";
    assert_eq!(
        breaking_changes(sql),
        [
            "drops a column",
            "renames something",
            "changes a column's type",
            "makes a column required",
            "adds a required column without a default",
            "drops a table",
        ]
    );
}

#[test]
fn comments_do_not_count() {
    let sql = "-- The old table was dropped long ago.\nALTER TABLE events ADD COLUMN source text;";
    assert!(breaking_changes(sql).is_empty());
}

#[test]
fn a_marked_second_step_may_take_away() {
    let sql = "-- second step: no released version reads events.source since #130.\n\
               ALTER TABLE events DROP COLUMN source;";
    assert!(breaking_changes(sql).is_empty());
}
