import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

export function statePaths(env = process.env, home = os.homedir()) {
  const base = env.FINDER_ICON_THEMES_HOME || path.join(home, '.local', 'state', 'finder-icon-themes');
  return { base, state: path.join(base, 'state.json'), cache: path.join(base, 'cache') };
}
export function isValidState(data) {
  return data?.schema === 1 &&
    data.entries !== null && typeof data.entries === 'object' && !Array.isArray(data.entries) &&
    data.excludes !== null && typeof data.excludes === 'object' && !Array.isArray(data.excludes);
}
export function readState(file) {
  try {
    const data = JSON.parse(fs.readFileSync(file, 'utf8'));
    if (!isValidState(data)) throw new Error('Unsupported state format');
    return data;
  } catch (error) {
    if (error.code === 'ENOENT') return { schema: 1, entries: {}, excludes: {} };
    throw new Error(`Cannot load icon journal at ${file}: ${error.message}. Refusing to proceed.`);
  }
}
export function saveState(file, state) {
  fs.mkdirSync(path.dirname(file), { recursive: true, mode: 0o700 });
  const temp = `${file}.${process.pid}.tmp`;
  fs.writeFileSync(temp, `${JSON.stringify(state, null, 2)}\n`, { mode: 0o600 });
  fs.renameSync(temp, file);
}
export function identity(st) { return `${st.dev}:${st.ino}:${Math.round(st.birthtimeMs)}`; }
export function isWithin(root, item) { return item === root || item.startsWith(root + path.sep); }
