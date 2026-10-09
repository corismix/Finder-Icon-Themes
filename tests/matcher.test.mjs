import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import { findIcon } from '../src/matcher.mjs';
import { collectCandidates } from '../src/scanner.mjs';
import { identity, isWithin, statePaths } from '../src/state.mjs';
import { listThemes } from '../src/themes/index.mjs';
import { importJournal, discoverJournals } from '../src/migrate.mjs';
import { gitRootFor, ensureLocalGitExclude, tidyLocalGitExcludes, normaliseOwnedGitExcludes, MARKER } from '../src/git.mjs';

const manifest = {
  fileNames: { 'package.json': 'npm', '.env': 'env', 'vitest.config.ts':'vitest' },
  fileExtensions: { 'config.ts': 'config', ts:'typescript', json:'json', 'test.ts': 'test' },
  folderNames: { docs:'folder-docs', 'node_modules': 'folder-npm', tests:'folder-tests', '.git':'folder-git' },
  rootFolderNames: { demo:'folder-root-demo' },
  file: 'file',
  folder: 'folder',
  rootFolder: 'folder-root',
  iconDefinitions: Object.fromEntries(['npm','env','vitest','config','typescript','json','test','folder-docs','folder-npm','folder-tests','folder-git','folder-root-demo','file','folder','folder-root'].map(x => [x, {iconPath:`../icons/${x}.svg`}]))
};

test('matching names beats extensions; longest suffix; case insensitive', () => {
  assert.equal(findIcon('package.json', false, manifest).id, 'npm');
  assert.equal(findIcon('VITEST.CONFIG.TS', false, manifest).id, 'vitest');
  assert.equal(findIcon('vite.config.ts', false, manifest).id, 'config');
  assert.equal(findIcon('hello.test.ts', false, manifest).id, 'test');
  assert.equal(findIcon('main.ts', false, manifest).id, 'typescript');
  assert.equal(findIcon('.env', false, manifest).id, 'env');
  assert.equal(findIcon('docs', true, manifest).id, 'folder-docs');
  assert.equal(findIcon('.git', true, manifest).id, 'folder-git');
  assert.equal(findIcon('unknown', true, manifest).id, 'folder');
  assert.equal(findIcon('unrecognised.weird', false, manifest).id, 'file');
});

test('generic icons are fallbacks, not overrides, and root folders use their own generic', () => {
  assert.equal(findIcon('docs', true, manifest).id, 'folder-docs');
  assert.equal(findIcon('demo', true, manifest, true).id, 'folder-root-demo');
  assert.equal(findIcon('misc', true, manifest, true).id, 'folder-root');
  assert.equal(findIcon('src', true, manifest).source, '../icons/folder.svg');
  assert.equal(findIcon('any.randomformat', false, manifest).source, '../icons/file.svg');
  assert.equal(findIcon('main.ts', false, manifest).id, 'typescript');
  // No fake fallback: if a package has no defaults, unmatched items stay alone.
  assert.equal(findIcon('misc', true, { iconDefinitions: {} }), null);
  assert.equal(findIcon('unknown.zzz', false, { iconDefinitions: {} }), null);
});

