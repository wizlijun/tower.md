import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, writeFileSync, readFileSync, mkdirSync, rmSync, symlinkSync, realpathSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { createHash } from 'node:crypto';
import { inventory, checkScope } from './audit.mjs';
const sha = value => createHash('sha256').update(value).digest('hex');
const script = resolve('scripts/release/audit.mjs');
function git(root, ...args) {
  const result = spawnSync('git', args, {cwd: root, encoding: 'utf8',
    env: {...process.env, GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_NOSYSTEM: '1',
      GIT_AUTHOR_NAME: 'fixture', GIT_AUTHOR_EMAIL: 'fixture@example.invalid',
      GIT_COMMITTER_NAME: 'fixture', GIT_COMMITTER_EMAIL: 'fixture@example.invalid'}});
  assert.equal(result.status, 0, `fixture git ${args[0]}: ${result.stderr}`);
  return result.stdout.trim();
}
function fixture(t) {
  const dir = realpathSync(mkdtempSync(join(tmpdir(), 'towermd-audit-test-')));
  t.after(() => rmSync(dir, {recursive: true, force: true}));
  const root = join(dir, 'repo'), other = join(dir, 'other 中文');
  mkdirSync(root); git(root, 'init', '-b', 'main');
  writeFileSync(join(root, '.gitignore'), '.release/\n');
  writeFileSync(join(root, 'tracked'), 'base\n');
  writeFileSync(join(root, 'rename old'), 'rename\n');
  git(root, 'add', '.'); git(root, '-c', 'commit.gpgsign=false', 'commit', '-m', 'base');
  git(root, 'worktree', 'add', '-b', 'feature', other);
  writeFileSync(join(other, 'feature'), 'feature\n');
  git(other, 'add', '.'); git(other, '-c', 'commit.gpgsign=false', 'commit', '-m', 'feature');
  writeFileSync(join(root, 'tracked'), 'SECRET stash body\n');
  git(root, 'stash', 'push', '-m', 'SECRET stash subject');
  writeFileSync(join(root, 'tracked'), Buffer.from([0, 1, 2, 255]));
  writeFileSync(join(other, '未跟踪 space\nline'), 'untracked');
  return {root, other, dir};
}
const decisions = inv => ({inventorySha256: inv.inventorySha256,
  items: inv.items.map(({id}) => ({id, disposition: 'exclude', reason: 'other task', evidence: 'fixture record'}))});

test('inventories both real worktrees, dirty bytes, untracked path, branch commit and stash without bodies', t => {
  const {root, other} = fixture(t), inv = inventory(root);
  assert.equal(inv.head, git(root, 'rev-parse', 'HEAD'));
  assert.equal(inv.main, git(root, 'rev-parse', 'main'));
  assert.equal(inv.items.filter(i => i.kind === 'worktree').length, 2);
  const dirty = inv.items.find(i => i.kind === 'file' && i.details.path === 'tracked');
  assert.equal(dirty.details.xy, ' M');
  assert.equal(dirty.details.contentSha256, sha(Buffer.from([0, 1, 2, 255])));
  assert.ok(inv.items.find(i => i.kind === 'file' && i.details.worktree === other && i.details.path === '未跟踪 space\nline'));
  assert.ok(inv.items.find(i => i.kind === 'commit' && i.details.branch === 'refs/heads/feature'));
  assert.equal(inv.items.filter(i => i.kind === 'branch').length, 2);
  assert.equal(inv.items.filter(i => i.kind === 'stash').length, 1);
  assert.match(inv.inventorySha256, /^[a-f0-9]{64}$/);
  assert.doesNotMatch(JSON.stringify(inv), /SECRET/);
  assert.deepEqual(inventory(root), inv);
});

test('scope rejects omissions, extra decisions, digest omissions, invalid evidence and dirty include', t => {
  const inv = inventory(fixture(t).root), d = decisions(inv);
  assert.doesNotThrow(() => checkScope(inv, d));
  for (const bad of [{items: d.items}, {...d, inventorySha256: '0'.repeat(64)}, {...d, items: []},
    {...d, items: [...d.items, {id: 'extra'}]},
    {...d, items: d.items.map((i, n) => n ? i : {...i, reason: ''})},
    {...d, items: d.items.map((i, n) => n ? i : {...i, evidence: ''})},
    {...d, items: d.items.map(i => ({...i, disposition: 'automatic'}))}]) {
    assert.throws(() => checkScope(inv, bad));
  }
  const file = inv.items.find(i => i.kind === 'file');
  assert.throws(() => checkScope(inv, {...d, items: d.items.map(i => i.id === file.id ? {...i, disposition: 'include'} : i)}));
});

