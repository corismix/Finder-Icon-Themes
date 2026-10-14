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
    let memo_key = (cache_dir.to_path_buf(), source.to_string());
    if theme.immutable() {
        if let Some(path) = theme.prepared.borrow().get(&memo_key) {
            if path.is_file() {
                return Ok(path.clone());
            }
        }
    }
    let svg = theme.svg_bytes(source)?;
    let mut hasher = Sha256::new();
    hasher.update(&svg);
    hasher.update(TARGET_WIDTH.to_le_bytes());
    let file = cache_dir.join(format!("{}.png", hex(&hasher.finalize())));
    if theme.immutable() {
        let mut memo = theme.prepared.borrow_mut();
        if memo.len() >= 256 {
            memo.clear();
        }
        memo.insert(memo_key, file.clone());
    }
    if file.is_file() {
        return Ok(file);
    }
    #[cfg(not(feature = "extended-svg"))]
    {
        let text = std::str::from_utf8(&svg).context("SVG must be UTF-8")?;
        anyhow::ensure!(
            !["<text", ":text", "<image", ":image"]
                .iter()
                .any(|tag| text.contains(tag)),
            "Icon {source} requires SVG text/raster support; rebuild with --features extended-svg"
        );
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
    let temp = file.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temp, png).with_context(|| format!("Cannot write {}", temp.display()))?;
    fs::rename(&temp, &file)?;
    thread_local! { static WRITES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
    let trim = WRITES.with(|n| {
        let next = n.get() + 1;
        n.set(next % 64);
        next % 64 == 0
    });
    if trim {
        prune_cache(cache_dir, 1024, 64 * 1024 * 1024)?;
    }
    Ok(file)
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut out, "{byte:02x}").unwrap();
    }
    out
}

/// Only disposable content-addressed PNGs are eligible; ownership journals,
/// backups, symlinks and arbitrary user files are never removed.
pub fn prune_cache(cache_dir: &Path, max_entries: usize, max_bytes: u64) -> Result<()> {
    if !cache_dir.is_dir() {
        return Ok(());
    }
    let mut items = Vec::new();
    for entry in fs::read_dir(cache_dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.len() != 68
            || !name.ends_with(".png")
            || !name[..64].bytes().all(|b| b.is_ascii_hexdigit())
        {
            continue;
        }
        let meta = entry.metadata()?;
        if entry.file_type()?.is_symlink() || !meta.is_file() {
            continue;
        }
        items.push((meta.modified()?, meta.len(), path));
    }
    let mut bytes: u64 = items.iter().map(|i| i.1).sum();
    let mut count = items.len();
    items.sort_by_key(|i| i.0);
    for (_, size, path) in items {
        if count <= max_entries && bytes <= max_bytes {
            break;
        }
        fs::remove_file(path)?;
        count -= 1;
        bytes -= size;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_shipped_resource_renders_at_256() {
        let cache = std::env::temp_dir().join(format!("fit-render-all-{}", std::process::id()));
        let theme = crate::theme::load("material", None).unwrap();
        for source in theme.manifest.icon_definitions.values() {
            let png = png_for(&theme, &source.icon_path, &cache).unwrap();
            let bytes = fs::read(png).unwrap();
            assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 256);
        }
        fs::remove_dir_all(cache).unwrap();
    }
    #[test]
    fn cache_eviction_preserves_user_files_and_symlinks() {
        let dir = std::env::temp_dir().join(format!("fit-cache-bound-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        for i in 0..5 {
            fs::write(dir.join(format!("{i:064x}.png")), [0; 10]).unwrap();
        }
        fs::write(dir.join("keep.txt"), "user data").unwrap();
        std::os::unix::fs::symlink(dir.join("keep.txt"), dir.join(format!("{:064x}.png", 999)))
            .unwrap();
        prune_cache(&dir, 2, 20).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("keep.txt")).unwrap(),
            "user data"
        );
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 4); // 2 PNGs, user file, symlink
        fs::remove_dir_all(dir).unwrap();
    }
}
