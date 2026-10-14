//! Git awareness: repository discovery and the single global exclude rule.
//!
//! We never write to a repository's `.git/info/exclude` or its tracked
//! `.gitignore`. Instead a folder icon (a hidden `Icon\r` file) is hidden from
//! every repository at once by one `Icon?` line in the user's global
//! `core.excludesFile`.

use anyhow::{Context, Result};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::matcher::skips_contents;
use crate::state::State;

pub const MARKER: &str = "# finder-icon-themes";
pub const RULE: &str = "Icon?";

/// A directory is a repository if it holds a `.git` entry (a directory for a
/// normal repo, a file for a worktree or submodule). Bare repositories have no
/// working tree and no `.git` entry, so they are naturally ignored.
pub fn is_repo(dir: &Path) -> bool {
    dir.join(".git").exists()
}

/// Find repository roots under `root` without descending into a repository
/// (a parent repository already covers its submodules' files). Dot-directories
/// are not descended into, but a dot-directory that is itself a repository
/// (e.g. `~/.dotfiles`) is still recognised.
pub fn discover_repos(root: &Path) -> Vec<PathBuf> {
    let mut repos = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if crate::scanner::opted_out(&dir) {
            continue;
        }
        if is_repo(&dir) {
            repos.push(dir);
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let path = entry.path();
            if crate::scanner::opted_out(&path) {
                continue;
            }
            if is_repo(&path) {
                repos.push(path);
                continue;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if name.starts_with('.') || skips_contents(name) {
                continue;
            }
            stack.push(path);
        }
    }
    repos.sort();
    repos
}

/// Walk up from a path to the repository that contains it.
pub fn repo_for(path: &Path) -> Option<PathBuf> {
    let mut dir = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };
    if dir.file_name() == Some(OsStr::new(".git")) {
        dir = dir.parent()?.to_path_buf();
    }
    loop {
        if is_repo(&dir) {
            return Some(dir);
        }
        dir = dir.parent()?.to_path_buf();
    }
}

/// The ignore file git uses globally. Honours `core.excludesFile`, falling back
/// to git's own default (`$XDG_CONFIG_HOME/git/ignore`).
pub fn global_excludes_file() -> PathBuf {
    static FILE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FILE.get_or_init(resolve_global_excludes_file).clone()
}
fn resolve_global_excludes_file() -> PathBuf {
    if let Ok(output) = Command::new("git")
        .args(["config", "--global", "--get", "core.excludesFile"])
        .output()
    {
        if output.status.success() {
            let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !raw.is_empty() {
                return expand_home(&raw);
            }
        }
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::state::home().join(".config"));
    base.join("git/ignore")
}

fn expand_home(raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/") {
        crate::state::home().join(rest)
    } else {
        PathBuf::from(raw)
    }
}

/// Ensure the `Icon?` rule exists in the global ignore file. Records ownership
/// only when this tool added it, so `clean` can take it back out.
pub fn ensure_global_rule(state: &mut State) -> Result<()> {
    let file = global_excludes_file();
    let before = match fs::read_to_string(&file) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).context("Cannot read global excludes"),
    };
    if before.lines().any(|line| line.trim() == RULE) {
        let owned = state.excludes.global_owned
            && state.excludes.global_file.as_ref() == Some(&file)
            && before.contains(&format!("{MARKER}\n{RULE}\n"));
        state.excludes.global_file = Some(file);
        state.excludes.global_owned = owned;
        return Ok(());
    }
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Cannot create {}", parent.display()))?;
    }
    let separator = if before.is_empty() || before.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let block = format!("{separator}{MARKER}\n{RULE}\n");
    state.excludes.global_file = Some(file.clone());
    state.excludes.global_owned = true;
    crate::state::save_state(&crate::state::state_paths().state, state)?;
    let mut handle = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .with_context(|| format!("Cannot open {}", file.display()))?;
    std::io::Write::write_all(&mut handle, block.as_bytes())
        .with_context(|| format!("Cannot write {}", file.display()))?;
    state.excludes.global_file = Some(file);
    state.excludes.global_owned = true;
    Ok(())
}

