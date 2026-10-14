//! Applying, previewing and undoing icons. Shared by the CLI and the daemon.

use anyhow::{bail, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::macos;
use crate::render::png_for;
use crate::scanner::{collect_with_config, Candidate};
use crate::state::{identity, save_state, scoped_entries, state_paths, Entry, State};
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
    collect_with_config(root, root, true, &theme.manifest, config)
}

/// Candidates that are new or whose recorded icon no longer matches.
pub fn affected<'a>(candidates: &'a [Candidate], state: &State) -> Vec<&'a Candidate> {
    candidates
        .iter()
        .filter(|item| {
            let Some(record) = state.entries.get(&item.path.to_string_lossy().into_owned()) else {
                return true;
            };
            record.identity != item.identity || record.id != item.id
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
    // Recover renames within a reconciliation scope before pruning old paths.
    let identities: HashMap<String, String> = scoped_entries(state, root)
        .map(|(p, e)| (e.identity.clone(), p.clone()))
        .collect();
    let mut moved = false;
    for item in &candidates {
        let current = item.identity.clone();
        if let Some(old) = identities.get(&current) {
            if old != &item.path.to_string_lossy() && !Path::new(old).exists() {
                moved |= transfer_move(Path::new(old), &item.path, state)?;
            }
        }
    }
    if moved {
        save_state(&state_paths().state, state)?;
    }
    apply_candidates(candidates, theme, cache_dir, state)
}

/// Transfer ownership by identity, including all descendants of a moved folder.
/// Never overwrite an unrelated destination record or follow symbolic links.
pub fn transfer_move(from: &Path, to: &Path, state: &mut State) -> Result<bool> {
    let keys: Vec<_> = scoped_entries(state, from)
        .map(|(p, _)| p.clone())
        .collect();
    let mut dirty = false;
    for key in keys {
        let suffix = Path::new(&key).strip_prefix(from)?;
        let destination = if suffix.as_os_str().is_empty() {
            to.to_path_buf()
        } else {
            to.join(suffix)
        };
        if destination
            .symlink_metadata()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(true)
        {
            continue;
        }
        let Some(record) = state.entries.get(&key) else {
            continue;
        };
        if identity(&destination).ok().as_ref() != Some(&record.identity) {
            continue;
        }
        let target = destination.to_string_lossy().into_owned();
        if state.entries.contains_key(&target) {
            continue;
        }
        let mut record = state.remove(&key).unwrap();
        record.missing_since = None;
        state.insert(target, record);
        dirty = true;
    }
    Ok(dirty)
}

pub fn apply_candidates(
    candidates: Vec<Candidate>,
    theme: &Theme,
    cache_dir: &Path,
    state: &mut State,
) -> Result<ApplyReport> {
    let mut report = ApplyReport {
        candidates: candidates.len(),
        ..Default::default()
    };
    let mut dirty = false;
    let mut todo = Vec::new();
    let mut png_cache: HashMap<String, PathBuf> = HashMap::new();
    for item in candidates {
        let key = item.path.to_string_lossy().into_owned();
        let current = item.identity.clone();
        if !state.entries.contains_key(&key) {
            if let Some(old) = state.identities.get(&current).cloned() {
                if old != key
                    && Path::new(&old)
                        .symlink_metadata()
                        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
                {
                    dirty |= transfer_move(Path::new(&old), &item.path, state)?;
                }
            }
        }
        if let Some(record) = state.entries.get_mut(&key) {
            if record.identity == current && record.missing_since.take().is_some() {
                dirty = true;
            }
        }
        let mut prev = state.entries.get(&key).cloned();
        if prev.as_ref().is_some_and(|p| p.identity != current) {
            state.remove(&key);
            dirty = true;
            prev = None;
        }
        let custom = macos::has_custom_icon(&item.path);
        if custom && prev.is_none() {
            report.custom_skipped += 1;
            continue;
        }
        if custom {
            let record = prev.as_ref().unwrap();
            if let Some(expected) = &record.payload {
                if macos::icon_fingerprint(&item.path)?.as_ref() != Some(expected) {
                    // Keep the record as evidence, but never overwrite a manual replacement.
                    report.custom_skipped += 1;
                    continue;
                }
            } else if record.pending {
                report.custom_skipped += 1;
                continue;
            }
        }
        // PNG filename is the SVG + size revision. No bitmap is loaded here.
        let png = if let Some(p) = png_cache.get(&item.source) {
            p.clone()
        } else {
            let p = png_for(theme, &item.source, cache_dir)?;
            png_cache.insert(item.source.clone(), p.clone());
            p
        };
        let revision = png.file_stem().unwrap().to_string_lossy().into_owned();
        if custom
            && prev.as_ref().is_some_and(|p| {
                p.id == item.id && p.revision.as_ref().map_or(true, |r| r == &revision)
            })
        {
            report.unchanged += 1;
            report.already += 1;
            continue;
        }
        // Legacy ownership cannot authorize replacing a possibly manual icon.
        if custom && prev.as_ref().is_some_and(|p| p.payload.is_none()) {
            report.custom_skipped += 1;
            continue;
        }
        todo.push((item, current, png, revision));
    }
    if !todo.is_empty() {
        let before = state.excludes.clone();
        crate::git::ensure_global_rule(state)?;
        dirty |= before != state.excludes;
    }
    for batch in todo.chunks(BATCH) {
        let mut pending = Vec::new();
        for (item, current, png, revision) in batch {
            let key = item.path.to_string_lossy().into_owned();
            let previous = state.entries.get(&key).cloned();
            let record = Entry {
                identity: current.clone(),
                id: item.id.clone(),
                directory: item.directory,
                revision: Some(revision.clone()),
                payload: previous.as_ref().and_then(|p| p.payload.clone()),
                pending: true,
                missing_since: None,
            };
            state.insert(key.clone(), record);
            pending.push((key, previous, png, item));
        }
        // Durable intent precedes AppKit. Successful fingerprints checkpoint in
        // the same bounded batch; interrupted ambiguous intents remain recorded.
        save_state(&state_paths().state, state)?;
        dirty = false;
        for (key, previous, png, item) in pending {
            // Recheck identity immediately before writing a path (atomic saves).
            if identity(&item.path).ok().as_ref() != state.entries.get(&key).map(|r| &r.identity) {
                report.stale_skipped += 1;
                continue;
            }
            match macos::set_icon(&item.path, Some(png)) {
                Ok(true) => {
                    report.applied += 1;
                    if let Ok(Some(payload)) = macos::icon_fingerprint(&item.path) {
                        let record = state.entries.get_mut(&key).unwrap();
                        record.payload = Some(payload);
                        record.pending = false;
                        dirty = true;
                    } else {
                        report.failed += 1;
                    }
                }
                _ => {
                    report.failed += 1;
                    // Keep intent if the API partly succeeded; ownership must
                    // not be silently discarded on a reported failure.
                    if !macos::has_custom_icon(&item.path) {
                        match previous {
                            Some(p) => {
                                state.insert(key, p);
                            }
                            None => {
                                state.remove(&key);
                            }
                        }
                        dirty = true;
                    }
                }
            }
        }
        if dirty {
            save_state(&state_paths().state, state)?;
            dirty = false;
        }
    }
    if dirty {
        save_state(&state_paths().state, state)?;
    }
    if report.applied > 0 {
        crate::render::prune_cache(cache_dir, 1024, 64 * 1024 * 1024)?;
    }
    Ok(report)
}

pub fn excluded_keys(
    root: &Path,
    theme: &Theme,
    config: &Config,
    state: &State,
) -> Result<Vec<String>> {
    let selected: std::collections::HashSet<_> = candidates_for(root, theme, config)?
        .into_iter()
        .map(|c| c.path.to_string_lossy().into_owned())
        .collect();
    Ok(scoped_entries(state, root)
        .filter(|(p, _)| !selected.contains(*p))
        .map(|(p, _)| p.clone())
        .collect())
}

pub fn reconcile(
    root: &Path,
    theme: &Theme,
    config: &Config,
    state: &mut State,
) -> Result<UndoReport> {
    remove_keys(excluded_keys(root, theme, config, state)?, state)
}

pub fn undo(root: &Path, state: &mut State) -> Result<UndoReport> {
    let keys = scoped_entries(state, root)
        .map(|(p, _)| p.clone())
        .collect();
    remove_keys(keys, state)
}

/// Clear only an identified object with a verified payload. Legacy/pending
/// custom icons are ambiguous, so retain their ownership record for recovery.
pub fn remove_keys(keys: Vec<String>, state: &mut State) -> Result<UndoReport> {
    let mut report = UndoReport::default();
    let mut dirty = false;
    for batch in keys.chunks(BATCH) {
        for key in batch {
            let Some(record) = state.entries.get(key) else {
                continue;
            };
            let path = Path::new(key);
            let current = match identity(path) {
                Ok(i) => i,
                Err(_)
                    if !path.exists()
                        && path
                            .symlink_metadata()
                            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
                {
                    state.remove(key);
                    dirty = true;
                    report.stale += 1;
                    continue;
                }
                Err(_) => {
                    report.failed += 1;
                    continue;
                }
            };
            if record.identity != current {
                state.remove(key);
                dirty = true;
                report.stale += 1;
                continue;
            }
            report.eligible += 1;
            if path.symlink_metadata()?.file_type().is_symlink() {
                report.failed += 1;
                continue;
            }
            if macos::has_custom_icon(path) {
                match macos::icon_fingerprint(path) {
                    Ok(Some(p)) if record.payload.as_ref() == Some(&p) && !record.pending => (),
                    _ => {
                        report.failed += 1;
                        continue;
                    }
                }
                if !macos::set_icon(path, None).unwrap_or(false) {
                    report.failed += 1;
                    continue;
                }
                report.removed += 1;
            }
            state.remove(key);
            dirty = true;
        }
        // Icons first on removal: interruption leaves retryable ownership.
        if dirty {
            save_state(&state_paths().state, state)?;
            dirty = false;
        }
    }
    if state.entries.is_empty() {
        let before = state.excludes.clone();
        crate::git::remove_global_rule(state)?;
        if before != state.excludes {
            save_state(&state_paths().state, state)?;
        }
    }
    Ok(report)
}

/// Prune only definitively missing paths in the affected scope. Permission and
/// transient metadata errors never discard ownership.
pub fn prune(state: &mut State, within: Option<&Path>) -> Result<()> {
    let entries: Box<dyn Iterator<Item = (&String, &Entry)> + '_> = match within {
        Some(root) => Box::new(scoped_entries(state, root)),
        None => Box::new(state.entries.iter()),
    };
    let keys: Vec<String> = entries
        .filter(|(p, _)| {
            Path::new(p)
                .symlink_metadata()
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
        })
        .map(|(p, _)| p.clone())
        .collect();
    if keys.is_empty() {
        return Ok(());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let mut dirty = false;
    for key in keys {
        let record = state.entries.get_mut(&key).unwrap();
        match record.missing_since {
            Some(since) if now.saturating_sub(since) >= 30 => {
                state.remove(&key);
                dirty = true;
            }
            None => {
                record.missing_since = Some(now);
                dirty = true;
            }
            _ => (),
        }
    }
    if !dirty {
        return Ok(());
    }
    if state.entries.is_empty() {
        crate::git::remove_global_rule(state)?;
    }
    save_state(&state_paths().state, state)
}

pub fn status(root: &Path, state: &State) -> Vec<(String, bool, String)> {
    scoped_entries(state, root)
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

pub fn clean(state: &mut State) -> Result<UndoReport> {
    remove_keys(state.entries.keys().cloned().collect(), state)
}

/// Guard against a `--files-only`/`--folders-only` combination that would do nothing.
pub fn ensure_something_to_do(config: &Config) -> Result<()> {
    if !config.files && !config.folders {
        bail!("Nothing to do with both files and folders disabled.");
    }
    Ok(())
}
