//! Theme loading.
//!
//! Material Icon Theme ships embedded in the binary as a gzipped archive
//! (`assets/material-theme.tar.gz`): the VS Code manifest plus its SVGs,
//! decompressed in memory. Setting `FINDER_ICON_THEMES_THEME_DIR` overrides
//! this with a theme on disk (an npm-installed `material-icon-theme` package).

use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use serde::Deserialize;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tar::Archive;

static EMBEDDED_ARCHIVE: &[u8] = include_bytes!("../assets/material-theme.tar.gz");
static EMBEDDED: OnceLock<Embedded> = OnceLock::new();

struct Embedded {
    manifest: Vec<u8>,
    icons: HashMap<String, Vec<u8>>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct IconDefinition {
    #[serde(rename = "iconPath")]
    pub icon_path: String,
}

#[derive(Deserialize, Default, Clone, Debug)]
pub struct Manifest {
    #[serde(rename = "fileNames", default)]
    pub file_names: HashMap<String, String>,
    #[serde(rename = "fileExtensions", default)]
    pub file_extensions: HashMap<String, String>,
    #[serde(rename = "folderNames", default)]
    pub folder_names: HashMap<String, String>,
    #[serde(rename = "rootFolderNames", default)]
    pub root_folder_names: HashMap<String, String>,
    #[serde(rename = "iconDefinitions", default)]
    pub icon_definitions: HashMap<String, IconDefinition>,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(rename = "rootFolder", default)]
    pub root_folder: Option<String>,
}

enum IconSource {
    Embedded(&'static HashMap<String, Vec<u8>>),
    Disk {
        manifest_dir: PathBuf,
        root: PathBuf,
    },
}

pub struct Theme {
    pub name: String,
    pub title: String,
    pub manifest: Manifest,
    source: IconSource,
    pub(crate) prepared: std::cell::RefCell<HashMap<(PathBuf, String), PathBuf>>,
}

impl Theme {
    pub(crate) fn immutable(&self) -> bool {
        matches!(self.source, IconSource::Embedded(_))
    }
    /// Raw SVG bytes for a manifest `iconPath`.
    pub fn svg_bytes(&self, source: &str) -> Result<Cow<'_, [u8]>> {
        let name = Path::new(source)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if !name.ends_with(".svg") && !(cfg!(feature = "extended-svg") && name.ends_with(".svgz")) {
            bail!("Unexpected icon resource: {source}");
        }
        match &self.source {
            IconSource::Embedded(icons) => icons
                .get(name)
                .map(|b| Cow::Borrowed(b.as_slice()))
                .with_context(|| format!("Theme is missing icon {name}")),
            IconSource::Disk { manifest_dir, root } => {
                let candidate = manifest_dir.join(source);
                let path = fs::canonicalize(&candidate)
                    .with_context(|| format!("Cannot read icon {}", candidate.display()))?;
                if !path.starts_with(root) {
                    bail!("Icon {source} points outside the theme package");
                }
                fs::read(&path)
                    .map(Cow::Owned)
                    .with_context(|| format!("Cannot read icon {}", path.display()))
            }
        }
    }
}

pub fn list() -> Vec<(&'static str, &'static str)> {
    vec![("material", "Material Icon Theme (VS Code)")]
}

/// The on-disk theme override, if the environment provides one.
pub fn override_dir() -> Option<PathBuf> {
    std::env::var_os("FINDER_ICON_THEMES_THEME_DIR").map(PathBuf::from)
}

pub fn load(theme: &str, override_dir: Option<&Path>) -> Result<Theme> {
    if theme != "material" {
        bail!("Unknown theme: {theme}. Run `finder-icon-themes themes`.");
    }
    let (manifest_bytes, source) = match override_dir {
        Some(dir) => {
            let manifest_path = dir.join("dist/material-icons.json");
            if !manifest_path.is_file() {
                bail!(
                    "Material theme not installed at {}.",
                    manifest_path.display()
                );
            }
            let bytes = fs::read(&manifest_path)
                .with_context(|| format!("Cannot read {}", manifest_path.display()))?;
            let manifest_dir = manifest_path
                .parent()
                .unwrap_or(Path::new("."))
                .to_path_buf();
            (
                bytes,
                IconSource::Disk {
                    manifest_dir,
                    root: fs::canonicalize(dir)?,
                },
            )
        }
        None => {
            let embedded = embedded()?;
            (
                embedded.manifest.clone(),
                IconSource::Embedded(&embedded.icons),
            )
        }
    };
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)
        .context("Cannot parse the Material Icon Theme manifest")?;
    Ok(Theme {
        name: theme.to_string(),
        title: "Material Icon Theme".to_string(),
        manifest,
        source,
        prepared: std::cell::RefCell::new(HashMap::new()),
    })
}

fn embedded() -> Result<&'static Embedded> {
    if let Some(existing) = EMBEDDED.get() {
        return Ok(existing);
    }
    let built = build_embedded()?;
    Ok(EMBEDDED.get_or_init(|| built))
}

fn build_embedded() -> Result<Embedded> {
    let mut archive = Archive::new(GzDecoder::new(EMBEDDED_ARCHIVE));
    let mut manifest = None;
    let mut icons = HashMap::new();
    for entry in archive.entries().context("Cannot read embedded theme")? {
        let mut entry = entry?;
        let path = entry
            .path()?
            .to_string_lossy()
            .trim_start_matches("./")
            .to_string();
        if path == "material-icons.json" {
            let mut buffer = Vec::new();
            entry.read_to_end(&mut buffer)?;
            manifest = Some(buffer);
        } else if let Some(name) = path.strip_prefix("icons/") {
            if name.ends_with(".svg") && !name.starts_with("._") {
                let mut buffer = Vec::new();
                entry.read_to_end(&mut buffer)?;
                icons.insert(name.to_string(), buffer);
            }
        }
    }
    Ok(Embedded {
        manifest: manifest.context("embedded theme has no manifest")?,
        icons,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_theme_loads_without_any_files() {
        let theme = load("material", None).unwrap();
        assert!(theme.manifest.icon_definitions.len() > 500);
        let source = theme
            .manifest
            .icon_definitions
            .values()
            .next()
            .unwrap()
            .icon_path
            .clone();
        assert!(theme.svg_bytes(&source).unwrap().starts_with(b"<svg"));
    }

    #[test]
    fn unknown_theme_is_rejected() {
        assert!(load("not-a-theme", None).is_err());
    }
}
