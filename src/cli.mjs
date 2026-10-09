#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { fileURLToPath } from 'node:url';
import { collectCandidates } from './scanner.mjs';
import { statePaths, readState, saveState, identity, isWithin } from './state.mjs';
import { gitRootFor, ensureLocalGitExclude, tidyLocalGitExcludes } from './git.mjs';
import { nativeRequest } from './native.mjs';
import { listThemes, loadTheme } from './themes/index.mjs';

const APP = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const HELP = `Finder Icon Themes · macOS custom icon sync

Usage:
  ./finder-icon-themes themes                List available icon themes
  ./finder-icon-themes preview <folder>      Read-only preview
  ./finder-icon-themes apply <folder>        Apply icons to files and folders
  ./finder-icon-themes undo <folder>         Remove only icons installed by this tool
  ./finder-icon-themes status <folder>       Show recorded changes

Options for preview/apply:
  --theme NAME                              Theme to use (default: material)
  --files-only                              Don't apply folder icons
  --folders-only                            Don't apply file icons
  --limit N                                 Maximum changes per apply (default 1200)

Examples:
  ./finder-icon-themes preview ~/Code/AI/Eidolon
  ./finder-icon-themes apply ~/Code/AI/Eidolon
  ./finder-icon-themes undo ~/Code/AI/Eidolon

Notes:
  • No file associations or Quick Look plugins are changed.
  • Existing custom icons are skipped (not overwritten).
  • Uses each theme's filenames, extensions, folders, and generic fallbacks.
  • Skips .git internals, dependencies, and common generated directories.
  • Folder icons may create hidden Icon\\r files; adds local-only Git excludes.
`;

const [command, ...rest] = process.argv.slice(2);
if (!command || ['--help', '-h', 'help'].includes(command)) { console.log(HELP); process.exit(0); }
if (command === 'themes') {
  console.log('Available themes:');
  for (const theme of listThemes()) console.log(`  ${theme.name.padEnd(12)} ${theme.title}`);
  process.exit(0);
}
if (!['preview', 'apply', 'undo', 'status'].includes(command)) { console.error(HELP); process.exit(2); }

let where;
let files = true, folders = true, limit = 1200, themeName = 'material';
for (let i = 0; i < rest.length; i++) {
  const x = rest[i];
  if (x === '--theme') {
    if (!['preview', 'apply'].includes(command)) throw new Error('--theme is only valid for preview/apply.');
    themeName = rest[++i];
    if (!themeName || themeName.startsWith('-')) throw new Error('Specify a theme after --theme.');
  } else if (x === '--files-only') folders = false;
  else if (x === '--folders-only') files = false;
  else if (x === '--limit') {
    limit = Number(rest[++i]);
    if (!Number.isSafeInteger(limit) || limit < 1 || limit > 20000) throw new Error('Specify a --limit between 1 and 20000.');
  } else if (x.startsWith('-')) throw new Error(`Unknown option: ${x}`);
  else if (where) throw new Error('Specify only one directory.');
  else where = x;
}
if (!where) { console.error('Specify a target directory. Try ./finder-icon-themes help'); process.exit(2); }
const expanded = where === '~' ? os.homedir() : where.startsWith('~/') ? path.join(os.homedir(), where.slice(2)) : where;
const absolute = path.resolve(expanded);
const root = fs.existsSync(absolute) ? fs.realpathSync(absolute) : absolute;
if (command !== 'undo' && !fs.existsSync(root)) throw new Error(`Directory not found: ${root}`);
if (fs.existsSync(root) && !fs.statSync(root).isDirectory()) throw new Error(`Not a directory: ${root}`);

const paths = statePaths();
const state = readState(paths.state);
const tracked = Object.entries(state.entries).filter(([p]) => isWithin(root, p));
if (command === 'status') {
  console.log(`${tracked.length} managed icons under ${root}`);
  for (const [p, v] of tracked.slice(0, 30)) console.log(`  ${v.directory ? '📁' : '📄'} ${path.relative(root, p)} (${v.id})`);
  if (tracked.length > 30) console.log(`  ... ${tracked.length - 30} more`);
  process.exit(0);
}

function acquireLock() {
  fs.mkdirSync(paths.base, { recursive: true, mode: 0o700 });
  const lock = path.join(paths.base, 'journal.lock');
  try { fs.writeFileSync(lock, String(process.pid), { flag: 'wx', mode: 0o600 }); }
  catch { throw new Error(`Another sync may be running. Lock exists at: ${lock}\nRemove it only if no other sync is running.`); }
  return () => { try { fs.unlinkSync(lock); } catch {} };
}

