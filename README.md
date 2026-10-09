# Finder Icon Themes

Real file and folder icons in **macOS Finder** — and any other browser that honours macOS custom icons — using the artwork and associations from VS Code icon themes. The built-in theme is **[Material Icon Theme](https://github.com/material-extensions/vscode-material-icon-theme)**, with its real SVGs, filename/extension associations, named folders, and generic fallbacks.

It's a local command-line tool. Point it at a folder, it walks the tree once and sets a custom icon on each recognised file and folder. Run it again to update; `undo` puts things back.

The icons are stored as filesystem metadata, so Finder, Path Finder, and similar browsers show them with no per-app theme switch. The tool does **not** change file-type associations, Quick Look previews, or system icon themes, and it is **not** a Finder extension or a background watcher — rerun it after adding files.

## Requirements

- macOS with Apple Command Line Tools — run `xcode-select --install` if needed
- Node.js 20 or newer, with npm
- An internet connection for the first install (npm fetches the theme and the SVG renderer)

## Install

From the project directory:

```sh
./setup.command
```

Setup checks for Node and the Swift compiler, runs `npm install`, writes `package-lock.json`, and — if it finds exactly one existing compatible icon journal — imports it into the current state location, keeping the original file as a backup. Nothing already in a journal is overwritten.

The macOS icon helper is compiled on the first `apply` or `undo`, so that first run takes a few extra seconds.

If you received this as a plain folder with no Git history, it's source-only: initialise a repository when you're ready and commit `package-lock.json` for reproducible installs. `node_modules/` is ignored.

## Usage

Start with a read-only preview — it never modifies anything:

```sh
./finder-icon-themes preview ~/Code/my-project
```

Then apply, and undo later if you change your mind:

```sh
./finder-icon-themes apply ~/Code/my-project
./finder-icon-themes undo  ~/Code/my-project
```

If the icons don't appear straight away, refresh the Finder window; the folder icon cache can lag.

| Command | Effect |
| --- | --- |
| `themes` | List the installed icon themes. |
| `preview <folder>` | Show what would change, without writing anything. |
| `apply <folder>` | Set icons on recognised files and folders. |
| `undo <folder>` | Remove **only** the icons this tool installed under that folder. |
| `status <folder>` | List the icons currently recorded for that folder. |

Options for `preview` and `apply`:

| Option | Meaning |
| --- | --- |
| `--theme NAME` | Icon theme to use (default `material`; see `themes`). |
| `--files-only` | Leave folder icons alone. |
| `--folders-only` | Leave file icons alone. |
| `--limit N` | Maximum icons changed in one run (default 1200, max 20000). |

Check a small project before pointing it at a large tree such as `~/Code`.

## How it chooses icons

Matching follows VS Code icon-theme rules, in this order:

- **Files:** exact filename (`package.json`) → longest extension match (`config.test.ts` → `test.ts` → `ts`) → dotfile extension (`.env.local`) → the theme's generic file icon.
- **Folders:** the folder's own name (`.git`, `src`) → the theme's generic folder icon.

Anything the theme has no specific icon for still gets its generic file or folder icon, so the tree stays consistent rather than patchy.

## What it skips

- **Pre-existing custom icons are never overwritten.** If you — or another app — set an icon on a file, the tool leaves it alone and manages only icons it installed itself.
- **Symlinks are not followed**, and it doesn't descend into dependency or build folders (`node_modules`, `dist`, `build`, `out`, `target`, `coverage`, `vendor`, `Pods`, `DerivedData`, `.venv`, `__pycache__`, and others). Those folders can still get an icon; their contents are just left unscanned.
- **`.git` is special-cased:** the `.git` *folder* gets a real icon, but its contents are never scanned. A `.git` *pointer file* (worktrees/submodules) is left untouched.
- Finder and OS metadata (`Icon\r`, `.DS_Store`, AppleDouble `._*` files) is skipped.
- If a scan would exceed 120,000 entries, it stops and asks you to target a smaller folder.

## Folder icons and Git

A folder icon lives in a hidden `Icon\r` file inside the folder. To keep that out of `git status`, the tool appends an `Icon?` rule to each affected repository's **local** `.git/info/exclude` — never to the tracked `.gitignore`. That rule hides any `Icon` plus one-character name, so be aware if your project deliberately uses filenames like that.

## State and undo

Every icon the tool installs is recorded in a journal at:

```text
~/.local/state/finder-icon-themes/state.json
```

`undo` reads that journal to remove only its own icons. If a file has been moved or replaced since it was iconised, its entry is forgotten rather than applied blindly to whatever is there now.

- Override the location with `FINDER_ICON_THEMES_HOME=/path ./finder-icon-themes apply ...`.
- Don't run two icon synchronisers against the same files at once.
- Switching themes replaces the icons the tool owns while keeping their undo ownership, so you can always get back to the macOS defaults.

## Project layout

```text
Finder-Icon-Themes/
├── finder-icon-themes    CLI entry point
├── setup.command         Install dependencies
├── src/
│   ├── cli.mjs           Commands, apply/preview/undo
│   ├── scanner.mjs       Safe filesystem traversal
│   ├── matcher.mjs       Icon matching and fallbacks
│   ├── themes/           Theme registry and adapters
│   ├── native.swift      macOS custom-icon helper
│   ├── render.mjs        SVG → PNG rendering
│   ├── git.mjs           Local Git exclude management
│   └── state.mjs         Undo journal
├── tests/
└── docs/adding-a-theme.md
```

## Development

```sh
npm test
```

The tests cover icon matching and fallbacks, `.git` safety, local Git exclusions, journal import, and the guarantee that `preview`/`status` leave files untouched. Applying icons goes through Apple's `NSWorkspace.setIcon` API and needs macOS, so it isn't exercised by the test suite.

## Adding another theme

Only Material ships today, and the theme seam is deliberately small. See [docs/adding-a-theme.md](docs/adding-a-theme.md) to add another icon set.

## Credits and licence

[Material Icon Theme](https://github.com/material-extensions/vscode-material-icon-theme) by Material Extensions / PKief (MIT); SVG rendering by [resvg-js](https://github.com/yisibl/resvg-js) (MPL-2.0). Their packages come from npm and are not vendored here.

This project is MIT-licensed (see [LICENSE](LICENSE)) and is independent — not affiliated with the Material Extensions maintainers.