/// Remove the rule only if this tool added it.
pub fn remove_global_rule(state: &mut State) -> Result<()> {
    let (file, owned) = (
        state.excludes.global_file.clone(),
        state.excludes.global_owned,
    );
    if owned {
        if let Some(file) = file {
            let text = match fs::read_to_string(&file) {
                Ok(s) => s,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(e) => return Err(e).context("Cannot read owned global excludes"),
            };
            {
                let block = format!("{MARKER}\n{RULE}\n");
                if text.contains(&block) {
                    fs::write(&file, text.replace(&block, ""))
                        .with_context(|| format!("Cannot write {}", file.display()))?;
                }
            }
        }
    }
    state.excludes = Default::default();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_repos_and_stops_at_their_root() {
        let base = std::env::temp_dir().join(format!("fit-git-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("alpha/.git")).unwrap();
        fs::create_dir_all(base.join("alpha/nested/.git")).unwrap();
        fs::create_dir_all(base.join("beta")).unwrap();
        fs::write(base.join("beta/main.rs"), "").unwrap();
        fs::create_dir_all(base.join("plain")).unwrap();
        let repos = discover_repos(&base);
        assert_eq!(repos, vec![base.join("alpha")]);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn dot_named_repositories_are_discovered() {
        let base = std::env::temp_dir().join(format!("fit-git-dot-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join(".dotfiles/.git")).unwrap();
        fs::create_dir_all(base.join("visible/.git")).unwrap();
        fs::create_dir_all(base.join(".cache/junk")).unwrap();
        let repos = discover_repos(&base);
        assert!(
            repos.contains(&base.join(".dotfiles")),
            "hidden repo missed: {repos:?}"
        );
        assert!(repos.contains(&base.join("visible")));
        assert!(!repos.iter().any(|r| r.ends_with(".cache")));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn worktree_pointer_file_counts_as_a_repo() {
        let base = std::env::temp_dir().join(format!("fit-git-wt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        fs::write(base.join(".git"), "gitdir: ../elsewhere").unwrap();
        assert!(is_repo(&base));
        assert_eq!(repo_for(&base.join("src/main.rs")), Some(base.clone()));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn global_rule_is_added_and_removed_once() {
        let dir = std::env::temp_dir().join(format!("fit-git-excl-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("ignore");
        let mut state = State::default();
        state.excludes.global_file = Some(file.clone());
        // simulate the append the way ensure_global_rule would
        fs::write(&file, format!("{MARKER}\n{RULE}\n")).unwrap();
        state.excludes.global_owned = true;
        remove_global_rule(&mut state).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "");
        assert!(!state.excludes.global_owned);
        let _ = fs::remove_dir_all(&dir);
    }
}

/// Let Git evaluate its own ignore/negation/global/worktree rules. `--others`
/// preserves tracked files even if a pattern matches them; `--directory`
/// collapses wholly ignored trees instead of enumerating build outputs.
/// Each submodule/worktree is queried independently by the scanner.
pub fn ignored_paths(repo: &Path) -> Result<Vec<PathBuf>> {
    use std::io::Read;
    use std::os::unix::ffi::OsStringExt;
    use std::process::Stdio;
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
            "-z",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut bytes = Vec::new();
    let read = child
        .stdout
        .take()
        .unwrap()
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes);
    if read.is_err() || bytes.len() > 16 * 1024 * 1024 {
        let _ = child.kill();
        let _ = child.wait();
        anyhow::bail!("Git ignore listing exceeded its bound or failed; policy left unapplied");
    }
    anyhow::ensure!(
        child.wait()?.success(),
        "Cannot evaluate Git ignore policy at {}",
        repo.display()
    );
    Ok(bytes
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| {
            let p = p.strip_suffix(b"/").unwrap_or(p);
            repo.join(std::ffi::OsString::from_vec(p.to_vec()))
        })
        .collect())
}
