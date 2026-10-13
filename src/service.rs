//! macOS LaunchAgent management for the background watcher.

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use crate::state::home;

pub const LABEL: &str = "com.finder-icon-themes.watch";

pub fn plist_path() -> PathBuf {
    home().join(format!("Library/LaunchAgents/{LABEL}.plist"))
}

fn log_path() -> PathBuf {
    home().join(".local/state/finder-icon-themes/watch.log")
}

fn uid() -> u32 {
    unsafe { libc::getuid() }
}

fn domain() -> String {
    format!("gui/{}", uid())
}

fn service_target() -> String {
    format!("{}/{LABEL}", domain())
}

fn current_exe() -> Result<PathBuf> {
    std::env::current_exe().context("Cannot locate the running binary")
}

pub fn is_installed() -> bool {
    plist_path().is_file()
}

/// Run `launchctl` and capture its output, so its diagnostics never leak into
/// the user's terminal as unhandled noise.
fn launchctl(args: &[&str]) -> Result<Output> {
    Command::new("launchctl")
        .args(args)
        .output()
        .with_context(|| format!("Cannot run launchctl {}", args.join(" ")))
}

fn launchctl_ok(args: &[&str]) -> bool {
    launchctl(args)
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn is_loaded() -> bool {
    launchctl_ok(&["print", &service_target()])
}

fn message(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        stderr
    } else {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn install() -> Result<PathBuf> {
    if uid() == 0 {
        bail!(
            "Run without sudo. A LaunchAgent is per-user; installing as root targets the wrong launchd domain."
        );
    }
    let exe = current_exe()?;
    let plist = plist_path();
    let log = log_path();
    if let Some(parent) = plist.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Cannot create {}", parent.display()))?;
    }
    if let Some(parent) = log.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let contents = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>watch</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>10</integer>
  <key>StandardOutPath</key><string>{log}</string>
  <key>StandardErrorPath</key><string>{log}</string>
</dict>
</plist>
"#,
        label = LABEL,
        exe = xml_escape(&exe.to_string_lossy()),
        log = xml_escape(&log.to_string_lossy()),
    );
    fs::write(&plist, contents).with_context(|| format!("Cannot write {}", plist.display()))?;

    // Replace a stale registration from an earlier install, quietly.
    if is_loaded() {
        bootout();
        std::thread::sleep(Duration::from_millis(300));
    }
    // Clear any earlier `stop` (which disables the job).
    let _ = launchctl(&["enable", &service_target()]);
    bootstrap(&plist)?;
    let _ = launchctl(&["kickstart", "-k", &service_target()]);
    Ok(plist)
}

/// Best-effort unload of the job from launchd.
fn bootout() {
    let _ = launchctl(&["bootout", &service_target()]);
    let _ = launchctl(&["bootout", &domain(), &plist_path().to_string_lossy()]);
}

fn bootstrap(plist: &Path) -> Result<()> {
    let output = launchctl(&["bootstrap", &domain(), &plist.to_string_lossy()])?;
    if output.status.success() {
        return Ok(());
    }
    let detail = message(&output);
    let already = detail.contains("already")
        || detail.contains("Input/output error")
        || detail.contains("37:")
        || detail.contains("5:");
    if already {
        return Ok(());
    }
    bail!("launchctl bootstrap failed: {detail}");
}

pub fn uninstall() -> Result<bool> {
    guard_non_root()?;
    let plist = plist_path();
    // If there is no plist in *this* home, leave launchd alone. `launchctl`
    // only knows the user's real domain, so an unconditional `bootout` would
    // reach outside an isolated (test) HOME and stop a real running service.
    if !plist.is_file() {
        return Ok(false);
    }
    bootout();
    // Clear a `stop` disable so a later install is not blocked.
    let _ = launchctl(&["enable", &service_target()]);
    fs::remove_file(&plist).with_context(|| format!("Cannot remove {}", plist.display()))?;
    Ok(true)
}

pub fn start() -> Result<()> {
    guard_non_root()?;
    if !is_installed() {
        bail!("Service is not installed. Run `service install` first.");
    }
    let _ = launchctl(&["enable", &service_target()]);
    if !is_loaded() {
        bootstrap(&plist_path())?;
    }
    if !launchctl_ok(&["kickstart", "-k", &service_target()]) {
        bail!("Could not start {LABEL}.");
    }
    Ok(())
}

/// A `kill` alone is not enough: `KeepAlive` respawns the process. Disable the
/// job so launchd leaves it down, then unload it.
pub fn stop() -> Result<()> {
    guard_non_root()?;
    if !is_installed() && !is_loaded() {
        bail!("Service is not installed.");
    }
    let _ = launchctl(&["disable", &service_target()]);
    bootout();
    Ok(())
}

fn guard_non_root() -> Result<()> {
    if uid() == 0 {
        bail!("Run without sudo; LaunchAgents are per-user.");
    }
    Ok(())
}

pub fn status() -> Result<bool> {
    Ok(is_loaded())
}
