//! End-to-end checks driven through the built binary.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_finder-icon-themes");

struct Sandbox {
    dir: PathBuf,
}

impl Sandbox {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "fit-it-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        Sandbox { dir }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.join(relative)
    }

    fn theme_dir(&self) -> PathBuf {
        let dir = self.path("theme");
        fs::create_dir_all(dir.join("dist")).unwrap();
        fs::create_dir_all(dir.join("icons")).unwrap();
        let svg = |color: &str| {
            format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" fill="{color}"/></svg>"##
            )
        };
        for (name, color) in [
            ("file", "#888888"),
            ("folder", "#4488cc"),
            ("folder-git", "#cc4444"),
            ("typescript", "#2255cc"),
            ("npm", "#cc2222"),
        ] {
            fs::write(dir.join("icons").join(format!("{name}.svg")), svg(color)).unwrap();
        }
        let manifest = serde_json::json!({
            "file": "file",
            "folder": "folder",
            "fileNames": { "package.json": "npm" },
            "fileExtensions": { "ts": "typescript" },
            "folderNames": { ".git": "folder-git" },
            "iconDefinitions": {
                "file": { "iconPath": "../icons/file.svg" },
                "folder": { "iconPath": "../icons/folder.svg" },
                "folder-git": { "iconPath": "../icons/folder-git.svg" },
                "typescript": { "iconPath": "../icons/typescript.svg" },
                "npm": { "iconPath": "../icons/npm.svg" }
            }
        });
        fs::write(dir.join("dist/material-icons.json"), manifest.to_string()).unwrap();
        dir
    }

    fn home(&self) -> PathBuf {
        self.dir.join("home")
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        self.command().args(args).output().unwrap()
    }

    fn command(&self) -> Command {
        let mut command = Command::new(BIN);
        command
            .env("FINDER_ICON_THEMES_HOME", self.path("state"))
            .env("FINDER_ICON_THEMES_THEME_DIR", self.theme_dir())
            .env("FINDER_ICON_THEMES_CONFIG", self.path("config.toml"))
            .env("HOME", self.home())
            .env("XDG_CONFIG_HOME", self.path("xdg"));
        command
    }

    /// The same command, but with the on-disk theme override removed so the
    /// theme embedded in the binary is used.
    fn command_embedded(&self) -> Command {
        let mut command = self.command();
        command.env_remove("FINDER_ICON_THEMES_THEME_DIR");
        command
    }

    fn write_config(&self, watch_roots: &[PathBuf]) {
        let roots = watch_roots
            .iter()
            .map(|r| format!("{:?}", r.to_string_lossy()))
            .collect::<Vec<_>>()
            .join(", ");
        let text = format!("watch_roots = [{roots}]\ntheme = \"material\"\n");
        fs::write(self.path("config.toml"), text).unwrap();
    }

    fn journal(&self) -> serde_json::Value {
        let text = fs::read_to_string(self.path("state/state.json")).unwrap_or_default();
        if text.is_empty() {
            return serde_json::Value::Null;
        }
        serde_json::from_str(&text).unwrap()
    }

    fn global_ignore(&self) -> PathBuf {
        self.path("xdg/git/ignore")
    }

    fn git_repo(&self, relative: &str) -> PathBuf {
        let repo = self.path(relative);
        fs::create_dir_all(&repo).unwrap();
        let status = Command::new("git")
            .args(["init", "-q"])
            .arg(&repo)
            .status()
            .unwrap();
        assert!(status.success());
        repo
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn preview_writes_nothing() {
    let sandbox = Sandbox::new("preview");
    let repo = sandbox.git_repo("project");
    fs::write(repo.join("main.ts"), "export {}").unwrap();
    fs::write(repo.join("package.json"), "{}").unwrap();

    let output = sandbox.run(&["preview", repo.to_str().unwrap()]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("main.ts"), "{text}");
    assert!(text.contains("package.json"), "{text}");

    assert!(
        !sandbox.path("state/state.json").exists(),
        "preview must not write a journal"
    );
    assert!(
        !repo.join("Icon\r").exists(),
        "preview must not write folder icons"
    );
}

#[test]
fn apply_icons_a_repo_and_uses_the_global_exclude() {
    let sandbox = Sandbox::new("apply");
    let repo = sandbox.git_repo("project");
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(repo.join("package.json"), "{}").unwrap();
    fs::write(repo.join("src/main.ts"), "export {}").unwrap();

    let output = sandbox.run(&["apply", repo.to_str().unwrap()]);
    assert!(output.status.success(), "{}", stderr(&output));

    // Icons really applied (the folder you point at is left alone; its contents are themed).
    assert!(
        repo.join("src/Icon\r").exists(),
        "subfolder should carry an Icon file"
    );
    assert!(
        repo.join(".git/Icon\r").exists(),
        "the .git folder gets a themed icon"
    );

    // Global exclude got the rule; the repository's local exclude did not.
    let ignore = fs::read_to_string(sandbox.global_ignore()).unwrap_or_default();
    assert!(
        ignore.contains("Icon?"),
        "global ignore should contain Icon?: {ignore:?}"
    );
    let local = repo.join(".git/info/exclude");
    let local_text = fs::read_to_string(&local).unwrap_or_default();
    assert!(
        !local_text.contains("Icon?"),
        "repo-local exclude must stay untouched: {local_text:?}"
    );

    // `git status` stays clean of the icon metadata.
    let status = Command::new("git")
        .args([
            "-C",
            repo.to_str().unwrap(),
            "status",
            "--porcelain",
            "--untracked-files=all",
        ])
        .env("HOME", sandbox.home())
        .env("XDG_CONFIG_HOME", sandbox.path("xdg"))
        .output()
        .unwrap();
    let status_text = stdout(&status);
    assert!(
        !status_text.to_lowercase().contains("icon"),
        "icons leaked into git status: {status_text:?}"
    );

    // Journal records entries with the global exclude ownership.
    let journal = sandbox.journal();
    assert_eq!(journal["schema"], 3);
    assert!(journal["entries"].as_object().unwrap().len() >= 3);
    assert_eq!(journal["excludes"]["go"], true);
}

#[test]
fn opt_out_marker_skips_a_folder_and_its_subtree() {
    let sandbox = Sandbox::new("optout");
    let repo = sandbox.git_repo("project");
    fs::create_dir_all(repo.join("keep")).unwrap();
    fs::create_dir_all(repo.join("skip/deep")).unwrap();
    fs::write(repo.join("keep/a.ts"), "").unwrap();
    fs::write(repo.join("skip").join(".no-finder-icons"), "").unwrap();
    fs::write(repo.join("skip/b.ts"), "").unwrap();
    fs::write(repo.join("skip/deep/c.ts"), "").unwrap();

    let output = sandbox.run(&["apply", repo.to_str().unwrap()]);
    assert!(output.status.success(), "{}", stderr(&output));

    assert!(repo.join("keep/Icon\r").exists());
    assert!(
        !repo.join("skip/Icon\r").exists(),
        "opted-out folder must not be iconised"
    );
    assert!(
        !repo.join("skip/deep/Icon\r").exists(),
        "opted-out subtree must not be iconised"
    );
    let journal_text = fs::read_to_string(sandbox.path("state/state.json")).unwrap();
    assert!(
        !journal_text.contains("/skip/"),
        "no journal entries under opted-out folder"
    );
}

#[test]
fn undo_removes_only_managed_icons_and_the_global_rule() {
    let sandbox = Sandbox::new("undo");
    let repo = sandbox.git_repo("project");
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(repo.join("src/main.ts"), "").unwrap();

    let applied = sandbox.run(&["apply", repo.to_str().unwrap()]);
    assert!(applied.status.success(), "{}", stderr(&applied));
    assert!(repo.join("src/Icon\r").exists());

    let undone = sandbox.run(&["undo", repo.to_str().unwrap()]);
    assert!(undone.status.success(), "{}", stderr(&undone));
    assert!(
        !repo.join("src/Icon\r").exists(),
        "undo should remove nested folder icons"
    );
    assert!(
        !repo.join(".git/Icon\r").exists(),
        "undo should remove the .git folder icon"
    );

    let ignore = fs::read_to_string(sandbox.global_ignore()).unwrap_or_default();
    assert!(
        !ignore.contains("Icon?"),
        "global rule should be taken back out: {ignore:?}"
    );
    let journal = sandbox.journal();
    assert!(journal["entries"].as_object().unwrap().is_empty());
}

#[test]
fn clean_removes_state_icons_and_exclude() {
    let sandbox = Sandbox::new("clean");
    let repo = sandbox.git_repo("project");
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(repo.join("package.json"), "{}").unwrap();
    let applied = sandbox.run(&["apply", repo.to_str().unwrap()]);
    assert!(applied.status.success(), "{}", stderr(&applied));
    assert!(repo.join("src/Icon\r").exists());

    let cleaned = sandbox.run(&["clean"]);
    assert!(cleaned.status.success(), "{}", stderr(&cleaned));
    assert!(!repo.join("src/Icon\r").exists());
    assert!(
        !sandbox.path("state/state.json").exists(),
        "clean removes the journal"
    );
    let ignore = fs::read_to_string(sandbox.global_ignore()).unwrap_or_default();
    assert!(
        !ignore.contains("Icon?"),
        "clean removes the global rule: {ignore:?}"
    );
}

#[test]
fn repo_helper_finds_the_repository_for_nested_paths() {
    // Guards the walk-up the daemon uses to map changed paths to repositories.
    let sandbox = Sandbox::new("repohelper");
    let repo = sandbox.git_repo("project");
    fs::create_dir_all(repo.join("a/b")).unwrap();
    use finder_icon_themes::git::repo_for;
    assert_eq!(repo_for(&repo.join("a/b/main.ts")), Some(repo.clone()));
    assert!(repo_for(Path::new("/nope/nothing/here")).is_none());
}

#[test]
fn daemon_iconises_a_repo_created_under_a_watch_root() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let sandbox = Sandbox::new("daemon");
    let root = sandbox.path("code");
    fs::create_dir_all(&root).unwrap();
    sandbox.write_config(&[root.clone()]);

    let mut child = sandbox
        .command()
        .arg("watch")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    std::thread::sleep(Duration::from_millis(1500));
    let repo = sandbox.git_repo("code/fresh");
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(repo.join("src/main.ts"), "export {}").unwrap();

    let target = repo.join("src/Icon\r");
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline && !target.exists() {
        std::thread::sleep(Duration::from_millis(250));
    }
    let detected = target.exists();
    let _ = child.kill();
    let _ = child.wait();
    assert!(
        detected,
        "daemon should iconise a repo created under a watch root"
    );
    assert!(target.exists(), "icons persist after the daemon stops");
}

