# Adding another icon theme

The app ships one theme (Material) on purpose, but keeps a small seam for adding more. A theme is just a module: it describes file and folder associations and points at SVG artwork. Scanning, matching, rendering, applying, and undo are all shared.

## 1. Write the theme module

Create `src/themes/<name>.mjs` exporting an async `load(appDirectory)` function:

```js
// src/themes/acme.mjs
import path from 'node:path';

export async function load(appDirectory) {
  return {
    name: 'acme',
    title: 'Acme Icons',
    assetsDir: path.join(appDirectory, 'src', 'themes', 'acme-icons'),
    manifest: {
      file: 'file',
      folder: 'folder',
      fileNames: { 'package.json': 'npm' },
      fileExtensions: { ts: 'typescript' },
      folderNames: { src: 'folder-src' },
      iconDefinitions: {
        file:          { iconPath: 'file.svg' },
        folder:        { iconPath: 'folder.svg' },
        npm:           { iconPath: 'npm.svg' },
        typescript:    { iconPath: 'typescript.svg' },
        'folder-src':  { iconPath: 'folder-src.svg' },
      },
    },
  };
}
```

`load` is called once per run and may do async work — reading a package from `node_modules`, generating a manifest, downloading assets, and so on. It returns:

| Field | Purpose |
| --- | --- |
| `name` | Stable id, written into the undo journal. Use the same id you register. |
| `title` | Human-readable name shown in run summaries. Keep it consistent with the registry title used by `themes`. |
| `assetsDir` | Directory holding the theme's SVG files. |
| `manifest` | The icon associations, in VS Code icon-theme shape. |

Material's adapter (`src/themes/material.mjs`) is a good reference for the package-backed case: it locates `material-icon-theme` inside `node_modules`, calls `generateManifest()`, and points `assetsDir` at that package's `icons` directory.

## 2. Shape of the manifest

Only these keys are read:

| Key | Meaning |
| --- | --- |
| `iconDefinitions` | Map of icon id → `{ iconPath }`. Everything else references these ids. |
| `fileNames` | Exact filename → icon id. |
| `fileExtensions` | Extension or compound suffix → icon id. |
| `folderNames` | Folder name → icon id. |
| `file`, `folder` | Generic fallback ids for unmatched files and folders. |

Two constraints on artwork:

- `iconPath` may be any relative path, but the tool renders it by taking the **basename** and resolving it inside `assetsDir`. Every SVG filename must therefore be unique within `assetsDir` and end in `.svg`.
- SVGs are rendered to 256 px PNGs and cached by content hash, so editing an icon regenerates it automatically — no cache-busting needed.

## 3. Register it

Add an entry to the registry in `src/themes/index.mjs`:

```js
['acme', {
  name: 'acme',
  title: 'Acme Icons',
  loader: () => import('./acme.mjs'),
}],
```

The loader is lazy: the module is imported only when the theme is used, so an optional or uninstalled theme won't break the others.

## 4. Install assets and dependencies

If the theme comes from npm, guard `load()` with a clear error telling the user to run setup, exactly as the Material adapter does. If you vendor SVGs into the repository, put them under `src/themes/<name>/` (or similar), commit them, and record the icon set's licence and attribution in the README credits.

## Matching

Matching is shared, not per-theme. Files resolve by exact filename, then longest extension suffix, then dotfile extension, then the generic file icon. Folders resolve by name, then the generic folder icon. A generic fallback is used only when the manifest actually defines that id.

If a new theme genuinely needs different rules, add a matcher adapter and select it per theme rather than changing the shared matcher — that keeps Material's behaviour intact.

## State and undo

The journal stores an optional `theme` field on each entry; entries without it are treated as `material` so existing journals still undo. Keep that default when adding a theme.

Don't change the filesystem identity/ownership logic (`dev:ino:birthtime`) — that is what makes `undo` safe, because it lets the tool recognise that a file was moved or replaced and skip it.

## Test it

- `./finder-icon-themes themes` lists the new theme.
- `./finder-icon-themes preview <dir> --theme <name>` shows matches without writing.
- `apply` then `undo` round-trips the folder back to macOS default icons.
- `npm test` covers the shared matcher, scanner, Git-exclude, and journal behaviour.

---

Theme icons are filesystem metadata: applying a theme changes what Finder and compatible browsers show, not an in-app theme setting.
