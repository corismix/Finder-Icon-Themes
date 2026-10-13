//! Applying, previewing and undoing icons. Shared by the CLI and the daemon.

use anyhow::{bail, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::macos;
use crate::render::png_for;
use crate::scanner::{collect_candidates, Candidate};
use crate::state::{identity, is_within, save_state, state_paths, Entry, State};
use crate::theme::Theme;

/// Write the journal once per this many icons instead of once per icon. The
/// journal-before-icons ordering is preserved per batch, so an interrupted run
/// is still fully undoable (at worst the current batch).
const BATCH: usize = 256;

#[derive(Default, Debug)]
pub struct ApplyReport {
    pub candidates: usize,
    pub already: usize,
    pub applied: usize,
    pub unchanged: usize,
    pub custom_skipped: usize,
    pub stale_skipped: usize,
    pub failed: usize,
}

#[derive(Default, Debug)]
pub struct UndoReport {
    pub eligible: usize,
    pub stale: usize,
    pub removed: usize,
    pub failed: usize,
}

pub fn candidates_for(root: &Path, theme: &Theme, config: &Config) -> Result<Vec<Candidate>> {
    collect_candidates(root, &theme.manifest, config.files, config.folders)
}

/// Candidates that are new or whose recorded icon no longer matches.
pub fn affected<'a>(candidates: &'a [Candidate], state: &State) -> Vec<&'a Candidate> {
    candidates
        .iter()
        .filter(|item| {
            let Some(record) = state.entries.get(&item.path.to_string_lossy().into_owned()) else {
                return true;
            };
            match identity(&item.path) {
                Ok(current) => record.identity != current || record.id != item.id,
                Err(_) => true,
            }
        })
        .collect()
}

pub fn apply(
    root: &Path,
    theme: &Theme,
    config: &Config,
    cache_dir: &Path,
    state: &mut State,
) -> Result<ApplyReport> {
    let candidates = candidates_for(root, theme, config)?;
    let mut report = ApplyReport {
        candidates: candidates.len(),
        ..Default::default()
    };
    report.already = candidates.len() - affected(&candidates, state).len();

    // Inspect first, so a manually customised item is never clobbered.
    let mut existing: HashMap<PathBuf, bool> = HashMap::new();
    for item in &candidates {
        existing.insert(item.path.clone(), macos::has_custom_icon(&item.path));
    }

    let mut todo: Vec<Candidate> = Vec::new();
    for item in candidates {
        let key = item.path.to_string_lossy().into_owned();
        let prev = state.entries.get(&key).cloned();
        if let Some(prev) = &prev {
            if prev.identity != identity(&item.path)? {
                report.stale_skipped += 1;
                continue;
            }
        }
        let custom = existing.get(&item.path).copied().unwrap_or(false);
        if custom && prev.is_none() {
            report.custom_skipped += 1;
            continue;
        }
        if custom && prev.as_ref().map(|p| p.id == item.id).unwrap_or(false) {
            report.unchanged += 1;
            continue;
        }
        todo.push(item);
    }

    if !todo.is_empty() {
        crate::git::ensure_global_rule(state)?;
    }

    let mut png_cache: HashMap<String, PathBuf> = HashMap::new();
    for batch in todo.chunks(BATCH) {
        // Record the batch, persist once, then change the icons. The journal is
        // always ahead of the filesystem, so an interruption stays undoable.
        let mut pending: Vec<(String, Option<Entry>, PathBuf, PathBuf)> = Vec::new();
        for item in batch {
            let png = match png_cache.get(&item.source) {
                Some(path) => path.clone(),
                None => {
                    let path = png_for(theme, &item.source, cache_dir)?;
                    png_cache.insert(item.source.clone(), path.clone());
                    path
                }
            };
            let key = item.path.to_string_lossy().into_owned();
            let previous = state.entries.get(&key).cloned();
            state.entries.insert(
                key.clone(),
                Entry {
                    identity: identity(&item.path)?,
                    id: item.id.clone(),
                    directory: item.directory,
                },
            );
            pending.push((key, previous, png, item.path.clone()));
        }
        save_state(&state_paths().state, state)?;
        for (key, previous, png, path) in pending {
            match macos::set_icon(&path, Some(&png)) {
                Ok(true) => report.applied += 1,
                Ok(false) | Err(_) => {
                    report.failed += 1;
                    match previous {
                        Some(prev) => state.entries.insert(key, prev),
                        None => state.entries.remove(&key),
                    };
                }
            }
        }
    }
    save_state(&state_paths().state, state)?;
    Ok(report)
}

pub fn undo(root: &Path, state: &mut State) -> Result<UndoReport> {
    let mut report = UndoReport::default();
    let tracked: Vec<String> = state
        .entries
        .keys()
        .filter(|p| is_within(root, Path::new(p)))
        .cloned()
        .collect();

    let mut eligible = Vec::new();
    for key in tracked {
        let record = state.entries.get(&key).cloned();
        let stale = match (record, identity(Path::new(&key))) {
            (Some(record), Ok(current)) => record.identity != current,
            _ => true,
        };
        if stale {
            report.stale += 1;
            state.entries.remove(&key);
        } else {
            eligible.push(key);
        }
    }
    report.eligible = eligible.len();
    save_state(&state_paths().state, state)?;

    for key in eligible {
        match macos::set_icon(Path::new(&key), None) {
            Ok(true) => {
                report.removed += 1;
                state.entries.remove(&key);
            }
            _ => report.failed += 1,
        }
        save_state(&state_paths().state, state)?;
    }

    if state.entries.is_empty() {
        crate::git::remove_global_rule(state)?;
        save_state(&state_paths().state, state)?;
    }
    Ok(report)
}

/// Forget journal entries whose paths no longer exist, and drop the global rule
/// when nothing is managed any more.
pub fn prune(state: &mut State, within: Option<&Path>) -> Result<()> {
    let keys: Vec<String> = state
        .entries
        .keys()
        .filter(|p| {
            within
                .map(|root| is_within(root, Path::new(p)))
                .unwrap_or(true)
        })
        .filter(|p| !Path::new(p).exists())
        .cloned()
        .collect();
    for key in keys {
        state.entries.remove(&key);
    }
    if state.entries.is_empty() {
        crate::git::remove_global_rule(state)?;
    }
    save_state(&state_paths().state, state)?;
    Ok(())
}

pub fn status(root: &Path, state: &State) -> Vec<(String, bool, String)> {
    state
        .entries
        .iter()
        .filter(|(p, _)| is_within(root, Path::new(p)))
        .map(|(p, e)| {
            let relative = Path::new(p)
                .strip_prefix(root)
                .unwrap_or(Path::new(p))
                .to_string_lossy()
                .into_owned();
            (relative, e.directory, e.id.clone())
        })
        .collect()
}

pub fn clean(state: &mut State) -> Result<usize> {
    let keys: Vec<String> = state.entries.keys().cloned().collect();
    let mut removed = 0;
    for key in keys {
        if macos::set_icon(Path::new(&key), None).unwrap_or(false) {
            removed += 1;
        }
        state.entries.remove(&key);
    }
    crate::git::remove_global_rule(state)?;
    state.entries.clear();
    save_state(&state_paths().state, state)?;
    Ok(removed)
}

/// Guard against a `--files-only`/`--folders-only` combination that would do nothing.
pub fn ensure_something_to_do(config: &Config) -> Result<()> {
    if !config.files && !config.folders {
        bail!("Nothing to do with both files and folders disabled.");
    }
    Ok(())
}
