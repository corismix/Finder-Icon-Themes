//! Background watcher: discover repositories and keep their icons applied.

use anyhow::{bail, Result};
use notify::{recommended_watcher, RecursiveMode, Watcher};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::time::Duration;

use crate::apply;
use crate::config::Config;
use crate::git;
use crate::state::{read_state, state_paths, MutationLock, State};
use crate::theme::Theme;

/// Wait this long after the last filesystem event before doing any work.
const DEBOUNCE: Duration = Duration::from_secs(2);

pub fn run(config: &Config, theme: &Theme) -> Result<()> {
    let roots: Vec<PathBuf> = config
        .watch_roots
        .iter()
        .filter(|r| r.is_dir())
        .cloned()
        .collect();
    if roots.is_empty() {
        bail!(
            "No watch roots exist. Add existing folders to `watch_roots` in {}",
            crate::config::config_path().display()
        );
    }
    let paths = state_paths();

    {
        let _guard = MutationLock::acquire(&paths.base)?;
        let mut state = read_state(&paths.state)?;
        sync(config, theme, &roots, None, &mut state);
    }

    let (tx, rx) = channel();
    let mut watcher = recommended_watcher(move |result: notify::Result<notify::Event>| {
        if let Ok(event) = result {
            let _ = tx.send(event.paths);
        }
    })?;
    for root in &roots {
        watcher.watch(root, RecursiveMode::Recursive)?;
    }

    loop {
        let mut changed: Vec<PathBuf> = match rx.recv() {
            Ok(paths) => paths,
            Err(_) => return Ok(()),
        };
        loop {
            match rx.recv_timeout(DEBOUNCE) {
                Ok(more) => changed.extend(more),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }
        let _guard = MutationLock::acquire(&paths.base)?;
        let mut state = read_state(&paths.state)?;
        sync(config, theme, &roots, Some(&changed), &mut state);
    }
}

fn sync(
    config: &Config,
    theme: &Theme,
    roots: &[PathBuf],
    changed: Option<&[PathBuf]>,
    state: &mut State,
) {
    let repos: Vec<PathBuf> = match changed {
        Some(paths) => {
            let mut set = BTreeSet::new();
            for path in paths {
                if let Some(repo) = git::repo_for(path) {
                    set.insert(repo);
                }
            }
            set.into_iter().collect()
        }
        None => roots
            .iter()
            .flat_map(|root| git::discover_repos(root))
            .collect(),
    };
    for repo in repos {
        if !repo.is_dir() {
            continue;
        }
        if let Err(error) = apply::apply(&repo, theme, config, &state_paths().cache, state) {
            eprintln!("finder-icon-themes: {}: {error}", repo.display());
        }
    }
    if let Err(error) = apply::prune(state, None) {
        eprintln!("finder-icon-themes: prune failed: {error}");
    }
}
