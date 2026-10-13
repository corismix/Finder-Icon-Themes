//! SVG to PNG rendering with an on-disk cache.

use anyhow::{bail, Context, Result};
use resvg::{tiny_skia, usvg};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

use crate::theme::Theme;

/// 256px is deliberately chosen: on macOS a custom icon is stored per size, and
/// 256px lands on the compressed representation — about 12 KB per file icon and
/// 8 KB per folder icon. 128px is eight times larger (96 KB / 64 KB), 64px and
/// 512px+ are larger too. Do not "optimise" this downwards.
const TARGET_WIDTH: f32 = 256.0;

/// Render a theme icon to a cached PNG and return its path. The cache key covers
/// both the SVG bytes and the target size, so changing the size cannot reuse a
/// stale bitmap.
pub fn png_for(theme: &Theme, source: &str, cache_dir: &Path) -> Result<PathBuf> {
    let svg = theme.svg_bytes(source)?;
    let mut hasher = Sha256::new();
    hasher.update(&svg);
    hasher.update(TARGET_WIDTH.to_le_bytes());
    let file = cache_dir.join(format!("{}.png", hex(&hasher.finalize())));
    if file.is_file() {
        return Ok(file);
    }
    fs::create_dir_all(cache_dir)
        .with_context(|| format!("Cannot create {}", cache_dir.display()))?;
    let tree = usvg::Tree::from_data(&svg, &usvg::Options::default())
        .with_context(|| format!("Cannot parse icon {source}"))?;
    let width = tree.size().width();
    if !(width > 0.0) {
        bail!("Icon {source} has no width");
    }
    let scale = TARGET_WIDTH / width;
    let out_w = (tree.size().width() * scale).round().max(1.0) as u32;
    let out_h = (tree.size().height() * scale).round().max(1.0) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(out_w, out_h).context("Cannot allocate icon bitmap")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let png = pixmap.encode_png().context("Cannot encode icon PNG")?;
    fs::write(&file, png).with_context(|| format!("Cannot write {}", file.display()))?;
    Ok(file)
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}