for (const change of ['untracked bytes', 'new file', 'stash', 'branch', 'head']) {
  test(`scope refuses drift: ${change}`, t => {
    const {root, other} = fixture(t), before = inventory(root), d = decisions(before);
    if (change === 'untracked bytes') writeFileSync(join(other, '未跟踪 space\nline'), 'changed');
    if (change === 'new file') writeFileSync(join(root, 'added'), 'new');
    if (change === 'stash') git(root, 'stash', 'push', '-m', 'another');
    if (change === 'branch') {
      writeFileSync(join(other, 'feature'), 'changed'); git(other, 'add', 'feature');
      git(other, '-c', 'commit.gpgsign=false', 'commit', '-m', 'next');
    }
    if (change === 'head') git(root, '-c', 'commit.gpgsign=false', 'commit', '--allow-empty', '-m', 'main next');
    assert.throws(() => checkScope(inventory(root), d));
  });
}

test('NUL parsing preserves rename paths and binary index/worktree hashes', t => {
  const {root} = fixture(t);
  git(root, 'mv', 'rename old', '改名 space\nnew');
  git(root, 'add', 'tracked');
  writeFileSync(join(root, 'tracked'), Buffer.from([0, 22, 33]));
  const inv = inventory(root);
  const renamed = inv.items.find(i => i.kind === 'file' && i.details.path === '改名 space\nnew');
  assert.equal(renamed.details.originalPath, 'rename old');
  assert.equal(renamed.details.xy, 'R ');
  const dirty = inv.items.find(i => i.kind === 'file' && i.details.path === 'tracked');
  assert.equal(dirty.details.xy, 'MM');
  const wt = inv.items.find(i => i.kind === 'worktree' && i.details.worktree === root);
  const binary = args => spawnSync('git', args, {cwd: root}).stdout;
  assert.equal(wt.details.indexDiffSha256, sha(binary(['diff', '--cached', '--binary', '--no-ext-diff', '--no-textconv'])));
  assert.equal(wt.details.workDiffSha256, sha(binary(['diff', '--binary', '--no-ext-diff', '--no-textconv'])));
});

test('symlink content hashes link text without reading an outside secret', t => {
  const {root, dir} = fixture(t), secret = join(dir, 'secret');
  writeFileSync(secret, 'OUTSIDE SECRET'); symlinkSync(secret, join(root, 'link'));
  const inv = inventory(root), link = inv.items.find(i => i.kind === 'file' && i.details.path === 'link');
  assert.equal(link.details.contentSha256, sha(secret));
  assert.doesNotMatch(JSON.stringify(inv), /OUTSIDE SECRET/);
});

test('patch equivalence requires actual cherry, both stable patch ids, frozen main and functional evidence', t => {
  const {root, other} = fixture(t), commit = git(other, 'rev-parse', 'HEAD');
  // Apply the same change under a distinct commit identity on main.
  writeFileSync(join(root, 'feature'), 'feature\n'); git(root, 'add', 'feature');
  git(root, '-c', 'commit.gpgsign=false', 'commit', '-m', 'rewritten feature');
  const inv = inventory(root), item = inv.items.find(i => i.kind === 'commit' && i.details.commit === commit);
  assert.equal(item.details.cherry, '-');
  assert.match(item.details.patchId, /^[a-f0-9]{40}$/);
  assert.ok(item.details.mainPatches.some(p => p.patchId === item.details.patchId));
  const evidence = {commit, frozenMain: inv.main, patchId: item.details.patchId,
    mainCommit: item.details.mainPatches[0].commit, mainPatchId: item.details.patchId,
    verification: 'frozen main feature assertion: fixture test record'};
  const d = decisions(inv), choose = e => ({...d, items: d.items.map(i => i.id === item.id ?
    {...i, disposition: 'patch-equivalent', evidence: e} : i)});
  assert.doesNotThrow(() => checkScope(inv, choose(evidence)));
  for (const e of ['cherry -', {...evidence, patchId: '0'.repeat(40)},
    {...evidence, mainCommit: commit}, {...evidence, frozenMain: commit}, {...evidence, verification: ''}])
    assert.throws(() => checkScope(inv, choose(e)));
});

test('fixed runner is readonly and a failed worktree enumeration rejects without leaking stderr', t => {
  const {root} = fixture(t), calls = [];
  const run = (file, args, opts) => {
    calls.push({file, args, opts}); return spawnSync(file, args, opts);
  };
  inventory(root, run);
  assert.ok(calls.length > 0);
  for (const {file, args, opts} of calls) {
    assert.equal(file, 'git'); assert.equal(opts.shell, false);
    assert.ok(!args.some(a => ['clean', 'reset', 'checkout', 'config', 'pop'].includes(a)));
  }
  assert.throws(() => inventory(root, (file, args, opts) => args[0] === 'worktree' ?
    {status: 1, stdout: '', stderr: 'SECRET env'} : run(file, args, opts)),
    error => !error.message.includes('SECRET'));
});

