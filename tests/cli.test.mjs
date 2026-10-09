import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const project = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

test('fresh copy can list themes, preview matches and show status without touching files', () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'finder-cli-test-'));
  try {
    const install = path.join(tmp, 'Finder-Icon-Themes');
    fs.mkdirSync(install);
    fs.cpSync(path.join(project, 'src'), path.join(install, 'src'), { recursive: true });
    fs.copyFileSync(path.join(project, 'package.json'), path.join(install, 'package.json'));
    const packagePath = path.join(install, 'node_modules', 'material-icon-theme');
    fs.mkdirSync(path.join(packagePath, 'icons'), { recursive: true });
    fs.writeFileSync(path.join(packagePath, 'package.json'), '{"name":"material-icon-theme","type":"module","exports":"./index.js"}');
    fs.writeFileSync(path.join(packagePath, 'index.js'), `export function generateManifest() { return {
      file: 'file', folder: 'folder', fileNames: {'package.json': 'npm'},
      fileExtensions: {ts:'typescript'}, folderNames: {'.git':'folder-git'},
      iconDefinitions: Object.fromEntries(['file','folder','npm','typescript','folder-git'].map(n=>[n,{iconPath:'../icons/'+n+'.svg'}]))
    }; }`);
    const target = path.join(tmp, 'example');
    fs.mkdirSync(target);
    fs.mkdirSync(path.join(target, '.git'));
    fs.writeFileSync(path.join(target, 'package.json'), '{}');
    fs.writeFileSync(path.join(target, 'surprise.xyz'), 'text');
    const before = fs.readdirSync(target).sort();
    const env = { ...process.env, FINDER_ICON_THEMES_HOME: path.join(tmp, 'state') };
    function run(...args) {
      return spawnSync(process.execPath, [path.join(install, 'src', 'cli.mjs'), ...args], { encoding: 'utf8', env });
    }
    let result = run('themes');
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /material.*Material Icon Theme/);
    result = run('preview', target, '--theme', 'material');
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /\.git.*folder-git/);
    assert.match(result.stdout, /package\.json.*npm/);
    assert.match(result.stdout, /surprise\.xyz.*file/);
    result = run('status', target);
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /0 managed icons/);
    assert.deepEqual(fs.readdirSync(target).sort(), before);
    assert.equal(fs.existsSync(path.join(tmp, 'state', 'state.json')), false);
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
});