if (command === 'undo') {
  const release = acquireLock();
  try {
    const eligible = []; let stale = 0;
    for (const [item, v] of tracked) {
      try {
        if (identity(fs.lstatSync(item)) !== v.identity) {
          stale++; delete state.entries[item]; continue;
        }
      } catch { stale++; delete state.entries[item]; continue; }
      eligible.push(item);
    }
    console.log(`Undoing ${eligible.length} managed icons under ${root}${stale ? ` (${stale} moved/replaced entries forgotten)` : ''}.`);
    // Persist journal changes before invoking the native helper.
    saveState(paths.state, state);
    let removed = 0; let failed = 0;
    for (let i = 0; i < eligible.length; i += 64) {
      const batch = eligible.slice(i, i+64);
      const result = nativeRequest(paths.cache, 'set', batch.map(p => ({ path: p, icon: null })));
      for (const r of result) {
        if (r.ok) { delete state.entries[r.path]; removed++; }
        else { console.error(`  failed: ${r.path} (${r.error})`); failed++; }
      }
      saveState(paths.state, state);
    }
    tidyLocalGitExcludes(state);
    saveState(paths.state, state);
    console.log(`Restored defaults: ${removed}; failed: ${failed}.`);
    if (failed) process.exitCode = 1;
  } finally { release(); }
} else {
  const theme = await loadTheme(themeName, APP);
  const manifest = theme.manifest;
  const candidates = collectCandidates(root, manifest, { files, folders });
  const affected = candidates.filter(item => {
    const record = state.entries[item.path];
    if (!record) return true;
    try { return record.identity !== identity(fs.lstatSync(item.path)) || record.id !== item.id || (record.theme || 'material') !== themeName; }
    catch { return true; }
  });
  console.log(`${theme.title}: ${candidates.length} recognised or fallback icons under ${root}.`);
  console.log(`Already recorded with matching icon: ${candidates.length - affected.length}.`);
  if (command === 'preview') {
    for (const item of affected.slice(0, 45)) console.log(`  ${item.directory ? 'folder' : 'file  '}  ${item.relative}  →  ${item.id}`);
    if (affected.length > 45) console.log(`  ... ${affected.length - 45} more`);
    console.log(`Preview only. To apply: ./finder-icon-themes apply ${JSON.stringify(root)}`);
  } else {
    const release = acquireLock();
    try {
      // Check icons before writing, to avoid clobbering manually customised items.
      const inspected = nativeRequest(paths.cache, 'inspect', candidates.map(i => ({ path: i.path })));
      const existing = new Map(inspected.map(i => [i.path, i.custom]));
      let skippedCustom = 0, skippedUnchanged = 0, skippedStale = 0;
      const todo = [];
      for (const item of candidates) {
        const prev = state.entries[item.path];
        const currentId = identity(fs.lstatSync(item.path));
        if (prev && prev.identity !== currentId) { skippedStale++; continue; }
        if (existing.get(item.path) && !prev) { skippedCustom++; continue; }
        if (existing.get(item.path) && prev?.id === item.id && (prev.theme || 'material') === themeName) { skippedUnchanged++; continue; }
        todo.push({ ...item, identity: currentId });
      }
      // Only count pending changes, not every icon already installed in a prior run.
      // This allows applying a newly introduced fallback in an existing project.
      if (todo.length > limit) throw new Error(`${todo.length} icon changes exceeds the --limit ${limit}. Use a smaller folder or raise --limit explicitly.`);
      const { pngFor } = await import('./render.mjs');
      const iconCache = new Map();
      for (const item of todo) {
        if (!iconCache.has(item.source)) iconCache.set(item.source, pngFor(item.source, path.join(paths.cache, 'png', themeName), theme.assetsDir));
        item.png = iconCache.get(item.source);
      }
      const repoCache = new Map();
      let changed = 0, failed = 0;
      for (let i = 0; i < todo.length; i += 64) {
        const batch = todo.slice(i, i+64);
        // Journal before changing any icons. Even after an interruption, undo can clear them.
        const previous = new Map();
        for (const item of batch) {
          previous.set(item.path, state.entries[item.path]);
          const repo = item.directory ? gitRootFor(item.path, repoCache) : null;
          if (repo) ensureLocalGitExclude(repo, state);
          state.entries[item.path] = {
            identity: item.identity, id: item.id, theme: themeName,
            directory: item.directory, repo,
          };
        }
        saveState(paths.state, state);
        const result = nativeRequest(paths.cache, 'set', batch.map(x => ({ path: x.path, icon: x.png })));
        for (const r of result) {
          if (r.ok) changed++;
          else {
            failed++;
            console.error(`  failed: ${r.path} (${r.error})`);
            // If it was already managed, preserve its original journal entry.
            if (previous.get(r.path)) state.entries[r.path] = previous.get(r.path);
            else delete state.entries[r.path];
          }
        }
        saveState(paths.state, state);
        console.log(`  processed ${Math.min(i+batch.length, todo.length)}/${todo.length}`);
      }
      tidyLocalGitExcludes(state);
      saveState(paths.state, state);
      console.log(`Applied: ${changed}; unchanged: ${skippedUnchanged}; pre-existing custom icons skipped: ${skippedCustom}; replaced paths skipped: ${skippedStale}; failed: ${failed}.`);
      console.log(`Undo: ./finder-icon-themes undo ${JSON.stringify(root)}`);
      if (failed) process.exitCode = 1;
    } finally { release(); }
  }
}
