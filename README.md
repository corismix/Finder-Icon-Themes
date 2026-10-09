# Finder Icon Themes

Bring developer-friendly file and folder icons to **macOS Finder** and other file browsers that honour macOS custom icons.

The built-in theme is **[Material Icon Theme](https://github.com/material-extensions/vscode-material-icon-theme)**, using its *real* SVG artwork, exact filename/extension associations, named folders, and generic fallback icons. The architecture keeps theme loading in `src/themes/` so other icon sets can be added later.

This is a local, on-demand command-line tool, not a Finder extension or background watcher. It **changes filesystem icon metadata** rather than changing application associations, Quick Look, or system icon themes.

## Requirements

- macOS with Apple Command Line Tools (`xcode-select --install` if needed)
- Node.js 20+ and npm
- Internet connection during initial `npm install` (to fetch the Material theme and SVG renderer)

## Quick start

Unzip anywhere, open Terminal in the unzipped `Finder-Icon-Themes` directory, and run:

```sh
./setup.command
```

Then preview a project without modifying anything:

```sh
./finder-icon-themes preview ~/Code/AI/Eidolon
```

Apply to that project:

```sh
./finder-icon-themes apply ~/Code/AI/Eidolon
```

Refresh your file browser if the icons are cached. Undo **only icons installed by this tool**:

```sh
./finder-icon-themes undo ~/Code/AI/Eidolon
```

Check which paths it manages:

```sh
./finder-icon-themes status ~/Code/AI/Eidolon
```

See supported themes:

```sh
./finder-icon-themes themes
```

The default is `material`. You can specify it explicitly with `--theme material` on `preview` or `apply`. `--folders-only`, `--files-only` and `--limit N` also work; the default change limit is 1,200. To apply to all projects later, target `~/Code` once you've checked a smaller project.

## Initialise your own Git repository

This ZIP deliberately contains **source and project configuration only**, no `.git/`, installed `node_modules/`, generated icons, caches, local icon journal, personal filesystem paths, or secrets. After extracting, you can initialise and commit it yourself:

```sh
git init
```

```sh
git add .
```

```sh
git commit -m "Initial commit: Finder Icon Themes"
```

The first `./setup.command` installs npm dependencies, generates `package-lock.json`, and checks for a compatible existing icon journal outside this repository. An imported journal is copied, not moved, and owned Git exclusion comments may be updated. The lockfile **should be committed** for reproducible installations. `node_modules/` is ignored. The lockfile is generated locally because this build environment cannot access npm's registry. Run setup before `git add .` if you want the lockfile in your initial commit.

## Git safety and existing installs

- Scans directory entries without following symlinks. Skips contents of `.git`, `node_modules`, and other dependency/build folders. It *does* give a real `.git` directory Material's Git folder icon. A `.git` pointer file (worktrees/submodules) stays untouched.
- Folder icons can create hidden `Icon\r` files. The tool adds an `Icon?` rule to each affected repository's **local `.git/info/exclude`**, not the tracked `.gitignore`. This excludes other `Icon` + one-character names too, so be aware if your project uses those.
- Skips pre-existing custom icons not installed by this tool. A journal outside the repository supports `undo`; moved/replaced files are not blindly modified.
- During setup, a compatible existing icon journal is imported into `~/.local/state/finder-icon-themes/state.json` if exactly one is found. Its original is retained as a backup, and its owned Git-exclude labels are updated. No existing state is overwritten. If more than one compatible journal is found, setup stops and asks you to choose one manually.
- State location can be overridden with `FINDER_ICON_THEMES_HOME`. Don’t run multiple icon synchronisers against the same files.
- If an icon already belongs to the tool, a future theme can replace it while keeping the original undo ownership. **No untracked existing custom icon is overwritten.**

## Project structure

```text
Finder-Icon-Themes/
├── finder-icon-themes         CLI entry point
├── setup.command             Install third-party dependencies
├── src/
│   ├── cli.mjs               Commands, apply, preview and undo
│   ├── scanner.mjs           Safe filesystem traversal
│   ├── matcher.mjs           VS Code-style icon matching + fallbacks
│   ├── themes/
│   │   ├── index.mjs         Theme registry
│   │   └── material.mjs      Material theme adapter
│   ├── native.swift          macOS custom-icon API
│   ├── native.mjs            Compile/run native Swift helper
│   ├── render.mjs            SVG to PNG conversion
│   ├── git.mjs               Local Git ignore management
│   └── state.mjs             External undo journal
├── tests/
├── docs/adding-a-theme.md
├── package.json
├── .gitignore
└── LICENSE
```

## Development

```sh
npm test
```

The Node tests cover matching, fallbacks, `.git` safety, Git exclusions, and one-time state import without hard-coded previous product names. Native icon application requires macOS and has **not been re-run in this build environment**. The macOS icon application uses Apple’s `NSWorkspace.setIcon` API.

See [docs/adding-a-theme.md](docs/adding-a-theme.md) for the extension boundary. Other themes are not implemented yet; `themes` currently lists Material only.

## Credits

[Material Icon Theme](https://github.com/material-extensions/vscode-material-icon-theme) by Material Extensions / PKief (MIT); SVG rendering by [resvg-js](https://github.com/yisibl/resvg-js) (MPL-2.0). Their packages are installed from npm and are not vendored into this Git repository. Independent project, not affiliated with the Material Extensions maintainers.
