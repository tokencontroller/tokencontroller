//! Facts about the git repository a session ran in: its name, as the
//! repository mapping in the web app writes it (`kestrel/storefront`), and the
//! tags in its `.tokencontroller.toml`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The repository config file at a repository's root.
pub const CONFIG_FILE: &str = ".tokencontroller.toml";

/// The root of the git repository that holds `directory`: the nearest folder
/// upwards with a `.git` folder or file.
pub fn root_of(directory: &Path) -> Option<PathBuf> {
    directory
        .ancestors()
        .find(|folder| folder.join(".git").exists())
        .map(Path::to_path_buf)
}

/// The repository name of a working directory, from the `origin` remote in
/// its git config.
pub fn name_of(directory: &Path) -> Option<String> {
    let root = root_of(directory)?;
    let config = git_dir(&root)?.join("config");
    let text = std::fs::read_to_string(config).ok()?;
    origin_url(&text).and_then(|url| name_from_url(&url))
}

/// The tags in the repository's `.tokencontroller.toml`, if it has one.
pub fn config_tags(directory: &Path) -> BTreeMap<String, String> {
    #[derive(Deserialize)]
    struct ConfigFile {
        #[serde(default)]
        tags: BTreeMap<String, toml::Value>,
    }
    let Some(root) = root_of(directory) else {
        return BTreeMap::new();
    };
    let Ok(text) = std::fs::read_to_string(root.join(CONFIG_FILE)) else {
        return BTreeMap::new();
    };
    let Ok(file) = toml::from_str::<ConfigFile>(&text) else {
        return BTreeMap::new();
    };
    file.tags
        .into_iter()
        .filter_map(|(name, value)| match value {
            toml::Value::String(text) => Some((name, text)),
            toml::Value::Integer(number) => Some((name, number.to_string())),
            _ => None,
        })
        .collect()
}

/// `.git` is a folder in a normal clone and a file pointing elsewhere in a
/// worktree or submodule.
fn git_dir(root: &Path) -> Option<PathBuf> {
    let dot_git = root.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    let text = std::fs::read_to_string(&dot_git).ok()?;
    let target = text.trim().strip_prefix("gitdir:")?.trim();
    let target = root.join(target);
    // A worktree's own git dir has no config; the main one is two levels up.
    if target.join("config").exists() {
        Some(target)
    } else {
        target.ancestors().nth(2).map(Path::to_path_buf)
    }
}

/// The URL of the `origin` remote in a git config file.
fn origin_url(config: &str) -> Option<String> {
    let mut in_origin = false;
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_origin = line.replace(' ', "") == "[remote\"origin\"]";
            continue;
        }
        if in_origin
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "url"
        {
            return Some(value.trim().to_owned());
        }
    }
    None
}

/// The repository name in a remote URL: its last two path parts, such as
/// `kestrel/storefront` for `git@github.com:kestrel/storefront.git`. Azure
/// DevOps' `_git` part is skipped.
pub fn name_from_url(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches('/');
    let url = url.strip_suffix(".git").unwrap_or(url);
    let path = match url.split_once("://") {
        Some((_, rest)) => rest.split_once('/').map(|(_, path)| path)?,
        // scp-like: git@host:owner/repo
        None => url.split_once(':').map_or(url, |(_, path)| path),
    };
    let parts: Vec<&str> = path
        .split('/')
        .filter(|part| !part.is_empty() && *part != "_git")
        .collect();
    match parts.as_slice() {
        [.., owner, repository] => Some(format!("{owner}/{repository}")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_come_from_every_kind_of_remote_url() {
        let cases = [
            (
                "git@github.com:kestrel/storefront.git",
                "kestrel/storefront",
            ),
            (
                "https://github.com/kestrel/storefront",
                "kestrel/storefront",
            ),
            (
                "https://github.com/kestrel/storefront.git/",
                "kestrel/storefront",
            ),
            (
                "ssh://git@github.com/kestrel/storefront.git",
                "kestrel/storefront",
            ),
            (
                "https://dev.azure.com/kestrel/web/_git/storefront",
                "web/storefront",
            ),
            ("git@gitlab.com:group/sub/app.git", "sub/app"),
        ];
        for (url, name) in cases {
            assert_eq!(name_from_url(url).as_deref(), Some(name), "{url}");
        }
        assert_eq!(name_from_url("https://github.com/"), None);
        assert_eq!(name_from_url("lonely"), None);
    }

    #[test]
    fn the_origin_remote_is_found_in_a_git_config() {
        let config = "[core]\n\turl = nope\n[remote \"upstream\"]\n\turl = git@x:a/b\n[remote \"origin\"]\n\tfetch = +refs\n\turl = git@github.com:kestrel/storefront.git\n";
        assert_eq!(
            origin_url(config).as_deref(),
            Some("git@github.com:kestrel/storefront.git")
        );
        assert_eq!(origin_url("[core]\n"), None);
    }

    #[test]
    fn a_working_directory_knows_its_repository_and_tags() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("storefront");
        let deep = root.join("src/app");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(
            root.join(".git/config"),
            "[remote \"origin\"]\n\turl = https://github.com/kestrel/storefront.git\n",
        )
        .unwrap();
        std::fs::write(
            root.join(CONFIG_FILE),
            "[tags]\nproject = \"storefront\"\npurpose = \"product\"\nyear = 2026\nnested = { a = 1 }\n",
        )
        .unwrap();
        assert_eq!(root_of(&deep).as_deref(), Some(root.as_path()));
        assert_eq!(name_of(&deep).as_deref(), Some("kestrel/storefront"));
        let tags = config_tags(&deep);
        assert_eq!(tags.get("project").map(String::as_str), Some("storefront"));
        assert_eq!(tags.get("year").map(String::as_str), Some("2026"));
        assert_eq!(tags.len(), 3);
        assert!(config_tags(dir.path()).is_empty());
    }

    #[test]
    fn a_worktree_finds_the_main_git_config() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main");
        let worktree_git = main.join(".git/worktrees/feature");
        std::fs::create_dir_all(&worktree_git).unwrap();
        std::fs::write(
            main.join(".git/config"),
            "[remote \"origin\"]\n\turl = git@github.com:kestrel/storefront.git\n",
        )
        .unwrap();
        let worktree = dir.path().join("feature");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", worktree_git.display()),
        )
        .unwrap();
        assert_eq!(name_of(&worktree).as_deref(), Some("kestrel/storefront"));
    }
}
