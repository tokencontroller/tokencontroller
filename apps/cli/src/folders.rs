//! Which sessions count, by the folder they ran in: `tc track`, `tc untrack`
//! and the question on the first interactive run.
//!
//! With no folder tracked every session on the machine counts, as before
//! folders existed. Once one is, a session counts when it ran in a tracked
//! folder or below it, or when `tc record` was told about it: the Claude Code
//! mod knows its own session wherever it runs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::home::{Config, expand_tilde};

/// The tracked folders, checked against the folder a session ran in.
pub struct Scope {
    folders: Vec<PathBuf>,
    /// Session folders already resolved through links, such as `/tmp` to
    /// `/private/tmp` on macOS.
    resolved: HashMap<String, Option<PathBuf>>,
}

impl Scope {
    pub fn new(folders: &[String]) -> Self {
        Self {
            folders: folders.iter().map(PathBuf::from).collect(),
            resolved: HashMap::new(),
        }
    }

    /// Whether every folder counts.
    pub fn is_everything(&self) -> bool {
        self.folders.is_empty()
    }

    /// Whether a session that ran in `directory` counts. A session with no
    /// known folder counts only when every folder does.
    pub fn includes(&mut self, directory: Option<&str>) -> bool {
        if self.folders.is_empty() {
            return true;
        }
        let Some(directory) = directory else {
            return false;
        };
        if self.covers(Path::new(directory)) {
            return true;
        }
        let real = self
            .resolved
            .entry(directory.to_owned())
            .or_insert_with_key(|directory| std::fs::canonicalize(directory).ok().map(plain))
            .clone();
        real.is_some_and(|real| self.covers(&real))
    }

    /// The folder itself or one below it, compared by whole path parts:
    /// `/work/shop` covers `/work/shop/api` but not `/work/shop-old`.
    fn covers(&self, path: &Path) -> bool {
        self.folders.iter().any(|folder| path.starts_with(folder))
    }
}

/// A path without Windows' `\\?\` prefix, which canonicalizing adds and
/// session files never have.
pub fn plain(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    path
}

