use anyhow::{bail, Result};
use clap::{Args, Parser, Subcommand};
use std::path::{Path, PathBuf};

use finder_icon_themes::apply::{self, ensure_something_to_do};
use finder_icon_themes::config::{self, Config};
use finder_icon_themes::state::{read_state, state_paths, MutationLock};
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
    /// Remove verified managed icons excluded by the selected policy.
    Reconcile(TargetArgs),
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
    #[arg(long)]
    specific_matches_only: bool,
    /// Literal name, root-relative subtree or absolute path (repeatable).
    #[arg(long)]
    exclude: Vec<PathBuf>,
    #[arg(long)]
    respect_gitignore: bool,
}

#[derive(Subcommand)]
enum ServiceAction {
    Install,
    Uninstall,
    Start,
    Stop,
    Status,
}

fn expand(folder: &Path) -> PathBuf {
    let raw = folder.to_string_lossy();
    if raw == "~" {
        finder_icon_themes::state::home()
    } else if let Some(rest) = raw.strip_prefix("~/") {
        finder_icon_themes::state::home().join(rest)
    } else {
        folder.to_path_buf()
    }
}

fn resolve(folder: &Path) -> PathBuf {
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
    config.specific_matches_only |= args.specific_matches_only;
    config.respect_gitignore |= args.respect_gitignore;
    config.exclude.extend(args.exclude.iter().cloned());
    config.validate()?;
    ensure_something_to_do(&config)?;
    Ok(config)
}

fn load_theme(config: &Config) -> Result<theme::Theme> {
    theme::load(&config.theme, theme::override_dir().as_deref())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if matches!(&cli.command, Command::Watch) {
        service::bound_log()?;
    }
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
            let mut candidates = apply::candidates_for(&root, &theme, &config)?;
            candidates.sort_by(|a, b| a.relative.cmp(&b.relative));
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
            let excluded = apply::excluded_keys(&root, &theme, &config, &state)?;
            println!("Previously managed icons excluded by this policy: {}. Apply leaves them in place; use reconcile to remove verified owned icons.", excluded.len());
            for key in excluded.iter().take(15) {
                println!("  excluded  {key}");
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
            let _guard = MutationLock::acquire(&paths.base)?;
            let mut state = read_state(&paths.state)?;
            let report = { apply::apply(&root, &theme, &config, &paths.cache, &mut state)? };
            println!(
                "Applied: {}; unchanged: {}; pre-existing custom icons skipped: {}; replaced paths skipped: {}; failed: {}.",
                report.applied, report.unchanged, report.custom_skipped, report.stale_skipped, report.failed
            );
            println!("Undo: finder-icon-themes undo {}", root.display());
            if report.failed > 0 {
                std::process::exit(1);
            }
        }
        Command::Reconcile(args) => {
            let config = with_overrides(config, &args)?;
            let root = resolve(&args.folder);
            require_dir(&root)?;
            let theme = load_theme(&config)?;
            let _guard = MutationLock::acquire(&paths.base)?;
            let mut state = read_state(&paths.state)?;
            let report = apply::reconcile(&root, &theme, &config, &mut state)?;
            println!("Excluded owned icons removed: {}; stale records: {}; ambiguous or failed removals retained: {}.", report.removed, report.stale, report.failed);
            if report.failed > 0 {
                std::process::exit(1);
            }
        }
        Command::Undo(args) => {
            let root = resolve(&args.folder);
            let _guard = MutationLock::acquire(&paths.base)?;
            let mut state = read_state(&paths.state)?;
            let report = { apply::undo(&root, &mut state)? };
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
            // Stop before taking the mutation lock, so the daemon cannot
            // reapply during cleanup. Isolated HOME has no service plist.
            if service::is_installed() {
                service::stop()?;
            }
            let _guard = MutationLock::acquire(&paths.base)?;
            let mut state = read_state(&paths.state)?;
            let report = apply::clean(&mut state)?;
            if report.failed > 0 {
                bail!(
                    "Removed {} icons; {} ambiguous or failed removals retained in the journal",
                    report.removed,
                    report.failed
                );
            }
            service::uninstall()?;
            if paths.cache.exists() {
                std::fs::remove_dir_all(&paths.cache)?;
            }
            if paths.state.exists() {
                std::fs::remove_file(&paths.state)?;
            }
            println!(
                "Removed {} managed icons, the global exclude rule, cache, service and state.",
                report.removed
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
