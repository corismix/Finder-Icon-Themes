import fs from 'node:fs';
import path from 'node:path';
import { findIcon, SKIP_CONTENTS } from './matcher.mjs';

export function collectCandidates(root, manifest, { files = true, folders = true, maxEntries = 120000 } = {}) {
  const found = [];
  let inspected = 0;
  // Iterate without following directory symlinks. The root was realpath-resolved by the caller.
  const stack = [root];
  while (stack.length) {
    const dir = stack.pop();
    const entries = fs.readdirSync(dir, { withFileTypes: true });
    for (const entry of entries) {
      // Iconise a real `.git` *folder*, but never descend into it. A `.git` *file*
      // can point to linked Git metadata in worktrees/submodules; leave it alone.
      if ((entry.name === '.git' && !entry.isDirectory()) || entry.name === 'Icon\r' || entry.name === '.DS_Store' || entry.name.startsWith('._')) continue;
      if (++inspected > maxEntries) throw new Error(`Scan stopped at ${maxEntries.toLocaleString()} entries. Choose a smaller project directory.`);
      if (entry.isSymbolicLink()) continue;
      const full = path.join(dir, entry.name);
      const isDirectory = entry.isDirectory();
      if (!isDirectory && !entry.isFile()) continue;
      if ((isDirectory && folders) || (!isDirectory && files)) {
        const match = findIcon(entry.name, isDirectory, manifest, false);
        if (match) found.push({ path: full, relative: path.relative(root, full), directory: isDirectory, ...match });
      }
      if (isDirectory && !SKIP_CONTENTS.has(entry.name) && !SKIP_CONTENTS.has(entry.name.toLowerCase())) {
        stack.push(full);
      }
    }
  }
  return found.sort((a,b) => a.relative.localeCompare(b.relative, 'en'));
}