fn png_width(bytes: &[u8]) -> u32 {
    // PNG signature (8) + length (4) + "IHDR" (4) then width.
    u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]])
}

#[test]
fn embedded_theme_applies_without_any_theme_files_and_renders_at_256() {
    let sandbox = Sandbox::new("embedded");
    let repo = sandbox.git_repo("project");
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(repo.join("package.json"), "{}").unwrap();
    fs::write(repo.join("src/main.ts"), "export {}").unwrap();

    // No FINDER_ICON_THEMES_THEME_DIR: the theme must come from the binary.
    let preview = sandbox
        .command_embedded()
        .args(["preview", repo.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(preview.status.success(), "{}", stderr(&preview));
    let text = stdout(&preview);
    assert!(
        text.contains("nodejs"),
        "embedded material theme missing: {text}"
    );
    assert!(
        text.contains("folder-src"),
        "embedded material theme missing: {text}"
    );

    let applied = sandbox
        .command_embedded()
        .args(["apply", repo.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(applied.status.success(), "{}", stderr(&applied));
    assert!(
        repo.join("src/Icon\r").exists(),
        "embedded theme should still apply icons"
    );

    // Rendered bitmaps are 256px wide (macOS stores that size most compactly).
    let cache = sandbox.path("state/cache");
    let mut checked = 0;
    for entry in fs::read_dir(&cache).unwrap().flatten() {
        let path = entry.path();
        if path.extension().map(|e| e == "png").unwrap_or(false) {
            let bytes = fs::read(&path).unwrap();
            assert_eq!(png_width(&bytes), 256, "{}", path.display());
            checked += 1;
        }
    }
    assert!(checked > 0, "expected rendered PNGs in the cache");
}

#[test]
fn a_repo_larger_than_the_old_limit_applies_in_full() {
    let sandbox = Sandbox::new("nolimit");
    let repo = sandbox.git_repo("project");
    fs::create_dir_all(repo.join("many")).unwrap();
    for i in 0..1300 {
        fs::write(repo.join("many").join(format!("file{i}.ts")), "x").unwrap();
    }

    let output = sandbox.run(&["apply", repo.to_str().unwrap()]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    let applied: usize = text
        .split("Applied: ")
        .nth(1)
        .and_then(|rest| rest.split(';').next())
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(0);
    assert!(applied >= 1300, "expected all 1300 icons, got: {text}");

    let journal = sandbox.journal();
    assert!(journal["entries"].as_object().unwrap().len() >= 1300);
}
