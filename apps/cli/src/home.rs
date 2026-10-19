//! Where `tc` keeps its files, and its settings in `config.toml`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// The folder with `tc.sqlite` and `config.toml`.
#[derive(Debug, Clone)]
pub struct Home {
    pub dir: PathBuf,
}

impl Home {
    /// `TC_HOME`, or the system's folder for application data.
    pub fn locate() -> Result<Self> {
        if let Some(dir) = std::env::var_os("TC_HOME").filter(|value| !value.is_empty()) {
            return Ok(Self {
                dir: PathBuf::from(dir),
            });
        }
        data_dir()
            .map(|dir| Self {
                dir: dir.join("tokencontroller"),
            })
            .ok_or_else(|| Error::usage("cannot find a folder for tc's data; set TC_HOME to one"))
    }

    pub fn database(&self) -> PathBuf {
        self.dir.join("tc.sqlite")
    }

    pub fn config_path(&self) -> PathBuf {
        self.dir.join("config.toml")
    }

    pub fn read_config(&self) -> Result<Config> {
        let path = self.config_path();
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text)
                .map_err(|error| Error::usage(format!("{}: {error}", path.display()))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(error) => Err(Error::file(path, error)),
        }
    }

    /// Writes the settings. The file holds the sign-in token, so on Unix only
    /// its owner may read it.
    pub fn write_config(&self, config: &Config) -> Result<()> {
        std::fs::create_dir_all(&self.dir).map_err(|source| Error::file(&self.dir, source))?;
        let path = self.config_path();
        let text =
            toml::to_string_pretty(config).map_err(|error| Error::Data(error.to_string()))?;
        write_private(&path, &text).map_err(|source| Error::file(path, source))
    }
}

#[cfg(unix)]
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(text.as_bytes())
}

#[cfg(not(unix))]
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    std::fs::write(path, text)
}

#[cfg(target_os = "windows")]
fn data_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(target_os = "macos")]
fn data_dir() -> Option<PathBuf> {
    crate::sources::home_dir().map(|home| home.join("Library/Application Support"))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn data_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| crate::sources::home_dir().map(|home| home.join(".local/share")))
}

/// Settings in `config.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    /// The service `tc connect` connected to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    /// The sign-in token `tc connect` received.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// A feature key pattern for working with no service connected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_key_pattern: Option<String>,
    /// The person's own price table: a CSV file with the columns provider,
    /// model, kind, `price_per_million` and `valid_from`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_table: Option<String>,
    /// Sessions that started before this day are not read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    /// The folders whose sessions count. Empty: every folder.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub folders: Vec<String>,
    /// The person chose every folder, so tc does not ask which to track.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub all_folders: bool,
}

/// The settings `tc config` can show and change, and what each means.
pub const SETTINGS: [(&str, &str); 5] = [
    (
        "feature-key-pattern",
        "A feature key pattern, such as ^(KD|OPS)-\\d+, for use with no service connected.",
    ),
    (
        "price-table",
        "Your own price table: a CSV file with provider, model, kind, price_per_million, valid_from. Connected, the service's table is used.",
    ),
    (
        "since",
        "A day, such as 2026-09-01. Sessions that started earlier are not read.",
    ),
    (
        "folders",
        "The folders whose sessions count; all for every folder. Change it with tc track and tc untrack.",
    ),
    (
        "service",
        "The service tc is connected to. Set by tc connect.",
    ),
];

impl Config {
    pub fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(match key {
            "feature-key-pattern" => self.feature_key_pattern.clone(),
            "price-table" => self.price_table.clone(),
            "since" => self.since.clone(),
            "folders" => self.folders_text(),
            "service" => self.service.clone(),
            _ => return Err(unknown_setting(key)),
        })
    }

    /// Changes a setting. An empty value removes it.
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        let value = Some(value.trim().to_owned()).filter(|value| !value.is_empty());
        match key {
            "feature-key-pattern" => {
                if let Some(pattern) = &value
                    && crate::keys::KeyPattern::new(pattern).is_none()
                {
                    return Err(Error::usage(format!(
                        "{pattern} is not a pattern tc can use; write a regular expression such as ^(KD|OPS)-\\d+"
                    )));
                }
                self.feature_key_pattern = value;
            }
            "price-table" => {
                if let Some(path) = &value {
                    crate::log::read_price_file(Path::new(path.as_str()))?;
                }
                self.price_table = value;
            }
            "since" => {
                if let Some(day) = &value
                    && crate::output::parse_date(day).is_none()
                {
                    return Err(Error::usage(format!(
                        "{day} is not a day; write it as 2026-09-01"
                    )));
                }
                self.since = value;
            }
            "folders" => match value.as_deref() {
                None => {
                    self.folders.clear();
                    self.all_folders = false;
                }
                Some("all") => {
                    self.folders.clear();
                    self.all_folders = true;
                }
                Some(_) => {
                    return Err(Error::usage(
                        "folders takes all or an empty value; add and remove folders with tc track DIR and tc untrack DIR",
                    ));
                }
            },
            "service" => {
                return Err(Error::usage(
                    "the service is set by tc connect; run tc connect URL",
                ));
            }
            _ => return Err(unknown_setting(key)),
        }
        Ok(())
    }

    /// The tracked folders as `tc config` shows them: `all` when every
    /// folder counts by choice, nothing when no choice was made yet.
    fn folders_text(&self) -> Option<String> {
        if !self.folders.is_empty() {
            Some(self.folders.join(", "))
        } else if self.all_folders {
            Some("all".to_owned())
        } else {
            None
        }
    }

    /// Every setting and its value, for `tc config` with no key.
    pub fn all(&self) -> BTreeMap<&'static str, Option<String>> {
        SETTINGS
            .iter()
            .map(|(key, _)| (*key, self.get(key).unwrap_or_default()))
            .collect()
    }
}

