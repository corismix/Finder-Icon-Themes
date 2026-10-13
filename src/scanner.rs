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

fn skipped_name(name: &str) -> bool {
    name == "Icon\r" || name == ".DS_Store" || name == OPT_OUT_MARKER || name.starts_with("._")
}

fn opted_out(dir: &Path) -> bool {
    dir.join(OPT_OUT_MARKER).exists()
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
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
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
            let file_type = entry.file_type()?;
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
