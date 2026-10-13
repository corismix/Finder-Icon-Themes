//! Undo journal: every icon this tool owns, plus the global Git exclude rule.
//!
//! The journal is rewritten often (once per applied batch), so it uses short,
//! minified keys. Schema 3.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub const SCHEMA: u32 = 3;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Entry {
    /// dev:ino:birthtime — detects a path replaced since we iconised it.
    #[serde(rename = "i")]
    pub identity: String,
    /// Theme icon id, e.g. `typescript`.
    #[serde(rename = "d")]
    pub id: String,
    /// Whether the entry is a folder.
    #[serde(rename = "dir")]
    pub directory: bool,
}

/// The single global Git exclude rule we may own. We never touch a repo's
/// `.git/info/exclude` or its tracked `.gitignore`.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Excludes {
    #[serde(rename = "gf", default)]
    pub global_file: Option<PathBuf>,
    #[serde(rename = "go", default)]
    pub global_owned: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct State {
    pub schema: u32,
    #[serde(default)]
    pub entries: BTreeMap<String, Entry>,
    #[serde(default)]
    pub excludes: Excludes,
}

impl Default for State {
    fn default() -> Self {
        State {
            schema: SCHEMA,
            entries: BTreeMap::new(),
            excludes: Excludes::default(),
        }
    }
}

pub struct Paths {
    pub base: PathBuf,
    pub state: PathBuf,
    pub cache: PathBuf,
}

pub fn state_paths() -> Paths {
    let base = std::env::var_os("FINDER_ICON_THEMES_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/state/finder-icon-themes"));
    Paths {
        state: base.join("state.json"),
        cache: base.join("cache"),
        base,
    }
}

pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Read the journal. An unreadable or foreign journal (for example a schema 1
/// file from the earlier Node tool) is preserved alongside and replaced with a
/// fresh one, rather than aborting the command — a background service must never
/// crash-loop on it.
pub fn read_state(file: &Path) -> Result<State> {
    let text = match fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(State::default()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Cannot read icon journal at {}", file.display()))
        }
    };
    match serde_json::from_str::<State>(&text) {
        Ok(state) if state.schema == SCHEMA => Ok(state),
        parsed => {
            let reason = match parsed {
                Ok(state) => format!("schema {}", state.schema),
                Err(error) => error.to_string(),
            };
            let backup = backup_path(file);
            fs::rename(file, &backup).with_context(|| {
                format!(
                    "Journal at {} is unusable ({reason}) and could not be set aside",
                    file.display()
                )
            })?;
            eprintln!(
                "finder-icon-themes: ignoring unusable journal at {} ({reason}); preserved as {}",
                file.display(),
                backup.display()
            );
            Ok(State::default())
        }
    }
}

fn backup_path(file: &Path) -> PathBuf {
    let candidate = suffixed(file, ".bak");
    if !candidate.exists() {
        return candidate;
    }
    for n in 2.. {
        let candidate = suffixed(file, &format!(".bak{n}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

fn suffixed(file: &Path, suffix: &str) -> PathBuf {
    let mut name = file.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    file.with_file_name(name)
}

pub fn save_state(file: &Path, state: &State) -> Result<()> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Cannot create {}", parent.display()))?;
    }
    let temp = file.with_extension(format!("{}.tmp", std::process::id()));
    let text = serde_json::to_string(state)?;
    fs::write(&temp, text).with_context(|| format!("Cannot write {}", temp.display()))?;
    fs::rename(&temp, file).with_context(|| format!("Cannot replace {}", file.display()))?;
    Ok(())
}

fn identity_of(meta: &fs::Metadata) -> String {
    let ms = meta
        .created()
        .or_else(|_| meta.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{}:{}:{}", meta.dev(), meta.ino(), ms)
}

/// Stable-ish identity used to detect a path that was replaced since we iconised it.
pub fn identity(path: &Path) -> Result<String> {
    let meta =
        fs::symlink_metadata(path).with_context(|| format!("Cannot stat {}", path.display()))?;
    Ok(identity_of(&meta))
}

pub fn is_within(root: &Path, item: &Path) -> bool {
    item == root || item.starts_with(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_journal_is_an_empty_state() {
        let dir = std::env::temp_dir().join(format!("fit-state-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let state = read_state(&dir.join("nope.json")).unwrap();
        assert!(state.entries.is_empty());
        assert_eq!(state.schema, SCHEMA);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn state_round_trips_and_stays_compact() {
        let dir = std::env::temp_dir().join(format!("fit-state-rt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let file = dir.join("state.json");
        let mut state = State::default();
        state.entries.insert(
            "/tmp/some/path/entry.ts".into(),
            Entry {
                identity: "16777220:123456789:1700000000000".into(),
                id: "typescript".into(),
                directory: false,
            },
        );
        save_state(&file, &state).unwrap();
        let text = fs::read_to_string(&file).unwrap();
        assert!(!text.contains('\n'), "journal should be minified");
        assert!(text.len() < 200, "compact entry, got {} bytes", text.len());
        let back = read_state(&file).unwrap();
        assert_eq!(back.entries["/tmp/some/path/entry.ts"].id, "typescript");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn foreign_journal_is_preserved_and_replaced() {
        let dir = std::env::temp_dir().join(format!("fit-state-foreign-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("state.json");
        let old = r#"{"schema":2,"entries":{"/tmp/old":{"identity":"1:2:3","id":"file","theme":"material","directory":false,"repo":null}},"excludes":{}}"#;
        fs::write(&file, old).unwrap();

        let state = read_state(&file).unwrap();
        assert!(
            state.entries.is_empty(),
            "a foreign journal must not be loaded"
        );
        assert!(!file.exists(), "the foreign journal is moved aside");
        assert_eq!(fs::read_to_string(dir.join("state.json.bak")).unwrap(), old);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn repeated_foreign_journals_do_not_clobber_backups() {
        let dir = std::env::temp_dir().join(format!("fit-state-multi-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("state.json");
        fs::write(&file, r#"{"schema":1,"entries":{},"excludes":{}}"#).unwrap();
        read_state(&file).unwrap();
        fs::write(&file, r#"{"schema":2,"entries":{},"excludes":{}}"#).unwrap();
        read_state(&file).unwrap();
        assert!(dir.join("state.json.bak").is_file(), "first backup kept");
        assert!(
            dir.join("state.json.bak2").is_file(),
            "second backup kept separately"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn within_is_boundary_aware() {
        assert!(is_within(Path::new("/a/b"), Path::new("/a/b/c")));
        assert!(is_within(Path::new("/a/b"), Path::new("/a/b")));
        assert!(!is_within(Path::new("/a/b"), Path::new("/a/bc")));
    }
}
