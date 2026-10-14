# Finder Icon Themes

Real file and folder icons in **macOS Finder** — and any other browser that honours macOS custom icons — using the artwork and associations from VS Code icon themes. The built-in theme is **[Material Icon Theme](https://github.com/material-extensions/vscode-material-icon-theme)**, with its real SVGs, filename/extension associations, named folders, and generic fallbacks.

A single Rust binary with the theme embedded — no dependencies at runtime. Point it at a folder for a one-off apply, or install the background service: it watches your code roots, finds every git repository, and keeps their icons applied automatically. Icons are filesystem metadata, so they show with no per-app theme switch. The tool does **not** change file-type associations, Quick Look previews, or system icon themes.

## Requirements

- macOS
- A Rust toolchain (`rustup`) to build, and `git`

## Install

```sh
./setup.command
```

This builds `finder-icon-themes`, installs it to `~/.local/bin/`, and removes the build tree. The Material theme is compiled into the binary, so there is nothing else to fetch.

## Usage

Read-only preview, then apply, then undo if you change your mind:

```sh
~/.local/bin/finder-icon-themes preview ~/Code/my-project
~/.local/bin/finder-icon-themes apply   ~/Code/my-project
~/.local/bin/finder-icon-themes undo    ~/Code/my-project
```

| Command | Effect |
| --- | --- |
| `themes` | List the installed icon themes. |
| `preview <folder>` | Show what would change, without writing anything. |
| `apply <folder>` | Set icons on recognised files and folders. |
| `reconcile <folder>` | Remove verified owned icons excluded by the selected policy; retain ambiguous/failed removals. |
| `undo <folder>` | Remove **only** the icons this tool installed under that folder. |
| `status <folder>` | List the icons currently recorded for that folder. |
| `watch` | Run the watcher in the foreground. |
| `service install\|uninstall\|start\|stop\|status` | Manage the background LaunchAgent. |
| `clean` | Remove all managed icons, the global exclude rule, the service and state. |

Options for `preview`, `apply` and `reconcile`:

| Option | Meaning |
| --- | --- |
| `--theme NAME` | Icon theme to use (default `material`). |
| `--files-only` | Leave folder icons alone. |
| `--folders-only` | Select folders only. |
| `--specific-matches-only` | Select name/extension matches, without generic fallbacks. |
| `--exclude PATH` | Exclude a literal name or subtree; repeatable. |
| `--respect-gitignore` | Skip Git-ignored untracked items, preserving tracked files. |

Space-saving policies are **off by default**. Narrowing a policy stops future application; it does not silently remove existing icons. Preview the same policy first, then explicitly reconcile:

```sh
finder-icon-themes preview   ~/Code/my-project --folders-only --specific-matches-only --exclude generated
finder-icon-themes reconcile ~/Code/my-project --folders-only --specific-matches-only --exclude generated
```

Exclusions are literal paths, not globs: `generated` matches that name at any depth; `src/generated` is relative to the selected repository/folder; absolute paths exclude that subtree. Discovery also applies exclusions relative to each watch root. Git evaluates ignore rules, including negations, global excludes, worktrees and nested repositories/submodules. New untracked source files remain eligible unless ignored. Built-in dependency exclusions always apply. Each scan is bounded at 120,000 inspected entries.

## Automatic mode

`service install` registers a per-user LaunchAgent that watches the folders in your config file and applies icons to every git repository it finds — on start, then incrementally for affected paths/subtrees. Events are coalesced after 500ms of quiet, with a five-second maximum debounce. Excluded build contents and Finder metadata produce no icon/journal work. Queue overflow or missed-event notifications trigger conservative reconciliation.

```sh
~/.local/bin/finder-icon-themes service install
~/.local/bin/finder-icon-themes service status
~/.local/bin/finder-icon-themes service stop      # icons stay in place
~/.local/bin/finder-icon-themes service uninstall
```

Config lives at `~/.config/finder-icon-themes/config.toml`:

```toml
watch_roots = ["~/Code"]
theme = "material"
files = true
folders = true
specific_matches_only = false
exclude = []
respect_gitignore = false
```

Nothing outside `watch_roots` is ever touched. Icons are never removed when the service stops, quits, or crashes — removal is always explicit (`undo`, `reconcile`, `clean`).

## How it chooses icons

Matching follows VS Code icon-theme rules, in this order:

- **Files:** exact filename (`package.json`) → longest extension match (`config.test.ts` → `test.ts` → `ts`) → dotfile extension (`.env.local`) → the theme's generic file icon.
- **Folders:** the folder's own name (`.git`, `src`) → the theme's generic folder icon.

Anything the theme has no specific icon for still gets its generic file or folder icon, so the tree stays consistent rather than patchy. Icons are rendered at 256px: on macOS that is the size whose custom-icon data is stored compressed (~12 KB per file, ~8 KB per folder), several times smaller than other common sizes.

## What it skips

- **Pre-existing custom icons are never overwritten.** If you — or another app — set an icon on a file, the tool leaves it alone and manages only icons it installed itself.
- **Opt out per folder:** drop a `.no-finder-icons` file in a folder and that folder and its whole subtree are left alone.
- **Symlinks are not followed**, and it doesn't descend into dependency or build folders (`node_modules`, `dist`, `build`, `out`, `target`, `coverage`, `vendor`, `Pods`, `DerivedData`, `.venv`, `__pycache__`, and others). Those folders can still get an icon; their contents are just left unscanned.
- **`.git` is special-cased:** the `.git` *folder* gets a real icon, but its contents are never scanned. A `.git` *pointer file* (worktrees/submodules) is left untouched.
- Finder and OS metadata (`Icon\r`, `.DS_Store`, AppleDouble `._*` files) is skipped.
- If a scan would exceed 120,000 entries in a single folder, it stops and asks you to target a smaller folder.

## Folder icons and Git

A folder icon lives in a hidden `Icon\r` file inside the folder, which git would otherwise see as a new untracked file. The tool adds one `Icon?` rule to your **global** git ignore file (`core.excludesFile`, defaulting to `~/.config/git/ignore`), so the metadata is hidden from every repository at once. It never writes to a repository's `.git/info/exclude` or its tracked `.gitignore`. `undo` and `clean` remove the rule again if the tool added it.

## State and undo

Every icon the tool installs is recorded in a compact journal at:

```text
~/.local/state/finder-icon-themes/state.json
```

Mutations share an OS advisory lock, acquired before reading state. Apply checkpoints ownership before each batch; undo checkpoints after batches. No-op runs leave the journal unchanged. Atomic-save replacements are checked as new objects. Reconciliation and watcher renames transfer ownership by stable identity.

New icons also record a resource-fork fingerprint. A manual replacement is never overwritten or removed; its record is retained. Legacy entries lack fingerprints, and an interrupted apply can leave an ambiguous intent: those custom icons are left in place and removal reports them for recovery. Cleanup stops the service first, preserves failed/ambiguous ownership records, and only removes state/cache after successful cleanup. Compatible schema-2/3 entries are retained; corrupt or unknown journals fail closed and remain intact.

- Override the location with `FINDER_ICON_THEMES_HOME=/path`.
- Override the config with `FINDER_ICON_THEMES_CONFIG=/path/to/config.toml`.
- Use an on-disk theme instead of the embedded one with `FINDER_ICON_THEMES_THEME_DIR=/path/to/material-icon-theme`.
- Don't run two icon synchronisers against the same files at once.

## Project layout

```text
finder-icon-themes/
├── Cargo.toml
├── setup.command              Build, install to ~/.local/bin, remove target/
├── assets/
│   └── material-theme.tar.gz  Material theme, embedded in the binary
├── src/
│   ├── main.rs                CLI commands
│   ├── apply.rs               Apply / preview / undo / clean
│   ├── scanner.rs             Safe filesystem traversal
│   ├── matcher.rs             Icon matching and fallbacks
│   ├── theme.rs               Theme loading (embedded or on disk)
│   ├── render.rs              SVG → PNG rendering
│   ├── macos.rs               NSWorkspace custom-icon calls
│   ├── git.rs                 Repo discovery and the global exclude rule
│   ├── watch.rs               Filesystem watcher
│   ├── service.rs             LaunchAgent management
│   ├── config.rs              Config file
│   └── state.rs               Undo journal
└── tests/
```

## Development

```sh
cargo test
```

The tests cover icon matching, scanning and opt-out, Git repository discovery, journal round-trips, the embedded theme, 256px rendering, and an end-to-end pass that applies icons to a real temporary repository, checks the global exclude, and verifies undo and clean.

## Credits and licence

[Material Icon Theme](https://github.com/material-extensions/vscode-material-icon-theme) by Material Extensions / PKief (MIT), embedded as a compressed archive; SVG rendering by [resvg](https://github.com/linebender/resvg).

This project is MIT-licensed (see [LICENSE](LICENSE)) and is independent — not affiliated with the Material Extensions maintainers.