/// An existing folder as an absolute path with links resolved.
pub fn resolve(dir: &str) -> Result<PathBuf> {
    let path = expand_tilde(dir);
    match std::fs::canonicalize(&path) {
        Ok(real) if real.is_dir() => Ok(plain(real)),
        Ok(_) => Err(Error::usage(format!("{dir} is not a folder"))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(Error::usage(format!(
            "{dir} does not exist; tc list shows the folders with sessions"
        ))),
        Err(error) => Err(Error::file(path, error)),
    }
}

/// The folder this command runs in.
pub fn current() -> Result<PathBuf> {
    resolve(".")
}

/// Adds a folder. Returns false when it was tracked already.
pub fn track(config: &mut Config, folder: &Path) -> bool {
    config.all_folders = false;
    if config
        .folders
        .iter()
        .any(|tracked| Path::new(tracked) == folder)
    {
        return false;
    }
    config.folders.push(folder.display().to_string());
    true
}

/// Removes a folder, given as typed or resolved. Returns false when it was
/// not tracked. Once the last one goes, every folder counts again and tc
/// does not ask.
pub fn untrack(config: &mut Config, dir: &str) -> bool {
    let typed = expand_tilde(dir);
    let typed = if typed.is_absolute() {
        typed
    } else {
        std::env::current_dir().map_or(typed.clone(), |here| here.join(&typed))
    };
    let real = resolve(dir).ok();
    let before = config.folders.len();
    config.folders.retain(|tracked| {
        let tracked = Path::new(tracked);
        tracked != typed && real.as_deref() != Some(tracked)
    });
    let removed = config.folders.len() != before;
    if removed && config.folders.is_empty() {
        config.all_folders = true;
    }
    removed
}

/// Whether to ask which folder to track: only a person at a terminal, and
/// only while nothing was chosen.
pub fn should_ask(config: &Config, interactive: bool) -> bool {
    interactive && config.folders.is_empty() && !config.all_folders
}

/// The answer to `[Y/n]`: yes by default, `None` for anything else, which
/// leaves the question for next time.
pub fn answer(line: &str) -> Option<bool> {
    match line.trim().to_ascii_lowercase().as_str() {
        "" | "y" | "yes" => Some(true),
        "n" | "no" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_covers_itself_and_below_but_not_its_siblings() {
        let mut scope = Scope::new(&["/work/shop".into()]);
        assert!(scope.includes(Some("/work/shop")));
        assert!(scope.includes(Some("/work/shop/")));
        assert!(scope.includes(Some("/work/shop/api")));
        assert!(!scope.includes(Some("/work/shop-old")));
        assert!(!scope.includes(Some("/work")));
        assert!(!scope.includes(None));
        assert!(!scope.is_everything());

        let mut everything = Scope::new(&[]);
        assert!(everything.is_everything());
        assert!(everything.includes(Some("/anywhere")));
        assert!(everything.includes(None));
    }

    #[test]
    fn a_session_folder_is_compared_with_links_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir_all(real.join("api")).unwrap();
        let tracked = resolve(&real.display().to_string()).unwrap();
        let mut scope = Scope::new(&[tracked.display().to_string()]);
        // A path with a `..` in it resolves into the tracked folder.
        let roundabout = real.join("api").join("..").join("api");
        assert!(scope.includes(Some(&roundabout.display().to_string())));
        assert!(!scope.includes(Some(&dir.path().join("gone").display().to_string())));
    }

    #[test]
    fn only_existing_folders_resolve() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file.txt");
        std::fs::write(&file, "x").unwrap();
        assert!(resolve(&dir.path().display().to_string()).is_ok());
        let missing = resolve(&dir.path().join("nope").display().to_string());
        assert_eq!(missing.unwrap_err().exit_code(), 1);
        let not_a_folder = resolve(&file.display().to_string());
        assert_eq!(not_a_folder.unwrap_err().exit_code(), 1);
        assert!(current().unwrap().is_absolute());
    }

    #[test]
    fn windows_prefixes_are_dropped() {
        assert_eq!(
            plain(PathBuf::from(r"\\?\C:\work\shop")),
            PathBuf::from(r"C:\work\shop")
        );
        assert_eq!(
            plain(PathBuf::from(r"\\?\UNC\server\share")),
            PathBuf::from(r"\\server\share")
        );
        assert_eq!(plain(PathBuf::from("/work")), PathBuf::from("/work"));
    }

    #[test]
    fn folders_are_tracked_once_and_untracking_the_last_means_every_folder() {
        let dir = tempfile::tempdir().unwrap();
        let folder = resolve(&dir.path().display().to_string()).unwrap();
        let mut config = Config {
            all_folders: true,
            ..Config::default()
        };
        assert!(track(&mut config, &folder));
        assert!(!config.all_folders);
        assert!(!track(&mut config, &folder));
        assert_eq!(config.folders.len(), 1);
        assert!(!untrack(&mut config, "/no/such/folder"));
        assert!(untrack(&mut config, &folder.display().to_string()));
        assert!(config.folders.is_empty());
        assert!(config.all_folders);
        assert!(!untrack(&mut config, &folder.display().to_string()));
    }

    #[test]
    fn the_question_is_asked_once_and_only_at_a_terminal() {
        let mut config = Config::default();
        assert!(should_ask(&config, true));
        assert!(!should_ask(&config, false));
        config.all_folders = true;
        assert!(!should_ask(&config, true));
        config.all_folders = false;
        config.folders.push("/work".into());
        assert!(!should_ask(&config, true));

        assert_eq!(answer(""), Some(true));
        assert_eq!(answer(" Y\n"), Some(true));
        assert_eq!(answer("yes"), Some(true));
        assert_eq!(answer("n"), Some(false));
        assert_eq!(answer("No\n"), Some(false));
        assert_eq!(answer("maybe"), None);
    }
}
