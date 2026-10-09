# Adding another icon theme

The current app intentionally implements one theme well. It supports a small registration seam instead of prematurely adding a plugin framework.

1. Add a module under `src/themes/<name>.mjs` exporting `async function load(appDirectory)`.
2. Return `{ name, title, manifest, assetsDir }`. The `manifest` uses the VS Code icon theme object shape: `iconDefinitions`, `fileNames`, `fileExtensions`, `folderNames`, plus generic `file` and `folder` identifiers. Each icon definition's `iconPath` should refer to a `.svg` file inside `assetsDir`.
3. Register the name, display title, and dynamic module import in `src/themes/index.mjs`.
4. Add dependency/assets installation as needed, plus tests and credits for the new icon set.

Matching uses exact filename associations first, then longest extension suffix, and a generic fallback. Directory matching uses the name followed by the generic folder icon. If a new icon theme has different rules, add an explicit matcher adapter rather than silently breaking Material matching.

The undo journal stores an optional `theme` field per managed icon. Entries without that field are treated as `material` for backwards-compatible undo. Avoid changing filesystem state/identity logic when adding a theme.

**Note:** Theme icons are applied as filesystem metadata. Finder and other compatible file browsers see the result; this isn't a per-browser theme switch.