test('scan iconises .git folder but skips its internals, symlinks and node_modules contents', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(),'finder-test-'));
  try {
    fs.mkdirSync(path.join(dir,'docs'));
    fs.mkdirSync(path.join(dir,'misc'));
    fs.mkdirSync(path.join(dir,'.build'));
    fs.mkdirSync(path.join(dir,'node_modules'));
    fs.mkdirSync(path.join(dir,'.git'));
    fs.writeFileSync(path.join(dir,'package.json'),'{}');
    fs.writeFileSync(path.join(dir,'unknown.blurb'),'hello');
    fs.writeFileSync(path.join(dir,'.DS_Store'),'OS metadata');
    fs.writeFileSync(path.join(dir,'._package.json'),'AppleDouble');
    fs.writeFileSync(path.join(dir,'Icon\r'),'folder icon resource');
    fs.writeFileSync(path.join(dir,'node_modules','some.ts'),'');
    fs.writeFileSync(path.join(dir,'.git','secret.ts'),'');
    fs.writeFileSync(path.join(dir,'docs','intro.ts'),'');
    fs.symlinkSync(path.join(dir, 'docs'),path.join(dir,'linked-docs'),'dir');
    const items = collectCandidates(dir, manifest);
    assert.deepEqual(items.map(x=>x.relative).sort(), ['.build','.git','docs','docs/intro.ts','misc','node_modules','package.json','unknown.blurb']);
    assert.equal(items.find(x=>x.relative === '.git').id, 'folder-git');
    assert.ok(!items.some(x => x.relative.startsWith('.git/')));
    assert.equal(items.find(x=>x.relative === 'misc').id, 'folder');
    assert.equal(items.find(x=>x.relative === 'unknown.blurb').id, 'file');
    assert.ok(items.find(x=>x.relative === 'docs').directory);
    assert.equal(identity(fs.statSync(dir)),identity(fs.statSync(dir)));
    assert.equal(isWithin(dir, path.join(dir,'docs','intro.ts')),true);
    assert.equal(isWithin(dir, dir+'-fake'),false);
  } finally { fs.rmSync(dir, { recursive:true, force:true }); }
});

test('worktree .git pointer files are excluded, but other unrecognised files get generic icons', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'finder-worktree-test-'));
  try {
    fs.writeFileSync(path.join(dir, '.git'), 'gitdir: ../somewhere');
    fs.writeFileSync(path.join(dir, 'sample.unrecognised'), 'content');
    const items = collectCandidates(dir, manifest);
    assert.deepEqual(items.map(x => x.relative), ['sample.unrecognised']);
    assert.equal(items[0].id, 'file');
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
});

test('local Git exclude is reversible and not duplicated', () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(),'finder-git-test-'));
  try {
    const repo = path.join(tmp,'repo'); fs.mkdirSync(repo);
    const init = spawnSync('git',['init','-q',repo]).status;
    assert.equal(init,0);
    const state={entries:{},excludes:{}};
    assert.equal(gitRootFor(repo), repo);
    assert.equal(gitRootFor(path.join(repo, '.git')), repo);
    ensureLocalGitExclude(repo,state);
    const exclude=state.excludes[repo].file;
    assert.ok(fs.readFileSync(exclude,'utf8').includes('Icon?'));
    fs.mkdirSync(path.join(repo, 'icons'));
    fs.writeFileSync(path.join(repo, 'icons', 'Icon\r'), 'finder metadata');
    assert.equal(spawnSync('git', ['-C', repo, 'status', '--porcelain', '--untracked-files=all'],{encoding:'utf8'}).stdout.trim(), '');
    ensureLocalGitExclude(repo,state);
    assert.equal((fs.readFileSync(exclude,'utf8').match(/finder-icon-themes/g) || []).length,1);
    const nested=path.join(repo, 'nested'); fs.mkdirSync(nested);
    state.entries[nested]={directory:true,repo};
    tidyLocalGitExcludes(state);
    assert.ok(fs.readFileSync(exclude,'utf8').includes('Icon?'));
    delete state.entries[nested];
    tidyLocalGitExcludes(state);
    assert.ok(!fs.readFileSync(exclude,'utf8').includes('finder-icon-themes'));
    assert.equal(Object.keys(state.excludes).length,0);
  } finally { fs.rmSync(tmp, { recursive:true, force:true }); }
});



test('theme registry exposes Material without loading npm dependencies', () => {
  assert.deepEqual(listThemes(), [{ name: 'material', title: 'Material Icon Theme (VS Code)' }]);
});

