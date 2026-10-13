use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

use finder_icon_themes::apply::{self, ensure_something_to_do};
use finder_icon_themes::config::{self, Config};
use finder_icon_themes::state::{read_state, state_paths};
use finder_icon_themes::theme;
use finder_icon_themes::{service, watch};

#[derive(Parser)]
#[command(
    name = "finder-icon-themes",
    version,
    about = "Automatic macOS custom icons for git repositories"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the installed icon themes.
    Themes,
    /// Show what would change, without writing anything.
    Preview(TargetArgs),
    /// Set icons on recognised files and folders.
    Apply(TargetArgs),
    /// Remove only the icons this tool installed under a folder.
    Undo(TargetArgs),
    /// Show the icons recorded for a folder.
    Status(TargetArgs),
    /// Watch configured roots and keep repository icons applied.
    Watch,
    /// Manage the background LaunchAgent.
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Remove all managed icons, the global exclude rule, the service and state.
    Clean,
}

#[derive(Args)]
struct TargetArgs {
    folder: PathBuf,
    #[arg(long)]
    theme: Option<String>,
    #[arg(long)]
    files_only: bool,
    #[arg(long)]
    folders_only: bool,
}

#[derive(Subcommand)]
enum ServiceAction {
    Install,
    Uninstall,
    Start,
    Stop,
    Status,
}

fn expand(folder: &PathBuf) -> PathBuf {
    let raw = folder.to_string_lossy();
    if raw == "~" {
        finder_icon_themes::state::home()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        finder_icon_themes::state::home().join(rest)
    } else {
        folder.clone()
    }
}

fn resolve(folder: &PathBuf) -> PathBuf {
    let path = expand(folder);
    std::fs::canonicalize(&path).unwrap_or(path)
}

fn with_overrides(mut config: Config, args: &TargetArgs) -> Result<Config> {
    if let Some(name) = &args.theme {
        config.theme = name.clone();
    }
    if args.files_only {
        config.folders = false;
    }
    if args.folders_only {
        config.files = false;
    }
    ensure_something_to_do(&config)?;
    Ok(config)
}

fn load_theme(config: &Config) -> Result<theme::Theme> {
    theme::load(&config.theme, theme::override_dir().as_deref())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = config::load()?;
    let paths = state_paths();

    match cli.command {
        Command::Themes => {
            println!("Available themes:");
            for (name, title) in theme::list() {
                println!("  {name:<12} {title}");
            }
        }
        Command::Preview(args) => {
            let config = with_overrides(config, &args)?;
            let root = resolve(&args.folder);
            require_dir(&root)?;
            let theme = load_theme(&config)?;
            let state = read_state(&paths.state)?;
            let candidates = apply::candidates_for(&root, &theme, &config)?;
            let affected = apply::affected(&candidates, &state);
            println!(
                "{}: {} recognised or fallback icons under {}.",
                theme.title,
                candidates.len(),
                root.display()
            );
            println!(
                "Already recorded with matching icon: {}.",
                candidates.len() - affected.len()
            );
            for item in affected.iter().take(45) {
                let kind = if item.directory { "folder" } else { "file  " };
                println!("  {kind}  {}  →  {}", item.relative, item.id);
            }
            if affected.len() > 45 {
                println!("  ... {} more", affected.len() - 45);
            }
            println!(
                "Preview only. To apply: finder-icon-themes apply {}",
                root.display()
            );
        }
        Command::Apply(args) => {
            let config = with_overrides(config, &args)?;
            let root = resolve(&args.folder);
            require_dir(&root)?;
            let theme = load_theme(&config)?;
            let mut state = read_state(&paths.state)?;
            acquire_lock(&paths.base)?;
            let report = {
                let _guard = LockGuard;
                apply::apply(&root, &theme, &config, &paths.cache, &mut state)?
            };
            println!(
                "Applied: {}; unchanged: {}; pre-existing custom icons skipped: {}; replaced paths skipped: {}; failed: {}.",
                report.applied, report.unchanged, report.custom_skipped, report.stale_skipped, report.failed
            );
            println!("Undo: finder-icon-themes undo {}", root.display());
            if report.failed > 0 {
                std::process::exit(1);
            }
        }
        Command::Undo(args) => {
            let root = resolve(&args.folder);
            let mut state = read_state(&paths.state)?;
            acquire_lock(&paths.base)?;
            let report = {
                let _guard = LockGuard;
                apply::undo(&root, &mut state)?
            };
            println!(
                "Restored defaults: {}; failed: {}{}.",
                report.removed,
                report.failed,
                if report.stale > 0 {
                    format!("; {} moved/replaced entries forgotten", report.stale)
                } else {
                    String::new()
                }
            );
            if report.failed > 0 {
                std::process::exit(1);
            }
        }
        Command::Status(args) => {
            let root = resolve(&args.folder);
            let state = read_state(&paths.state)?;
            let entries = apply::status(&root, &state);
            println!("{} managed icons under {}", entries.len(), root.display());
            for (relative, directory, id) in entries.iter().take(30) {
                println!(
                    "  {} {} ({id})",
                    if *directory { "📁" } else { "📄" },
                    relative
                );
            }
            if entries.len() > 30 {
                println!("  ... {} more", entries.len() - 30);
            }
        }
        Command::Watch => {
            let theme = load_theme(&config)?;
            watch::run(&config, &theme)?;
        }
        Command::Service { action } => match action {
            ServiceAction::Install => {
                let path = config::write_template_if_missing()?;
                let plist = service::install()?;
                println!("Installed {}.", plist.display());
                println!("Config: {}", path.display());
                println!("The watcher is running. Icons persist if it stops.");
            }
            ServiceAction::Uninstall => {
                let removed = service::uninstall()?;
                println!(
                    "{}",
                    if removed {
                        "Service uninstalled."
                    } else {
                        "Service was not installed."
                    }
                );
            }
            ServiceAction::Start => {
                service::start()?;
                println!("Service started.");
            }
            ServiceAction::Stop => {
                service::stop()?;
                println!("Service stopped. Icons are left in place.");
            }
            ServiceAction::Status => {
                println!(
                    "{}",
                    if service::status()? {
                        "Service is running."
                    } else {
                        "Service is not running."
                    }
                );
            }
        },
        Command::Clean => {
            let mut state = read_state(&paths.state)?;
            let removed = apply::clean(&mut state)?;
            let _ = service::uninstall();
            let _ = std::fs::remove_file(&paths.state);
            println!(
                "Removed {removed} managed icons, the global exclude rule, the service and state."
            );
        }
    }
    Ok(())
}

fn require_dir(root: &std::path::Path) -> Result<()> {
    if !root.exists() {
        bail!("Directory not found: {}", root.display());
    }
    if !root.is_dir() {
        bail!("Not a directory: {}", root.display());
    }
    Ok(())
}

fn acquire_lock(base: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(base).with_context(|| format!("Cannot create {}", base.display()))?;
    let lock = base.join("journal.lock");
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock)
        .map(|mut file| {
            std::io::Write::write_all(&mut file, std::process::id().to_string().as_bytes())
        })
        .with_context(|| {
            format!(
                "Another sync may be running. Lock exists at {}",
                lock.display()
            )
        })?
        .with_context(|| format!("Cannot write lock at {}", lock.display()))?;
    Ok(())
}

struct LockGuard;

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(state_paths().base.join("journal.lock"));
    }
}
