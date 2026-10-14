//! Bounded, incremental filesystem reconciliation. Full sweeps are reserved
//! for startup, repository topology changes and missed-event recovery.
use crate::{
    apply,
    config::Config,
    git, scanner,
    state::{read_state, save_state, state_paths, MutationLock, State},
    theme::Theme,
};
use anyhow::{bail, Result};
use notify::{
    event::{ModifyKind, RenameMode},
    recommended_watcher, Event, EventKind, RecursiveMode, Watcher,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{sync_channel, RecvTimeoutError},
    Arc,
};
use std::time::{Duration, Instant, SystemTime};

const QUEUE: usize = 1024;
const MAX_PATHS: usize = 2048;
const QUIET: Duration = Duration::from_millis(500);
const MAX_DELAY: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Dirty {
    paths: BTreeMap<PathBuf, bool>,
    moves: Vec<(PathBuf, PathBuf)>,
    rescan: bool,
    discovery: bool,
}
impl Dirty {
    fn add(&mut self, path: PathBuf, recursive: bool) {
        if self.rescan {
            return;
        }
        if self.paths.iter().any(|(p, r)| *r && path.starts_with(p)) {
            return;
        }
        if recursive {
            self.paths.retain(|p, _| !p.starts_with(&path));
        }
        self.paths
            .entry(path)
            .and_modify(|r| *r |= recursive)
            .or_insert(recursive);
        if self.paths.len() > MAX_PATHS {
            self.overflow();
        }
    }
    fn overflow(&mut self) {
        self.rescan = true;
        self.paths.clear();
        self.moves.clear();
    }
    fn event(&mut self, event: Event, roots: &[PathBuf]) {
        if event.need_rescan() || matches!(event.kind, EventKind::Any | EventKind::Other) {
            self.overflow();
            return;
        }
        if let EventKind::Modify(ModifyKind::Name(RenameMode::Both)) = event.kind {
            if event.paths.len() == 2
                && event
                    .paths
                    .iter()
                    .all(|p| roots.iter().any(|r| p.starts_with(r)))
            {
                self.moves
                    .push((event.paths[0].clone(), event.paths[1].clone()));
                if self.moves.len() > 256 {
                    self.overflow();
                    return;
                }
            }
        }
        for path in event.paths {
            let Some(root) = roots.iter().find(|r| path.starts_with(r)) else {
                continue;
            };
            if path
                .file_name()
                .is_some_and(|n| n == scanner::OPT_OUT_MARKER || n == ".git")
            {
                self.discovery = true;
                self.add(path.parent().unwrap_or(root).to_path_buf(), true);
                continue;
            }
            if !scanner::allowed_path(root, &path) {
                continue;
            }
            let recursive = !matches!(
                event.kind,
                EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Metadata(_))
                    | EventKind::Access(_)
            );
            if matches!(event.kind, EventKind::Access(_)) {
                continue;
            }
            if matches!(
                event.kind,
                EventKind::Create(_)
                    | EventKind::Remove(_)
                    | EventKind::Modify(ModifyKind::Name(_))
            ) && (path.is_dir() || matches!(event.kind, EventKind::Remove(_)))
            {
                self.discovery = true;
            }
            self.add(path, recursive);
        }
    }
}

fn roots(config: &Config) -> Vec<PathBuf> {
    let mut roots: Vec<_> = config
        .watch_roots
        .iter()
        .filter_map(|p| p.canonicalize().ok())
        .filter(|p| p.is_dir())
        .collect();
    roots.sort();
    roots.dedup();
    let mut out: Vec<PathBuf> = Vec::new();
    for root in roots {
        if !out.iter().any(|r| root.starts_with(r)) {
            out.push(root);
        }
    }
    out
}
fn discover(roots: &[PathBuf], config: &Config) -> Vec<PathBuf> {
    roots
        .iter()
        .flat_map(|r| {
            git::discover_repos(r)
                .into_iter()
                .filter(|p| scanner::config_allows(r, p, config))
                .collect::<Vec<_>>()
        })
        .collect()
}
fn version(path: &Path) -> Option<(SystemTime, u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let m = path.metadata().ok()?;
    Some((m.modified().ok()?, m.len(), m.ino()))
}