fn unknown_setting(key: &str) -> Error {
    let known: Vec<&str> = SETTINGS.iter().map(|(name, _)| *name).collect();
    Error::usage(format!(
        "there is no setting {key}; the settings are {}",
        known.join(", ")
    ))
}

/// Expands a leading `~` to the home folder, as people write paths such as
/// `~/work/storefront`.
pub fn expand_tilde(path: &str) -> PathBuf {
    match path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        Some(rest) => {
            crate::sources::home_dir().map_or_else(|| PathBuf::from(path), |home| home.join(rest))
        }
        None if path == "~" => crate::sources::home_dir().unwrap_or_else(|| PathBuf::from(path)),
        None => PathBuf::from(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_checked_before_they_are_kept() {
        let mut config = Config::default();
        config.set("feature-key-pattern", r"^(KD|OPS)-\d+").unwrap();
        config.set("since", "2026-09-01").unwrap();
        assert_eq!(config.get("since").unwrap().as_deref(), Some("2026-09-01"));
        assert!(config.set("since", "last week").is_err());
        assert!(config.set("feature-key-pattern", "(").is_err());
        assert!(config.set("price-table", "/no/such/file.csv").is_err());
        assert!(config.set("service", "https://x").is_err());
        assert!(config.set("colour", "blue").is_err());
        assert!(config.get("colour").is_err());
        config.set("feature-key-pattern", " ").unwrap();
        assert_eq!(config.feature_key_pattern, None);
        let all = config.all();
        assert_eq!(all.len(), 5);
        assert_eq!(all.get("folders"), Some(&None));
        config.set("folders", "all").unwrap();
        assert_eq!(config.get("folders").unwrap().as_deref(), Some("all"));
        assert!(config.all_folders);
        assert!(config.set("folders", "/work").is_err());
        config.folders = vec!["/a".into(), "/b".into()];
        assert_eq!(config.get("folders").unwrap().as_deref(), Some("/a, /b"));
        config.set("folders", "").unwrap();
        assert!(config.folders.is_empty());
        assert!(!config.all_folders);
        assert_eq!(all.get("price-table"), Some(&None));
    }

    #[test]
    fn a_price_file_is_read_before_it_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("prices.csv");
        std::fs::write(
            &file,
            "provider,model,kind,price_per_million,valid_from\nanthropic,claude-sonnet-4-5,input,3,2026-01-01\n",
        )
        .unwrap();
        let mut config = Config::default();
        config
            .set("price-table", &file.display().to_string())
            .unwrap();
        assert!(config.price_table.is_some());
        std::fs::write(&file, "provider,model\nanthropic,x\n").unwrap();
        assert!(
            config
                .set("price-table", &file.display().to_string())
                .is_err()
        );
    }

    #[test]
    fn the_config_file_round_trips_and_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let home = Home {
            dir: dir.path().join("tc"),
        };
        assert_eq!(home.read_config().unwrap(), Config::default());
        let config = Config {
            service: Some("https://app.tokencontroller.com".into()),
            token: Some("tc_cli_x".into()),
            ..Config::default()
        };
        home.write_config(&config).unwrap();
        assert_eq!(home.read_config().unwrap(), config);
        let text = std::fs::read_to_string(home.config_path()).unwrap();
        assert!(text.contains("service = \"https://app.tokencontroller.com\""));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(home.config_path())
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::write(home.config_path(), "since = [").unwrap();
        assert!(home.read_config().is_err());
        assert!(home.database().ends_with("tc.sqlite"));
    }

    #[test]
    fn a_leading_tilde_is_the_home_folder() {
        assert_eq!(expand_tilde("/bin/x"), PathBuf::from("/bin/x"));
        if let Some(home) = crate::sources::home_dir() {
            assert_eq!(expand_tilde("~/bin/x"), home.join("bin/x"));
            assert_eq!(expand_tilde("~"), home);
        }
    }

    #[test]
    fn the_token_is_never_a_setting() {
        let mut config = Config {
            service: Some("https://tc.example.com".into()),
            token: Some("tc_cli_secret".into()),
            ..Config::default()
        };
        assert_eq!(
            config.get("service").unwrap().as_deref(),
            Some("https://tc.example.com")
        );
        assert!(config.get("token").is_err());
        assert!(config.set("token", "x").is_err());
        let names: Vec<&str> = config.all().keys().copied().collect();
        assert_eq!(
            names,
            [
                "feature-key-pattern",
                "folders",
                "price-table",
                "service",
                "since"
            ]
        );
        assert!(
            !config
                .all()
                .values()
                .any(|value| value.as_deref() == Some("tc_cli_secret"))
        );
    }
}
