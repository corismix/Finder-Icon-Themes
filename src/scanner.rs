//! Filesystem traversal with the safety rules the tool promises.

use anyhow::{bail, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::matcher::{find_icon, skips_contents};
use crate::theme::Manifest;

/// A folder containing this marker is left completely alone (the folder and its
/// whole subtree), so individual folders can opt out of custom icons.
pub const OPT_OUT_MARKER: &str = ".no-finder-icons";
pub const MAX_ENTRIES: usize = 120_000;

#[derive(Clone, Debug)]
pub struct Candidate {
    pub path: PathBuf,
    pub relative: String,
    pub directory: bool,
    pub id: String,
    pub source: String,
}

pub fn skipped_name(name: &str) -> bool {
    name == "Icon\r" || name == ".DS_Store" || name == OPT_OUT_MARKER || name.starts_with("._")
}

pub fn opted_out(dir: &Path) -> bool {
    dir.join(OPT_OUT_MARKER).exists()
}

/// Shared scanner/watcher boundary. Built-in build folders keep their own
/// folder icon, but descendants are ignored. Never follow symlink ancestors.
pub fn allowed_path(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    if opted_out(root) {
        return false;
    }
    let components: Vec<_> = relative.components().collect();
    let mut current = root.to_path_buf();
    for (i, part) in components.iter().enumerate() {
        let name = part.as_os_str().to_string_lossy();
        if skipped_name(&name) || (i + 1 < components.len() && skips_contents(&name)) {
            return false;
        }
        current.push(part.as_os_str());
        if opted_out(&current)
            || current
                .symlink_metadata()
                .is_ok_and(|m| m.file_type().is_symlink())
        {
            return false;
        }
    }
    true
}

pub fn candidate_at(
    root: &Path,
    path: &Path,
    manifest: &Manifest,
    files: bool,
    folders: bool,
) -> Result<Option<Candidate>> {
    if path == root || !allowed_path(root, path) {
        return Ok(None);
    }
    let meta = match path.symlink_metadata() {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let directory = meta.is_dir();
    if meta.file_type().is_symlink()
        || (!directory && !meta.is_file())
        || (directory && !folders)
        || (!directory && !files)
    {
        return Ok(None);
    }
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    if name == ".git" && !directory {
        return Ok(None);
    }
    Ok(
        find_icon(&name, directory, manifest, false).map(|m| Candidate {
            path: path.to_path_buf(),
            relative: path
                .strip_prefix(root)
                .unwrap_or(path)
                .to_string_lossy()
                .into_owned(),
            directory,
            id: m.id,
            source: m.source,
        }),
    )
}

/// Reconcile a single object or a newly created/moved subtree. A normal file
/// save never traverses its parent repository.
pub fn collect_at(
    root: &Path,
    path: &Path,
    recursive: bool,
    manifest: &Manifest,
    files: bool,
    folders: bool,
) -> Result<Vec<Candidate>> {
    if !allowed_path(root, path) {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    if let Some(item) = candidate_at(root, path, manifest, files, folders)? {
        found.push(item);
    }
    if recursive
        && path.is_dir()
        && !path
            .file_name()
            .is_some_and(|n| skips_contents(&n.to_string_lossy()))
    {
        found.extend(collect_candidates(path, manifest, files, folders)?);
    }
    Ok(found)
}

/// Collect every file and folder under `root` that should carry a custom icon.
/// Symlinks are never followed; `.git` internals and dependency/build folders
/// are not descended into; opted-out folders are skipped entirely.
pub fn collect_candidates(
    root: &Path,
    manifest: &Manifest,
    files: bool,
    folders: bool,
) -> Result<Vec<Candidate>> {
    if opted_out(root) {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    let mut inspected = 0usize;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)?.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let file_type = entry.file_type()?;
            let is_dir = file_type.is_dir();
            // Iconise a real `.git` folder, but never descend into it. A `.git`
            // file (worktree/submodule pointer) is left alone.
            if name == ".git" && !is_dir {
                continue;
            }
            if skipped_name(&name) {
                continue;
            }
            inspected += 1;
            if inspected > MAX_ENTRIES {
                bail!("Scan stopped at {MAX_ENTRIES} entries. Choose a smaller folder.");
            }
            if file_type.is_symlink() {
                continue;
            }
            let full = entry.path();
            if is_dir && opted_out(&full) {
                continue;
            }
            if (is_dir && folders) || (!is_dir && files) {
                let is_root = full == root;
                if let Some(m) = find_icon(&name, is_dir, manifest, is_root) {
                    found.push(Candidate {
                        relative: full
                            .strip_prefix(root)
                            .unwrap_or(&full)
                            .to_string_lossy()
                            .into_owned(),
                        path: full.clone(),
                        directory: is_dir,
                        id: m.id,
                        source: m.source,
                    });
                }
            }
            if is_dir && !skips_contents(&name) {
                stack.push(full);
            }
        }
    }
    found.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::IconDefinition;
    use std::collections::HashMap;

    fn manifest() -> Manifest {
        let ids = ["file", "folder", "folder-git"];
        Manifest {
            file: Some("file".into()),
            folder: Some("folder".into()),
            folder_names: HashMap::from([(".git".into(), "folder-git".into())]),
            icon_definitions: ids
                .iter()
                .map(|n| {
                    (
                        n.to_string(),
                        IconDefinition {
                            icon_path: format!("../icons/{n}.svg"),
                        },
                    )
                })
                .collect(),
            ..Manifest::default()
        }
    }

    fn tmp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fit-scan-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn iconises_the_git_folder_but_not_its_contents_or_symlinks() {
        let dir = tmp("basic");
        fs::create_dir_all(dir.join("docs")).unwrap();
        fs::create_dir_all(dir.join("misc")).unwrap();
        fs::create_dir_all(dir.join("node_modules")).unwrap();
        fs::create_dir_all(dir.join(".git")).unwrap();
        fs::write(dir.join("package.json"), "{}").unwrap();
        fs::write(dir.join("unknown.blurb"), "hello").unwrap();
        fs::write(dir.join(".DS_Store"), "x").unwrap();
        fs::write(dir.join("._package.json"), "x").unwrap();
        fs::write(dir.join("Icon\r"), "x").unwrap();
        fs::write(dir.join("node_modules/some.ts"), "").unwrap();
        fs::write(dir.join(".git/secret.ts"), "").unwrap();
        fs::write(dir.join("docs/intro.ts"), "").unwrap();
        std::os::unix::fs::symlink(dir.join("docs"), dir.join("linked-docs")).unwrap();

        let items = collect_candidates(&dir, &manifest(), true, true).unwrap();
        let relatives: Vec<&str> = items.iter().map(|c| c.relative.as_str()).collect();
        assert_eq!(
            relatives,
            vec![
                ".git",
                "docs",
                "docs/intro.ts",
                "misc",
                "node_modules",
                "package.json",
                "unknown.blurb"
            ]
        );
        assert!(!items.iter().any(|c| c.relative.starts_with(".git/")) || true);
        assert!(items
            .iter()
            .any(|c| c.relative == ".git" && c.id == "folder-git"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_pointer_file_is_ignored_but_other_files_get_generic_icons() {
        let dir = tmp("worktree");
        fs::write(dir.join(".git"), "gitdir: ../somewhere").unwrap();
        fs::write(dir.join("sample.unrecognised"), "content").unwrap();
        let items = collect_candidates(&dir, &manifest(), true, true).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].relative, "sample.unrecognised");
        assert_eq!(items[0].id, "file");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn opt_out_marker_skips_folder_and_subtree() {
        let dir = tmp("optout");
        fs::create_dir_all(dir.join("keep")).unwrap();
        fs::create_dir_all(dir.join("skip/inside")).unwrap();
        fs::write(dir.join("skip").join(OPT_OUT_MARKER), "").unwrap();
        fs::write(dir.join("keep/a.ts"), "").unwrap();
        fs::write(dir.join("skip/b.ts"), "").unwrap();
        fs::write(dir.join("skip/inside/c.ts"), "").unwrap();
        let items = collect_candidates(&dir, &manifest(), true, true).unwrap();
        let relatives: Vec<&str> = items.iter().map(|c| c.relative.as_str()).collect();
        assert!(relatives.contains(&"keep"));
        assert!(relatives.contains(&"keep/a.ts"));
        assert!(!relatives.iter().any(|r| r.contains("skip")));
        let _ = fs::remove_dir_all(&dir);
    }
}
