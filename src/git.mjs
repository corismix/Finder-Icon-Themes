import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

export const MARKER = '# finder-icon-themes (local macOS folder icon metadata)';
const RULE = 'Icon?';
const ownLine = `${MARKER}\n${RULE}\n`;
// Earlier journals recorded ownership but sometimes omitted the marker itself.
// Recognise our characteristic comment format without knowing an earlier app name.
const ownedBlock = /^# [a-z0-9-]+ \(local macOS folder icon metadata\)\r?\nIcon\?\r?\n/gm;

export function gitRootFor(item, cache = new Map()) {
  let dir = item;
  try { if (!fs.statSync(dir).isDirectory()) dir = path.dirname(dir); } catch { return null; }
  if (path.basename(dir) === '.git') dir = path.dirname(dir);
  const history = [];
  while (true) {
    if (cache.has(dir)) {
      const found = cache.get(dir);
      for (const h of history) cache.set(h, found);
      return found;
    }
    history.push(dir);
    if (fs.existsSync(path.join(dir, '.git'))) {
      for (const h of history) cache.set(h, dir);
      return dir;
    }
    const parent = path.dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  for (const h of history) cache.set(h, null);
  return null;
}
function excludePath(repo) {
  const r = spawnSync('git', ['-C', repo, 'rev-parse', '--git-path', 'info/exclude'], { encoding: 'utf8' });
  if (r.status !== 0) throw new Error(`Could not find local Git exclude file for ${repo}: ${r.stderr.trim()}`);
  return path.resolve(repo, r.stdout.trim());
}

// Upgrades older journal-owned marker blocks without touching unrelated Git rules.
// A journal entry must explicitly claim ownership before any old comment is rewritten.
export function normaliseOwnedGitExcludes(state) {
  for (const record of Object.values(state.excludes)) {
    if (!record.owned || typeof record.file !== 'string' || !fs.existsSync(record.file)) continue;
    const before = fs.readFileSync(record.file, 'utf8');
    let originalBlock;
    if (typeof record.marker === 'string' && record.marker !== MARKER) {
      const block = `${record.marker}\n${RULE}\n`;
      if (before.includes(block)) originalBlock = block;
    }
    if (!originalBlock && !before.includes(ownLine)) {
      originalBlock = before.match(ownedBlock)?.[0];
    }
    if (originalBlock && originalBlock !== ownLine) {
      // Refuse to overwrite a separately existing marker; no duplicate blocks.
      if (!before.includes(ownLine)) fs.writeFileSync(record.file, before.replace(originalBlock, ownLine));
    }
    // Only claim a normalized block that is confirmed to be present on disk.
    const current = fs.readFileSync(record.file, 'utf8');
    if (current.includes(ownLine)) record.marker = MARKER;
    else { record.owned = false; delete record.marker; }
  }
}

export function ensureLocalGitExclude(repo, state) {
  if (!repo || state.excludes[repo]) return;
  const file = excludePath(repo);
  const before = fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : '';
  if (before.split(/\r?\n/).includes(RULE) || before.includes(MARKER)) {
    state.excludes[repo] = { file, owned: false };
    return;
  }
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.appendFileSync(file, `${before && !before.endsWith('\n') ? '\n' : ''}${ownLine}`);
  state.excludes[repo] = { file, owned: true, marker: MARKER };
}
export function tidyLocalGitExcludes(state) {
  for (const [repo, record] of Object.entries(state.excludes)) {
    const stillNeeded = Object.entries(state.entries).some(([p, data]) => data.directory && data.repo === repo);
    if (stillNeeded) continue;
    if (record.owned && fs.existsSync(record.file)) {
      const current = fs.readFileSync(record.file, 'utf8');
      const marker = record.marker || MARKER;
      const target = `${marker}\n${RULE}\n`;
      if (current.includes(target)) fs.writeFileSync(record.file, current.replace(target, ''));
    }
    delete state.excludes[repo];
  }
}
