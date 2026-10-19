//! `tc track`, `tc untrack` and `tc list`: which folders' sessions count,
//! and where sessions ran, to choose from.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::app::App;
use crate::error::{Error, Result};
use crate::folders::{self, Scope};
use crate::output;
use crate::sources::Harness;

pub fn track(app: &mut App<'_>, dir: Option<&str>) -> Result<()> {
    let folder = match dir {
        Some(dir) => folders::resolve(dir)?,
        None => folders::current()?,
    };
    if !folders::track(&mut app.config, &folder) {
        eprintln!("tc: {} was tracked already", folder.display());
    }
    app.home.write_config(&app.config)?;
    print_tracked(app)
}

pub fn untrack(app: &mut App<'_>, dir: &str) -> Result<()> {
    if !folders::untrack(&mut app.config, dir) {
        return Err(Error::usage(format!(
            "{dir} is not tracked; tc config folders lists the tracked folders"
        )));
    }
    app.home.write_config(&app.config)?;
    print_tracked(app)
}

/// What counts now: `{"folders": [...]}`, empty when every folder does.
fn print_tracked(app: &mut App<'_>) -> Result<()> {
    if app.json {
        return output::json(
            app.out,
            &serde_json::json!({ "folders": app.config.folders }),
        );
    }
    if app.config.folders.is_empty() {
        writeln!(app.out, "Sessions in every folder count.")?;
    }
    for folder in &app.config.folders {
        writeln!(app.out, "tracking {folder}")?;
    }
    Ok(())
}

/// One folder in `tc list --json`.
#[derive(Debug, Serialize, PartialEq, Eq)]
struct FolderView {
    /// `None` for sessions whose files name no folder.
    folder: Option<String>,
    harnesses: Vec<String>,
    sessions: u64,
    last_at: String,
    tracked: bool,
}

pub fn list(app: &mut App<'_>) -> Result<()> {
    app.read_sessions()?;
    let mut by_folder: BTreeMap<Option<String>, (BTreeSet<Harness>, u64, i64)> = BTreeMap::new();
    for session in app.store.session_folders()? {
        let (harnesses, sessions, last_ms) = by_folder.entry(session.folder).or_default();
        harnesses.insert(session.harness);
        *sessions = sessions.saturating_add(1);
        *last_ms = (*last_ms).max(session.last_ms);
    }
    let mut scope = Scope::new(&app.config.folders);
    let mut rows: Vec<(FolderView, i64, Vec<&'static str>)> = by_folder
        .into_iter()
        .map(|(folder, (harnesses, sessions, last_ms))| {
            let tracked = scope.includes(folder.as_deref());
            let view = FolderView {
                folder,
                harnesses: harnesses.iter().map(|h| h.name().to_owned()).collect(),
                sessions,
                last_at: output::moment(last_ms).map(output::utc).unwrap_or_default(),
                tracked,
            };
            (view, last_ms, harnesses.iter().map(|h| h.label()).collect())
        })
        .collect();
    // The most recent first: that is usually the folder to track.
    rows.sort_by_key(|row| std::cmp::Reverse(row.1));
    if app.json {
        let views: Vec<&FolderView> = rows.iter().map(|(view, _, _)| view).collect();
        return output::json(app.out, &views);
    }
    if rows.is_empty() {
        writeln!(
            app.out,
            "No sessions found. tc reads the session files of Claude Code and Codex."
        )?;
        return Ok(());
    }
    let mut table = vec![vec![
        "folder".to_owned(),
        "agents".to_owned(),
        "sessions".to_owned(),
        "last active".to_owned(),
        "tracked".to_owned(),
    ]];
    for (view, last_ms, labels) in &rows {
        table.push(vec![
            view.folder
                .clone()
                .unwrap_or_else(|| "(no folder)".to_owned()),
            labels.join(", "),
            view.sessions.to_string(),
            output::moment(*last_ms)
                .map(|at| at.date().to_string())
                .unwrap_or_default(),
            if view.tracked { "yes" } else { "no" }.to_owned(),
        ]);
    }
    output::table(app.out, &table, &[2])?;
    if scope.is_everything() {
        writeln!(
            app.out,
            "No folder is tracked, so every folder counts. tc track DIR limits it."
        )?;
    }
    Ok(())
}
