//! Undo journal: every icon this tool owns, plus the global Git exclude rule.
//!
//! The journal is rewritten often (once per applied batch), so it uses short,
//! minified keys. Schema 3.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub const SCHEMA: u32 = 3;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// dev:ino:birthtime — detects a path replaced since we iconised it.
    #[serde(rename = "i", alias = "identity")]
    pub identity: String,
    /// Theme icon id, e.g. `typescript`.
    #[serde(rename = "d", alias = "id")]
    pub id: String,
    /// Whether the entry is a folder.
    #[serde(rename = "dir", alias = "directory")]
    pub directory: bool,
    /// Content revision, independent of the matcher's ID.
    #[serde(rename = "r", default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// Resource-fork fingerprint; absent on legacy or interrupted intents.
    #[serde(rename = "p", default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
    #[serde(
        rename = "pending",
        default,
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub pending: bool,
}

/// The single global Git exclude rule we may own. We never touch a repo's
/// `.git/info/exclude` or its tracked `.gitignore`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Excludes {
    #[serde(rename = "gf", default)]
    pub global_file: Option<PathBuf>,
    #[serde(rename = "go", default)]
    pub global_owned: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
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

/// Read compatible journals without abandoning ownership. Unknown/corrupt data
/// fails closed: a background restart must never replace it with an empty state.
pub fn read_state(file: &Path) -> Result<State> {
    let text = match fs::read_to_string(file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(State::default()),
        Err(e) => return Err(e).context("Cannot read icon journal"),
    };
    let mut state: State =
        serde_json::from_str(&text).context("Cannot parse icon journal; left intact")?;
    anyhow::ensure!(
        (2..=SCHEMA).contains(&state.schema),
        "Unsupported journal schema {}; left intact",
        state.schema
    );
    state.schema = SCHEMA;
    Ok(state)
}

/// All mutation sessions acquire this lock BEFORE loading the journal. Keep
/// the inode in place; unlinking an advisory lock allows two independent locks.
pub struct MutationLock(fs::File);
impl MutationLock {
    pub fn acquire(base: &Path) -> Result<Self> {
        fs::create_dir_all(base)?;
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(base.join("journal.lock"))?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(std::io::Error::last_os_error()).context("Cannot lock journal");
        }
        Ok(Self(file))
    }
}
impl Drop for MutationLock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

pub fn save_state(file: &Path, state: &State) -> Result<()> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Cannot create {}", parent.display()))?;
    }
    let temp = file.with_extension(format!("{}.tmp", std::process::id()));
    let text = serde_json::to_string(state)?;
    let mut output = fs::File::create(&temp)?;
    output.write_all(text.as_bytes())?;
    output.sync_all()?;
    fs::rename(&temp, file).with_context(|| format!("Cannot replace {}", file.display()))?;
    if let Some(parent) = file.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

pub fn identity_of(meta: &fs::Metadata) -> String {
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
                revision: None,
                payload: None,
                pending: false,
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
    fn compatible_journal_preserves_ownership_and_unknown_fails_closed() {
        let dir = std::env::temp_dir().join(format!("fit-migrate-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("state.json");
        fs::write(&file, r#"{"schema":2,"entries":{"/tmp/old":{"identity":"1:2:3","id":"file","directory":false}},"excludes":{}}"#).unwrap();
        assert_eq!(
            read_state(&file).unwrap().entries["/tmp/old"].identity,
            "1:2:3"
        );
        let unknown = r#"{"schema":99,"entries":{},"excludes":{}}"#;
        fs::write(&file, unknown).unwrap();
        assert!(read_state(&file).is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), unknown);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn within_is_boundary_aware() {
        assert!(is_within(Path::new("/a/b"), Path::new("/a/b/c")));
        assert!(is_within(Path::new("/a/b"), Path::new("/a/b")));
        assert!(!is_within(Path::new("/a/b"), Path::new("/a/bc")));
    }
}
