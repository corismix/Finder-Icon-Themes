import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { isValidState, statePaths } from './state.mjs';
import { normaliseOwnedGitExcludes } from './git.mjs';

// Import an existing compatible journal without carrying historical product
// names or hard-coded directories into this project.
function isCompatibleJournal(data) {
  if (!isValidState(data)) return false;
  const entries = Object.entries(data.entries);
  if (!entries.length) return false;
  return entries.every(([item, record]) =>
    path.isAbsolute(item) && record !== null && typeof record === 'object' &&
    typeof record.id === 'string' && record.id.length > 0 &&
    typeof record.identity === 'string' && /^\d+:\d+:\d+$/.test(record.identity) &&
    typeof record.directory === 'boolean' &&
    (record.repo == null || typeof record.repo === 'string'));
}

export function discoverJournals(home = os.homedir(), destination = statePaths({}, home).state) {
  const parent = path.join(home, '.local', 'state');
  let dirs;
  try { dirs = fs.readdirSync(parent, { withFileTypes: true }); }
  catch (error) { if (error.code === 'ENOENT') return []; throw error; }
  const found = [];
  for (const entry of dirs) {
    if (!entry.isDirectory()) continue;
    const file = path.join(parent, entry.name, 'state.json');
    if (file === destination) continue;
    try {
      const raw = JSON.parse(fs.readFileSync(file, 'utf8'));
      if (isCompatibleJournal(raw)) found.push({ file, data: raw });
    } catch (error) {
      if (error.code === 'ENOENT' || error instanceof SyntaxError) continue;
      throw error;
    }
  }
  return found;
}

export function importJournal({ env = process.env, home = os.homedir() } = {}) {
  const dest = statePaths(env, home).state;
  if (fs.existsSync(dest)) return { status: 'exists', imported: 0 };
  const candidates = discoverJournals(home, dest);
  if (!candidates.length) return { status: 'none', imported: 0 };
  if (candidates.length > 1) return { status: 'ambiguous', imported: 0, count: candidates.length };
  const data = candidates[0].data;
  // Don't mutate the source journal. Update only owned, recognisable Git
  // exclusion blocks; the original file remains a fallback backup.
  normaliseOwnedGitExcludes(data);
  // Create destination exclusively to avoid overwriting a concurrent install.
  fs.mkdirSync(path.dirname(dest), { recursive: true, mode: 0o700 });
  const temp = `${dest}.${process.pid}.tmp`;
  fs.writeFileSync(temp, `${JSON.stringify(data, null, 2)}\n`, { mode: 0o600, flag: 'wx' });
  try {
    fs.linkSync(temp, dest);
  } finally {
    fs.unlinkSync(temp);
  }
  return { status: 'imported', imported: Object.keys(data.entries).length };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const result = importJournal();
  if (result.status === 'imported') console.log(`Imported ${result.imported} existing icon records into Finder Icon Themes. Original journal retained as backup.`);
  if (result.status === 'ambiguous') {
    console.error(`Found ${result.count} compatible journals. No automatic import performed. Copy the desired state.json into ~/.local/state/finder-icon-themes/ manually.`);
    process.exitCode = 1;
  }
}
