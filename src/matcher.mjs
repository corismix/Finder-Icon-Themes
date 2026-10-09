/** VS Code Material Icon Theme manifest associations, closest matches first. */
export function findIcon(name, isDirectory, manifest, isRoot = false) {
  const lower = name.toLowerCase();
  if (isDirectory) {
    // In VS Code, an unmatched folder uses the theme's generic folder icon.
    // The default is in manifest.folder, not in manifest.folderNames.
    const id = (isRoot && manifest.rootFolderNames?.[lower])
      || manifest.folderNames?.[lower]
      || (isRoot && manifest.rootFolder)
      || manifest.folder;
    return id && manifest.iconDefinitions?.[id]?.iconPath ? { id, source: manifest.iconDefinitions[id].iconPath } : null;
  }
  let id = manifest.fileNames?.[lower];
  if (!id) {
    const parts = lower.split('.');
    // Longest extension match first: config.test.ts, test.ts, ts.
    for (let index = 1; index < parts.length; index++) {
      const suffix = parts.slice(index).join('.');
      if (manifest.fileExtensions?.[suffix]) { id = manifest.fileExtensions[suffix]; break; }
    }
    // Dotfile extensions, e.g. .env.local, when no exact file association exists.
    if (!id && lower.startsWith('.')) id = manifest.fileExtensions?.[lower.slice(1)];
  }
  // Unmatched files also get the actual generic Material file icon.
  id ||= manifest.file;
  return id && manifest.iconDefinitions?.[id]?.iconPath ? { id, source: manifest.iconDefinitions[id].iconPath } : null;
}

export const SKIP_CONTENTS = new Set([
  '.git', '.svn', '.hg', '.build', '.next', '.nuxt', '.svelte-kit', '.turbo',
  '.vercel', '.output', '.cache', '.parcel-cache', '.venv', 'venv', '__pycache__',
  'node_modules', 'dist', 'build', 'out', 'target', 'coverage', 'vendor',
  '.gradle', '.idea', '.dart_tool', 'Pods', 'DerivedData', '.pytest_cache',
]);
