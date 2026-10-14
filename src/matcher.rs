//! Icon matching, ported from the VS Code material icon theme rules.

use crate::theme::Manifest;

#[derive(Clone, Debug, PartialEq)]
pub struct IconMatch {
    pub id: String,
    /// Manifest `iconPath`, resolved against the theme directory by the renderer.
    pub source: String,
}

/// Closest match first: exact filename, longest extension, dotfile extension,
/// then the theme's generic file/folder fallback. Returns `None` when the theme
/// defines no such icon (no invented fallback).
pub fn find_icon(
    name: &str,
    is_directory: bool,
    manifest: &Manifest,
    is_root: bool,
) -> Option<IconMatch> {
    find_icon_policy(name, is_directory, manifest, is_root, true)
}

pub fn find_icon_policy(
    name: &str,
    is_directory: bool,
    manifest: &Manifest,
    is_root: bool,
    fallbacks: bool,
) -> Option<IconMatch> {
    let lower = name.to_lowercase();
    let id = if is_directory {
        let id = if is_root {
            manifest.root_folder_names.get(&lower)
        } else {
            None
        }
        .or_else(|| manifest.folder_names.get(&lower))
        .or_else(|| {
            if is_root && fallbacks {
                manifest.root_folder.as_ref()
            } else {
                None
            }
        })
        .or_else(|| {
            if fallbacks {
                manifest.folder.as_ref()
            } else {
                None
            }
        })?;
        id.clone()
    } else {
        let mut id = manifest.file_names.get(&lower).cloned();
        if id.is_none() {
            for (index, _) in lower.match_indices('.') {
                let suffix = &lower[index + 1..];
                if let Some(found) = manifest.file_extensions.get(suffix) {
                    id = Some(found.clone());
                    break;
                }
            }
            if id.is_none() && lower.starts_with('.') {
                id = manifest.file_extensions.get(&lower[1..]).cloned();
            }
        }
        id.or_else(|| {
            if fallbacks {
                manifest.file.clone()
            } else {
                None
            }
        })?
    };
    let definition = manifest.icon_definitions.get(&id)?;
    if definition.icon_path.is_empty() {
        return None;
    }
    Some(IconMatch {
        id,
        source: definition.icon_path.clone(),
    })
}

pub const SKIP_CONTENTS: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    ".build",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    ".vercel",
    ".output",
    ".cache",
    ".parcel-cache",
    ".venv",
    "venv",
    "__pycache__",
    "node_modules",
    "dist",
    "build",
    "out",
    "target",
    "coverage",
    "vendor",
    ".gradle",
    ".idea",
    ".dart_tool",
    "Pods",
    "DerivedData",
    ".pytest_cache",
];

pub fn skips_contents(name: &str) -> bool {
    let lower = name.to_lowercase();
    SKIP_CONTENTS
        .iter()
        .any(|skip| *skip == name || skip.to_lowercase() == lower)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::IconDefinition;
    use std::collections::HashMap;

    fn manifest() -> Manifest {
        let names = [
            "npm",
            "env",
            "vitest",
            "config",
            "typescript",
            "json",
            "test",
            "folder-docs",
            "folder-npm",
            "folder-tests",
            "folder-git",
            "folder-root-demo",
            "file",
            "folder",
            "folder-root",
        ];
        Manifest {
            file_names: HashMap::from([
                ("package.json".into(), "npm".into()),
                (".env".into(), "env".into()),
                ("vitest.config.ts".into(), "vitest".into()),
            ]),
            file_extensions: HashMap::from([
                ("config.ts".into(), "config".into()),
                ("ts".into(), "typescript".into()),
                ("json".into(), "json".into()),
                ("test.ts".into(), "test".into()),
            ]),
            folder_names: HashMap::from([
                ("docs".into(), "folder-docs".into()),
                ("node_modules".into(), "folder-npm".into()),
                ("tests".into(), "folder-tests".into()),
                (".git".into(), "folder-git".into()),
            ]),
            root_folder_names: HashMap::from([("demo".into(), "folder-root-demo".into())]),
            icon_definitions: names
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
            file: Some("file".into()),
            folder: Some("folder".into()),
            root_folder: Some("folder-root".into()),
        }
    }

    #[test]
    fn names_beat_extensions_longest_suffix_case_insensitive() {
        let m = manifest();
        assert_eq!(
            find_icon("package.json", false, &m, false).unwrap().id,
            "npm"
        );
        assert_eq!(
            find_icon("VITEST.CONFIG.TS", false, &m, false).unwrap().id,
            "vitest"
        );
        assert_eq!(
            find_icon("vite.config.ts", false, &m, false).unwrap().id,
            "config"
        );
        assert_eq!(
            find_icon("hello.test.ts", false, &m, false).unwrap().id,
            "test"
        );
        assert_eq!(
            find_icon("main.ts", false, &m, false).unwrap().id,
            "typescript"
        );
        assert_eq!(find_icon(".env", false, &m, false).unwrap().id, "env");
        assert_eq!(
            find_icon("docs", true, &m, false).unwrap().id,
            "folder-docs"
        );
        assert_eq!(find_icon(".git", true, &m, false).unwrap().id, "folder-git");
        assert_eq!(find_icon("unknown", true, &m, false).unwrap().id, "folder");
        assert_eq!(
            find_icon("unrecognised.weird", false, &m, false)
                .unwrap()
                .id,
            "file"
        );
    }

    #[test]
    fn specificity_is_a_rule_not_an_icon_id() {
        let mut m = manifest();
        m.file_names.insert("special".into(), "file".into());
        assert!(find_icon_policy("special", false, &m, false, false).is_some());
        assert!(find_icon_policy("unknown", false, &m, false, false).is_none());
        assert!(find_icon_policy("docs", true, &m, false, false).is_some());
        assert!(find_icon_policy("misc", true, &m, false, false).is_none());
    }

    #[test]
    fn generic_icons_fall_back_and_root_folders_use_their_own() {
        let m = manifest();
        assert_eq!(
            find_icon("demo", true, &m, true).unwrap().id,
            "folder-root-demo"
        );
        assert_eq!(find_icon("misc", true, &m, true).unwrap().id, "folder-root");
        assert_eq!(
            find_icon("src", true, &m, false).unwrap().source,
            "../icons/folder.svg"
        );
        assert_eq!(
            find_icon("any.randomformat", false, &m, false)
                .unwrap()
                .source,
            "../icons/file.svg"
        );
        assert!(find_icon("misc", true, &Manifest::default(), false).is_none());
        assert!(find_icon("unknown.zzz", false, &Manifest::default(), false).is_none());
    }
}