function cli(root, ...args) { return spawnSync(process.execPath, [script, ...args], {cwd: root, encoding: 'utf8'}); }

test('CLI scans only explicit local JSON and re-collects before checking, preserving other work', t => {
  const {root} = fixture(t);
  const before = git(root, 'status', '--porcelain=v1', '-z');
  const scan = cli(root, 'scan', '--root', root, '--output', '.release/inventory.json');
  assert.equal(scan.status, 0, scan.stderr);
  const inv = JSON.parse(readFileSync(join(root, '.release/inventory.json'), 'utf8'));
  writeFileSync(join(root, '.release/scope.json'), JSON.stringify(decisions(inv)));
  const check = cli(root, 'check', '--root', root, '--decisions', '.release/scope.json');
  assert.equal(check.status, 0, check.stderr); assert.match(check.stdout, /scope verified/);
  assert.equal(git(root, 'status', '--porcelain=v1', '-z'), before);
  writeFileSync(join(root, 'new'), 'new');
  assert.notEqual(cli(root, 'check', '--root', root, '--decisions', '.release/scope.json').status, 0);
});

for (const args of [[], ['scan'], ['wat', '--root', '.'], ['scan', '--root', '.', '--output'],
  ['scan', '--root', '.', '--output', '../escape.json'], ['scan', '--root', '.', '--output', '/tmp/escape.json'],
  ['scan', '--root', '.', '--output', 'a.txt'], ['scan', '--root', '.', '--output', '.release/a.json', '--force'],
  ['check', '--root', '.', '--decisions', '.release/scope.json', '--output', '.release/a.json'],
  ['scan', '--root', '.', '--root', '.', '--output', '.release/a.json']]) {
  test(`CLI rejects invalid arguments ${JSON.stringify(args)}`, t => {
    const {root} = fixture(t); assert.notEqual(cli(root, ...args).status, 0);
  });
}

test('CLI rejects a symlink output directory outside root', t => {
  const {root, dir} = fixture(t); symlinkSync(dir, join(root, 'escape'));
  assert.notEqual(cli(root, 'scan', '--root', root, '--output', 'escape/leak.json').status, 0);
});

test('deleted tracked files in a removed directory retain status with null content hash', t => {
  const {root} = fixture(t); mkdirSync(join(root, 'gone'));
  writeFileSync(join(root, 'gone', 'file'), 'gone'); git(root, 'add', 'gone');
  git(root, '-c', 'commit.gpgsign=false', 'commit', '-m', 'directory');
  rmSync(join(root, 'gone'), {recursive: true});
  const item = inventory(root).items.find(i => i.kind === 'file' && i.details.path === 'gone/file');
  assert.equal(item.details.xy, ' D'); assert.equal(item.details.contentSha256, null);
});

test('duplicate decisions and changed inventory content cannot reuse a digest', t => {
  const inv = inventory(fixture(t).root), d = decisions(inv);
  const duplicate = {...d, items: [...d.items.slice(1), d.items[1]]};
  assert.throws(() => checkScope(inv, duplicate));
  const changed = structuredClone(inv); changed.items[0].details.changed = true;
  assert.throws(() => checkScope(changed, d));
});

test('status paths through an outside directory symlink fail instead of reading outside bytes', t => {
  const {root, dir} = fixture(t); mkdirSync(join(dir, 'outside'));
  writeFileSync(join(dir, 'outside', 'file'), 'OUTSIDE CONTENT');
  symlinkSync(join(dir, 'outside'), join(root, 'escape'));
  const run = (file, args, opts) => args[0] === 'status' && opts.cwd === root ?
    {status: 0, stdout: Buffer.from('?? escape/file\0'), stderr: ''} : spawnSync(file, args, opts);
  assert.throws(() => inventory(root, run), /escapes worktree/);
});

test('excluding input is read only and including dirty worktree or stash requires a new committed scan', t => {
  const {root} = fixture(t), inv = inventory(root), d = decisions(inv);
  for (const item of inv.items.filter(i => i.kind === 'stash' || (i.kind === 'worktree' && i.details.dirty)))
    assert.throws(() => checkScope(inv, {...d, items: d.items.map(i => i.id === item.id ? {...i, disposition: 'include'} : i)}));
  const before = git(root, 'status', '--porcelain=v1', '-z'), stash = git(root, 'rev-parse', 'refs/stash');
  checkScope(inv, d);
  assert.equal(git(root, 'status', '--porcelain=v1', '-z'), before);
  assert.equal(git(root, 'rev-parse', 'refs/stash'), stash);
});