pub fn run(config: &Config, theme: &Theme) -> Result<()> {
    let roots = roots(config);
    if roots.is_empty() {
        bail!(
            "No watch roots exist. Configure watch_roots in {}",
            crate::config::config_path().display()
        );
    }
    let paths = state_paths();
    crate::render::prune_cache(&paths.cache, 1024, 64 * 1024 * 1024)?;
    let (tx, rx) = sync_channel(QUEUE);
    let missed = Arc::new(AtomicBool::new(false));
    let overflow = missed.clone();
    let callback_roots = roots.clone();
    let callback_config = config.clone();
    let mut watcher = recommended_watcher(move |result: notify::Result<Event>| {
        let mut event = match result {
            Ok(e) if e.paths.len() <= MAX_PATHS => e,
            _ => {
                overflow.store(true, Ordering::Release);
                let _ = tx.try_send(Event::new(EventKind::Other));
                return;
            }
        };
        if callback_config.respect_gitignore {
            let mut policy_changed = false;
            for path in &mut event.paths {
                if path.file_name().is_some_and(|n| n == ".gitignore") {
                    *path = path.parent().unwrap_or(path).to_path_buf();
                    policy_changed = true;
                } else if path.ends_with(".git/info/exclude") || path.ends_with(".git/index") {
                    if let Some(repo) = path
                        .ancestors()
                        .find(|p| p.file_name().is_some_and(|n| n == ".git"))
                        .and_then(|p| p.parent())
                    {
                        *path = repo.to_path_buf();
                        policy_changed = true;
                    }
                }
            }
            if policy_changed {
                event.kind = EventKind::Modify(ModifyKind::Any);
            }
        }
        // Reject irrelevant paths before they consume bounded queue space. Keep
        // both endpoints of renames; a move out of a policy boundary still owns
        // its icon and must transfer its journal record.
        let relevant = event.need_rescan()
            || matches!(
                event.kind,
                EventKind::Any | EventKind::Other | EventKind::Modify(ModifyKind::Name(_))
            )
            || event.paths.iter().any(|p| {
                callback_roots.iter().any(|r| {
                    p.starts_with(r)
                        && (p
                            .file_name()
                            .is_some_and(|n| n == scanner::OPT_OUT_MARKER || n == ".git")
                            || scanner::allowed_path(r, p)
                                && scanner::config_allows(r, p, &callback_config))
                })
            });
        if !relevant || matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        if tx.try_send(event).is_err() {
            overflow.store(true, Ordering::Release);
        }
    })?;
    // Register first; edits during reconciliation are queued and reconciled.
    for root in &roots {
        watcher.watch(root, RecursiveMode::Recursive)?;
    }
    let mut state = State::default();
    let mut loaded = false;
    let mut generation = None;
    let mut repos = Vec::new();
    let mut dirty = Dirty {
        rescan: true,
        ..Default::default()
    };
    loop {
        {
            let _guard = MutationLock::acquire(&paths.base)?;
            if !loaded || generation != version(&paths.state) {
                state = read_state(&paths.state)?;
                loaded = true;
            }
            if dirty.rescan || dirty.discovery {
                repos = discover(&roots, config);
            }
            if dirty.rescan {
                for repo in &repos {
                    if let Err(e) = apply::apply(repo, theme, config, &paths.cache, &mut state) {
                        eprintln!("finder-icon-themes: {}: {e:#}", repo.display());
                    }
                }
                for root in &roots {
                    apply::prune(&mut state, Some(root))?;
                }
            } else {
                let mut moved = false;
                for (from, to) in &dirty.moves {
                    moved |= apply::transfer_move(from, to, &mut state)?;
                }
                if moved {
                    save_state(&paths.state, &state)?;
                }
                // Dirty paths collapse subtrees and duplicates. Repository
                // resolution uses the index, never climbs outside watch roots.
                for (path, recursive) in &dirty.paths {
                    for repo in repos
                        .iter()
                        .filter(|r| r.starts_with(path) || path.starts_with(r))
                    {
                        let result = if repo.starts_with(path) {
                            apply::apply(repo, theme, config, &paths.cache, &mut state)
                        } else {
                            scanner::collect_with_config(
                                repo,
                                path,
                                *recursive,
                                &theme.manifest,
                                config,
                            )
                            .and_then(|c| {
                                apply::apply_candidates(c, theme, &paths.cache, &mut state)
                            })
                        };
                        if let Err(e) = result {
                            eprintln!("finder-icon-themes: {}: {e:#}", path.display());
                        }
                    }
                }
            }
            if !dirty.rescan {
                // Reconcile ALL destinations before forgetting removed sources,
                // including backends that emit unpaired rename notifications.
                for path in dirty.paths.keys() {
                    apply::prune(&mut state, Some(path))?;
                }
            }
            generation = version(&paths.state);
        }
        dirty = Dirty::default();
        let first = loop {
            crate::service::bound_log()?;
            match rx.recv_timeout(Duration::from_secs(30)) {
                Ok(e) => break e,
                Err(RecvTimeoutError::Timeout) => {
                    if state.entries.values().any(|e| e.missing_since.is_some()) {
                        break Event::new(EventKind::Other);
                    }
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
        };
        let deadline = Instant::now() + MAX_DELAY;
        dirty.event(first, &roots);
        loop {
            let timeout = QUIET.min(deadline.saturating_duration_since(Instant::now()));
            match rx.recv_timeout(timeout) {
                Ok(e) => dirty.event(e, &roots),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
            if Instant::now() >= deadline {
                break;
            }
        }
        if missed.swap(false, Ordering::AcqRel) {
            dirty.overflow();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dirty_set_coalesces_and_overflow_requests_recovery() {
        let mut d = Dirty::default();
        d.add("/repo/a/file".into(), false);
        d.add("/repo/a".into(), true);
        d.add("/repo/a/other".into(), false);
        assert_eq!(d.paths.len(), 1);
        for i in 0..=MAX_PATHS {
            d.add(format!("/repo/{i}").into(), false);
        }
        assert!(d.rescan);
        assert!(d.paths.is_empty());
    }
    #[test]
    fn shared_exclusions_reject_build_contents_and_finder_noise() {
        let roots = vec![PathBuf::from("/repo")];
        let mut d = Dirty::default();
        d.event(
            Event::new(EventKind::Create(notify::event::CreateKind::File))
                .add_path("/repo/node_modules/a.ts".into()),
            &roots,
        );
        d.event(
            Event::new(EventKind::Modify(ModifyKind::Any)).add_path("/repo/Icon\r".into()),
            &roots,
        );
        assert!(d.paths.is_empty());
        d.event(Event::new(EventKind::Other), &roots);
        assert!(d.rescan);
    }
    #[test]
    fn missed_events_force_conservative_recovery() {
        let mut d = Dirty::default();
        let event = Event::new(EventKind::Modify(ModifyKind::Data(
            notify::event::DataChange::Content,
        )))
        .set_flag(notify::event::Flag::Rescan);
        d.event(event, &[]);
        assert!(d.rescan);
    }
}
