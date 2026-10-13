//! macOS custom-icon operations.

use std::path::Path;

#[cfg(target_os = "macos")]
pub use imp::{has_custom_icon, set_icon};

/// Does this item already carry a custom icon (ours or anyone else's)?
pub fn exists(path: &Path) -> bool {
    path.exists()
}

#[cfg(target_os = "macos")]
mod imp {
    use anyhow::Result;
    use objc2::AnyThread;
    use objc2_app_kit::{NSImage, NSWorkspace, NSWorkspaceIconCreationOptions};
    use objc2_foundation::NSString;
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    /// `NSWorkspace.shared.setIcon(_:forFile:options:)`. `None` clears the icon.
    ///
    /// Each call runs inside an autorelease pool: without one, the objects
    /// AppKit autoreleases in a long-lived process are never drained and RSS
    /// grows without bound across a large sweep.
    pub fn set_icon(path: &Path, icon: Option<&Path>) -> Result<bool> {
        let ok = objc2::rc::autoreleasepool(|_pool| {
            let ns_path = NSString::from_str(&path.to_string_lossy());
            let image = icon.and_then(|png| {
                let ns_icon = NSString::from_str(&png.to_string_lossy());
                NSImage::initWithContentsOfFile(NSImage::alloc(), &ns_icon)
            });
            let workspace = NSWorkspace::sharedWorkspace();
            workspace.setIcon_forFile_options(
                image.as_deref(),
                &ns_path,
                NSWorkspaceIconCreationOptions::empty(),
            )
        });
        Ok(ok)
    }

    /// A custom icon sets the `kHasCustomIcon` Finder flag and, for folders, an
    /// `Icon\r` sibling. Mirrors the check the original Swift helper used.
    pub fn has_custom_icon(path: &Path) -> bool {
        if finder_info_flag(path) {
            return true;
        }
        path.is_dir() && path.join("Icon\r").exists()
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

    pub fn has_custom_icon(path: &Path) -> bool {
        path.is_dir() && path.join("Icon\r").exists()
    }
}