test('state location uses only the current project name', () => {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'finder-state-'));
  try {
    const expected = path.join(home, '.local/state/finder-icon-themes');
    const prior = path.join(home, '.local/state/earlier-install');
    fs.mkdirSync(prior, { recursive: true });
    fs.writeFileSync(path.join(prior, 'state.json'), '{}');
    assert.equal(statePaths({}, home).base, expected);
    assert.equal(statePaths({ FINDER_ICON_THEMES_HOME: '/tmp/other' }, home).base, '/tmp/other');
  } finally { fs.rmSync(home, { recursive: true, force: true }); }
});

test('generic journal import preserves entries and updates only owned Git exclusion comments', () => {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'finder-import-'));
  try {
    const parent = path.join(home, '.local/state');
    const source = path.join(parent, 'earlier-install', 'state.json');
    const newState = path.join(parent, 'finder-icon-themes', 'state.json');
    const gitExclude = path.join(home, 'repo.git-exclude');
    const priorMarker = '# archived-tool (local macOS folder icon metadata)';
    fs.writeFileSync(gitExclude, `# personal note\n${priorMarker}\nIcon?\n*.user\n`);
    fs.mkdirSync(path.dirname(source), { recursive: true });
    const item = path.join(home, 'project', 'docs');
    const state = { schema: 1,
      entries: { [item]: { identity: '12:34:56', id: 'folder-docs', directory: true, repo: path.join(home, 'project') } },
      excludes: { [path.join(home, 'project')]: { file: gitExclude, owned: true } }
    };
    fs.writeFileSync(source, JSON.stringify(state));
    assert.equal(discoverJournals(home).length, 1);
    assert.deepEqual(importJournal({home, env:{}}), {status:'imported', imported:1});
    assert.equal(fs.existsSync(newState), true);
    assert.deepEqual(JSON.parse(fs.readFileSync(source, 'utf8')), state, 'source journal must not change');
    const migrated = JSON.parse(fs.readFileSync(newState, 'utf8'));
    assert.deepEqual(migrated.entries, state.entries);
    assert.equal(migrated.excludes[path.join(home, 'project')].marker, MARKER);
    assert.equal(fs.readFileSync(gitExclude, 'utf8'), `# personal note\n${MARKER}\nIcon?\n*.user\n`);
    assert.deepEqual(importJournal({home, env:{}}), {status:'exists', imported:0});
    delete migrated.entries[item];
    tidyLocalGitExcludes(migrated);
    assert.equal(fs.readFileSync(gitExclude, 'utf8'), '# personal note\n*.user\n');
  } finally { fs.rmSync(home, { recursive: true, force: true }); }
});

test('import rejects other state formats, and refuses ambiguous candidates', () => {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'finder-import-multi-'));
  try {
    const parent = path.join(home, '.local/state');
    for (const name of ['one','two','unrelated']) {
      const dir = path.join(parent, name);
      fs.mkdirSync(dir, {recursive: true});
      fs.writeFileSync(path.join(dir, 'state.json'), name === 'unrelated' ? '{}' : JSON.stringify({
        schema:1, entries:{[path.join(home,name)]:{id:'file',identity:'1:2:3',directory:false,repo:null}}, excludes:{}
      }));
    }
    assert.equal(discoverJournals(home).length, 2);
    assert.deepEqual(importJournal({home,env:{}}), {status:'ambiguous', imported:0, count:2});
    assert.equal(fs.existsSync(path.join(parent,'finder-icon-themes','state.json')), false);
  } finally { fs.rmSync(home, {recursive:true, force:true}); }
});

test('normalising ignores unowned exclusion comments', () => {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'finder-exclude-test-'));
  try {
    const file = path.join(tmp,'exclude');
    fs.writeFileSync(file, '# personal comment\nIcon?\n');
    const state = { entries:{}, excludes:{repo:{ file, owned:false }} };
    normaliseOwnedGitExcludes(state);
    assert.equal(fs.readFileSync(file, 'utf8'), '# personal comment\nIcon?\n');
  } finally { fs.rmSync(tmp, {recursive:true, force:true}); }
});
