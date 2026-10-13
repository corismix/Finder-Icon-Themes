//! Configuration file: watch roots, theme and what to iconise.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::state::home;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    /// Roots the background service watches for repositories.
    #[serde(default)]
    pub watch_roots: Vec<PathBuf>,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_true")]
    pub files: bool,
    #[serde(default = "default_true")]
    pub folders: bool,
}

fn default_theme() -> String {
    "material".to_string()
}

fn default_true() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Config {
            watch_roots: Vec::new(),
            theme: default_theme(),
            files: true,
            folders: true,
        }
    }
}

pub fn config_path() -> PathBuf {
    std::env::var_os("FINDER_ICON_THEMES_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config/finder-icon-themes/config.toml"))
}

pub fn load() -> Result<Config> {
    let path = config_path();
    if !path.is_file() {
        return Ok(Config::default());
    }
    let text =
        fs::read_to_string(&path).with_context(|| format!("Cannot read {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("Cannot parse {}", path.display()))
}

/// Write a starter config if none exists yet.
pub fn write_template_if_missing() -> Result<PathBuf> {
    let path = config_path();
    if path.is_file() {
        return Ok(path);
    }
    let config = Config {
        watch_roots: vec![home().join("Code")],
        ..Config::default()
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Cannot create {}", parent.display()))?;
    }
    fs::write(&path, toml::to_string_pretty(&config)?)
        .with_context(|| format!("Cannot write {}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let config = Config::default();
        assert_eq!(config.theme, "material");
        assert!(config.files && config.folders);
    }

    #[test]
    fn round_trips_through_toml() {
        let config = Config {
            watch_roots: vec![PathBuf::from("/tmp/x")],
            ..Config::default()
        };
        let text = toml::to_string_pretty(&config).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.watch_roots, vec![PathBuf::from("/tmp/x")]);
    }
}
