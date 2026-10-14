//! macOS custom-icon operations.

use std::path::Path;

pub use imp::{has_custom_icon, icon_fingerprint, set_icon};

/// Does this item already carry a custom icon (ours or anyone else's)?
pub fn exists(path: &Path) -> bool {
    path.exists()
}

#[cfg(target_os = "macos")]
mod imp {
    use anyhow::Result;
    use objc2::rc::Retained;
    use objc2::AnyThread;
    use objc2_app_kit::{NSImage, NSWorkspace, NSWorkspaceIconCreationOptions};
    use objc2_foundation::NSString;
    use sha2::{Digest, Sha256};
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    thread_local! { static IMAGES: RefCell<VecDeque<(PathBuf, Retained<NSImage>)>> = const { RefCell::new(VecDeque::new()) }; }

    /// `NSWorkspace.shared.setIcon(_:forFile:options:)`. `None` clears the icon.
    ///
    /// Each call runs inside an autorelease pool: without one, the objects
    /// AppKit autoreleases in a long-lived process are never drained and RSS
    /// grows without bound across a large sweep.
    pub fn set_icon(path: &Path, icon: Option<&Path>) -> Result<bool> {
        let ok = objc2::rc::autoreleasepool(|_pool| {
            let ns_path = NSString::from_str(&path.to_string_lossy());
            let image = match icon {
                None => None,
                Some(png) => Some(IMAGES.with(|cache| -> Result<Retained<NSImage>> {
                    let mut cache = cache.borrow_mut();
                    if let Some(i) = cache.iter().position(|(p, _)| p == png) {
                        let item = cache.remove(i).unwrap();
                        let image = item.1.clone();
                        cache.push_back(item);
                        return Ok(image);
                    }
                    let ns_icon = NSString::from_str(&png.to_string_lossy());
                    let image = NSImage::initWithContentsOfFile(NSImage::alloc(), &ns_icon)
                        .ok_or_else(|| anyhow::anyhow!("Cannot load icon {}", png.display()))?;
                    if cache.len() == 64 {
                        cache.pop_front();
                    }
                    cache.push_back((png.to_path_buf(), image.clone()));
                    Ok(image)
                })?),
            };
            let workspace = NSWorkspace::sharedWorkspace();
            Ok(workspace.setIcon_forFile_options(
                image.as_deref(),
                &ns_path,
                NSWorkspaceIconCreationOptions::empty(),
            ))
        });
        ok
    }

    /// A custom icon sets the `kHasCustomIcon` Finder flag and, for folders, an
    /// `Icon\r` sibling. Mirrors the check the original Swift helper used.
    pub fn has_custom_icon(path: &Path) -> bool {
        if finder_info_flag(path) {
            return true;
        }
        path.is_dir() && path.join("Icon\r").exists()
    }

    /// Fingerprint only icon payload, not other Finder metadata. Failure is
    /// conservative: callers retain ownership and never clear an unknown icon.
    pub fn icon_fingerprint(path: &Path) -> Result<Option<String>> {
        if !has_custom_icon(path) {
            return Ok(None);
        }
        let resource = if path.is_dir() {
            path.join("Icon\r")
        } else {
            path.to_path_buf()
        };
        let c_path = CString::new(resource.as_os_str().as_bytes())?;
        let attr = c"com.apple.ResourceFork";
        let size = unsafe {
            libc::getxattr(
                c_path.as_ptr(),
                attr.as_ptr(),
                std::ptr::null_mut(),
                0,
                0,
                0,
            )
        };
        anyhow::ensure!(
            (0..=16 * 1024 * 1024).contains(&size),
            "Cannot read icon payload at {}",
            path.display()
        );
        let mut bytes = vec![0u8; size as usize];
        let read = unsafe {
            libc::getxattr(
                c_path.as_ptr(),
                attr.as_ptr(),
                bytes.as_mut_ptr().cast(),
                bytes.len(),
                0,
                0,
            )
        };
        anyhow::ensure!(
            read == size,
            "Icon payload changed while reading {}",
            path.display()
        );
        Ok(Some(format!("{:x}", Sha256::digest(&bytes))))
    }

    fn finder_info_flag(path: &Path) -> bool {
        let Ok(c_path) = CString::new(path.as_os_str().as_bytes()) else {
            return false;
        };
        let attribute = c"com.apple.FinderInfo";
        let mut buffer = [0u8; 32];
        let read = unsafe {
            libc::getxattr(
                c_path.as_ptr(),
                attribute.as_ptr(),
                buffer.as_mut_ptr() as *mut libc::c_void,
                buffer.len(),
                0,
                0,
            )
        };
        if read >= 10 {
            let flags = (u16::from(buffer[8]) << 8) | u16::from(buffer[9]);
            return flags & 0x0400 != 0;
        }
        false
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use anyhow::{bail, Result};
    use std::path::Path;

    pub fn set_icon(_path: &Path, _icon: Option<&Path>) -> Result<bool> {
        bail!("Custom icons require macOS.")
    }

    pub fn icon_fingerprint(_path: &Path) -> Result<Option<String>> {
        Ok(None)
    }

    pub fn has_custom_icon(path: &Path) -> bool {
        path.is_dir() && path.join("Icon\r").exists()
    }
}
