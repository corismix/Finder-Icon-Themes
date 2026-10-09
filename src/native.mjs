import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';
const source = fileURLToPath(new URL('./native.swift', import.meta.url));

function nativeExecutable(cacheDir) {
  if (process.platform !== 'darwin') throw new Error('Applying or undoing icons requires macOS.');
  fs.mkdirSync(cacheDir, { recursive: true });
  const binary = path.join(cacheDir, 'icon-helper');
  const sourceMtime = fs.statSync(source).mtimeMs;
  if (!fs.existsSync(binary) || fs.statSync(binary).mtimeMs < sourceMtime) {
    process.stdout.write('Compiling macOS icon helper (first run only)…\n');
    const result = spawnSync('/usr/bin/xcrun', ['swiftc', '-O', source, '-o', binary], { encoding: 'utf8', maxBuffer: 4*1024*1024 });
    if (result.error || result.status !== 0) throw new Error(`Swift build failed: ${result.stderr || result.error}\nInstall Apple Command Line Tools with xcode-select --install.`);
  }
  return binary;
}
export function nativeRequest(cacheDir, action, items) {
  if (!items.length) return [];
  const command = nativeExecutable(cacheDir);
  const child = spawnSync(command, [], {
    input: JSON.stringify({ action, items }), encoding: 'utf8',
    maxBuffer: 16*1024*1024,
  });
  if (child.error || child.status !== 0) throw new Error(`Icon helper failed: ${child.stderr || child.error || child.stdout}`);
  return JSON.parse(child.stdout);
}
