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
| `undo <folder>` | Remove **only** the icons this tool installed under that folder. |
| `status <folder>` | List the icons currently recorded for that folder. |
| `watch` | Run the watcher in the foreground. |
| `service install\|uninstall\|start\|stop\|status` | Manage the background LaunchAgent. |
| `clean` | Remove all managed icons, the global exclude rule, the service and state. |

Options for `preview` and `apply`:

| Option | Meaning |
| --- | --- |
| `--theme NAME` | Icon theme to use (default `material`). |
| `--files-only` | Leave folder icons alone. |
| `--folders-only` | Leave file icons alone. |

There is no size limit: every file and folder the theme can name is themed, however large the repository.

## Automatic mode

`service install` registers a per-user LaunchAgent that watches the folders in your config file and applies icons to every git repository it finds — on start, and again a couple of seconds after anything changes.

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
```

Nothing outside `watch_roots` is ever touched. Icons are never removed when the service stops, quits, or crashes — removal is always explicit (`undo`, `clean`).

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

`undo` reads that journal to remove only its own icons. If a file has been moved or replaced since it was iconised, its entry is forgotten rather than applied blindly to whatever is there now.

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

The tests cover icon matching, scanning and opt-out, Git repository discovery, journal round-trips, the embedded theme, 128px rendering, and an end-to-end pass that applies icons to a real temporary repository, checks the global exclude, and verifies undo and clean.

## Credits and licence

[Material Icon Theme](https://github.com/material-extensions/vscode-material-icon-theme) by Material Extensions / PKief (MIT), embedded as a compressed archive; SVG rendering by [resvg](https://github.com/linebender/resvg).

This project is MIT-licensed (see [LICENSE](LICENSE)) and is independent — not affiliated with the Material Extensions maintainers.
