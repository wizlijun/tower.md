import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir, homedir } from 'node:os';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { prepareVersion, checkVersion, main } from './prepare.mjs';
import { checkChangelogs } from './core.mjs';

const cargo = join(homedir(), '.cargo/bin/cargo');
const script = fileURLToPath(new URL('./prepare.mjs', import.meta.url));
const oldEnglish = '## v0.0.1 — 2026-10-16\n\n- Old English.  \n<!-- preserved -->\n';
const oldChinese = '## v0.0.1 — 2026-10-16\n\n- 原有内容。  \n<!-- 保留 -->\n';
const files = {
  'Cargo.toml': '[workspace]\nmembers = ["crates/towermd-core", "crates/towermd"]\nexclude = ["src-tauri"]\n\n[workspace.package]\nversion = "0.0.0"\nedition = "2021"\n\n[workspace.dependencies]\nthird = { version = "0.0.0" }\n',
  'crates/towermd-core/Cargo.toml': '[package]\nname = "towermd-core"\nversion.workspace = true\n\n[dependencies]\nthird = "0.0.0"\n',
  'crates/towermd/Cargo.toml': '[package]\nname = "towermd"\nversion.workspace = true\n',
  'src-tauri/Cargo.toml': '[package]\nname = "towermd-app"\nversion = "0.0.0"\n\n[dependencies]\nthird = { version = "0.0.0" }\n\n[workspace]\n',
  'package.json': JSON.stringify({ name: 'towermd-app', version: '0.0.0', dependencies: { third: '0.0.0' } }, null, 2) + '\n',
  'src-tauri/tauri.conf.json': '{\n\t"version": "0.0.0",\n\t"bundle": {}\n}\n',
  'Cargo.lock': 'version = 4\n\n[[package]]\nname = "towermd-core"\nversion = "0.0.0"\n\n[[package]]\nname = "towermd"\nversion = "0.0.0"\n\n[[package]]\nname = "third"\nversion = "0.0.0"\nsource = "registry+sentinel"\nchecksum = "sentinel-checksum"\n',
  'src-tauri/Cargo.lock': 'version = 4\n\n[[package]]\nname = "towermd-app"\nversion = "0.0.0"\n\n[[package]]\nname = "third"\nversion = "0.0.0"\nsource = "registry+sentinel"\nchecksum = "sentinel-checksum"\n',
  'pnpm-lock.yaml': "lockfileVersion: '9.0'\n\nimporters:\n\n  .:\n    dependencies:\n      third:\n        specifier: 0.0.0\n        version: 0.0.0\n\npackages:\n  third@0.0.0:\n    resolution: {integrity: sentinel}\n",
  'CHANGELOG.md': '# Changelog\n\n## Unreleased\n\n### Added\n\n- New feature.\n\n' + oldEnglish,
  'CHANGELOG.zh-CN.md': '# 更新日志\n\n## 未发布\n\n### 新增\n\n- 新功能。\n\n' + oldChinese,
  'not-a-manifest.txt': 'version = "0.0.0"\n',
};
function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'towermd-prepare-test-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const read = path => readFileSync(join(root, path), 'utf8');
  const write = (path, text) => { mkdirSync(join(root, path, '..'), { recursive: true }); writeFileSync(join(root, path), text); };
  for (const [path, text] of Object.entries(files)) write(path, text);
  const calls = [];
  const options = {};
  const metadata = desktop => {
    const version = /^version = "([^"]+)"/m.exec(read(desktop ? 'src-tauri/Cargo.toml' : 'Cargo.toml'))[1];
    const names = desktop ? ['towermd-app'] : ['towermd-core', 'towermd'];
    return { workspace_root: join(root, desktop ? 'src-tauri' : ''), workspace_members: names, packages: names.map(name => ({ id: name, name, version,
      manifest_path: join(root, name === 'towermd-app' ? 'src-tauri/Cargo.toml' : `crates/${name}/Cargo.toml`) })) };
  };
  const updateLock = desktop => {
    const file = desktop ? 'src-tauri/Cargo.lock' : 'Cargo.lock';
    const value = metadata(desktop).packages[0].version;
    write(file, read(file).replace(/(name = "towermd(?:-core|-app)?"\nversion = ")[^"]+/g, `$1${value}`));
  };
  const run = (file, args, config) => {
    calls.push({ file, args: [...args], ...config });
    assert.equal(config.cwd, root);
    assert.equal(config.shell, false);
    if (options.fail) return { status: 7, stdout: 'SECRET-SENTINEL', stderr: 'SECRET-SENTINEL cache miss' };
    if (file === cargo) {
      const desktop = args.includes('--manifest-path');
      if (args[0] === 'metadata') {
        if (!args.includes('--locked') && !options.needsCheck) updateLock(desktop);
        const data = metadata(desktop);
        if (options.metadataDrift) data.packages[0].version = '9.9.9';
        if (options.metadataMissing) data.workspace_members = [];
        return { status: 0, stdout: JSON.stringify(data), stderr: '' };
      }
      assert.equal(args[0], 'check');
      if (!options.keepStale) updateLock(desktop);
    } else {
      assert.equal(file, process.execPath);
      assert.equal(args[0], 'scripts/project-pnpm.mjs');
      assert.equal(args[1], 'install');
      assert.ok(args.includes('--offline'));
    }
    return { status: 0, stdout: '', stderr: '' };
  };
  return { root, read, write, calls, run, options };
}
const snapshot = f => Object.fromEntries(Object.keys(files).map(path => [path, f.read(path)]));

test('prepare synchronizes only project versions, locks and bilingual sections', t => {
  const f = fixture(t), before = snapshot(f);
  const changed = prepareVersion(f.root, '0.1.0', '2026-10-17', f.run);
  for (const path of ['Cargo.toml', 'src-tauri/Cargo.toml']) assert.match(f.read(path), /version = "0.1.0"/);
  assert.equal(JSON.parse(f.read('package.json')).version, '0.1.0');
  assert.equal(JSON.parse(f.read('src-tauri/tauri.conf.json')).version, '0.1.0');
  assert.match(f.read('src-tauri/tauri.conf.json'), /\n\t"version"/);
  for (const path of ['Cargo.lock', 'src-tauri/Cargo.lock']) {
    assert.match(f.read(path), /name = "towermd[^\n]*\nversion = "0.1.0"/);
    assert.ok(f.read(path).endsWith(before[path].slice(before[path].indexOf('[[package]]\nname = "third"'))));
  }
  assert.match(f.read('Cargo.toml'), /third = \{ version = "0.0.0" \}/);
  assert.equal(f.read('crates/towermd-core/Cargo.toml'), before['crates/towermd-core/Cargo.toml']);
  assert.equal(f.read('pnpm-lock.yaml'), before['pnpm-lock.yaml']);
  assert.equal(f.read('not-a-manifest.txt'), before['not-a-manifest.txt']);
  const en = f.read('CHANGELOG.md'), zh = f.read('CHANGELOG.zh-CN.md');
  assert.match(en, /## Unreleased\n\n## v0\.1\.0 — 2026-10-17/);
  assert.match(zh, /## 未发布\n\n## v0\.1\.0 — 2026-10-17/);
  checkChangelogs(en, zh, 'frozen', '0.1.0');
  assert.throws(() => checkChangelogs(en, zh, 'prepare', '0.1.1'));
  assert.equal(en.slice(en.indexOf('## v0.0.1')), oldEnglish);
  assert.equal(zh.slice(zh.indexOf('## v0.0.1')), oldChinese);
  assert.deepEqual([...changed].sort(), Object.keys(before).filter(path => before[path] !== f.read(path)).sort());
  assert.deepEqual(f.calls.filter(c => c.file === cargo && !c.args.includes('--locked')).map(c => c.args), [
    ['metadata', '--offline', '--no-deps', '--format-version', '1'],
    ['metadata', '--offline', '--no-deps', '--format-version', '1', '--manifest-path', 'src-tauri/Cargo.toml'],
  ]);
  assert.deepEqual(f.calls.filter(c => c.file === process.execPath).map(c => c.args), [
    ['scripts/project-pnpm.mjs', 'install', '--offline', '--lockfile-only'],
    ['scripts/project-pnpm.mjs', 'install', '--frozen-lockfile', '--offline'],
  ]);
  assert.equal(f.calls.filter(c => c.file === cargo && c.args.includes('--locked')).length, 4);
});

test('check prepare and frozen are read-only and repeated prepare cannot rotate twice', t => {
  const f = fixture(t);
  const before = snapshot(f);
  checkVersion(f.root, '0.0.0', 'prepare', f.run);
  assert.deepEqual(snapshot(f), before);
  assert.ok(f.calls.every(c => c.file === cargo && c.args.includes('--locked')));
  prepareVersion(f.root, '0.1.0', '2026-10-17', f.run);
  const frozen = snapshot(f); f.calls.length = 0;
  checkVersion(f.root, '0.1.0', 'frozen', f.run);
  assert.deepEqual(snapshot(f), frozen);
  assert.throws(() => prepareVersion(f.root, '0.1.0', '2026-10-18', f.run), /unreleased|already released/);
  assert.deepEqual(snapshot(f), frozen);
});

for (const [name, path, transform, pattern] of [
  ['frontend version', 'package.json', text => text.replace('"version": "0.0.0"', '"version": "8.0.0"'), /package.json/],
  ['Tauri version', 'src-tauri/tauri.conf.json', text => text.replace('0.0.0', '8.0.0'), /tauri.conf.json/],
  ['desktop version', 'src-tauri/Cargo.toml', text => text.replace('0.0.0', '8.0.0'), /src-tauri\/Cargo.toml/],
  ['core inheritance', 'crates/towermd/Cargo.toml', text => text.replace('version.workspace = true', 'version = "0.0.0"'), /crates\/towermd\/Cargo.toml/],
  ['missing exclude', 'Cargo.toml', text => text.replace('exclude = ["src-tauri"]', ''), /exclude/],
  ['missing desktop workspace', 'src-tauri/Cargo.toml', text => text.replace('[workspace]', ''), /workspace/],
  ['unknown workspace member', 'Cargo.toml', text => text.replace('"crates/towermd"]', '"crates/towermd", "extra"]'), /members/],
  ['duplicate version source', 'Cargo.toml', text => text.replace('version = "0.0.0"', 'version = "0.0.0"\nversion = "0.0.0"'), /version/],
  ['root stale lock', 'Cargo.lock', text => text.replace('version = "0.0.0"', 'version = "8.0.0"'), /Cargo.lock/],
  ['desktop stale lock', 'src-tauri/Cargo.lock', text => text.replace('version = "0.0.0"', 'version = "8.0.0"'), /src-tauri\/Cargo.lock/],
  ['missing lock member', 'Cargo.lock', text => text.replace('name = "towermd"', 'name = "wrong"'), /Cargo.lock/],
  ['duplicate lock member', 'Cargo.lock', text => text + '\n[[package]]\nname = "towermd"\nversion = "0.0.0"\n', /Cargo.lock/],
  ['unknown lock member', 'Cargo.lock', text => text + '\n[[package]]\nname = "towermd-extra"\nversion = "0.0.0"\n', /Cargo.lock/],
  ['pnpm dependency mismatch', 'pnpm-lock.yaml', text => text.replace('specifier: 0.0.0', 'specifier: 8.0.0'), /pnpm-lock.yaml/],
  ['pnpm importer missing', 'pnpm-lock.yaml', text => text.replace('  .:', '  other:'), /pnpm-lock.yaml/],
  ['pnpm root version mismatch', 'pnpm-lock.yaml', text => text.replace('  .:\n', "  .:\n    version: '8.0.0'\n"), /pnpm-lock.yaml/],
]) test(`check rejects ${name} without changing files`, t => {
  const f = fixture(t); f.write(path, transform(f.read(path))); const before = snapshot(f);
  assert.throws(() => checkVersion(f.root, '0.0.0', 'prepare', f.run), pattern);
  assert.deepEqual(snapshot(f), before);
  assert.throws(() => prepareVersion(f.root, '0.1.0', '2026-10-17', f.run), pattern);
  assert.deepEqual(snapshot(f), before);
});

test('prepare gate rejects empty translation before any manifest write or runner call', t => {
  const f = fixture(t); f.write('CHANGELOG.zh-CN.md', '# 更新日志\n\n## 未发布\n\n' + oldChinese);
  const before = snapshot(f);
  assert.throws(() => prepareVersion(f.root, '0.1.0', '2026-10-17', f.run), /unreleased/);
  assert.deepEqual(snapshot(f), before); assert.equal(f.calls.length, 0);
});
test('optional pnpm root importer version is updated without touching dependency versions', t => {
  const f = fixture(t); f.write('pnpm-lock.yaml', f.read('pnpm-lock.yaml').replace('  .:\n', "  .:\n    version: '0.0.0'\n"));
  prepareVersion(f.root, '0.1.0', '2026-10-17', f.run);
  assert.match(f.read('pnpm-lock.yaml'), /    version: '0.1.0'/);
  assert.match(f.read('pnpm-lock.yaml'), /        version: 0.0.0/);
});
test('metadata fallback uses offline check separately for each stale workspace lock', t => {
  const f = fixture(t); f.options.needsCheck = true;
  prepareVersion(f.root, '0.1.0', '2026-10-17', f.run);
  assert.deepEqual(f.calls.filter(c => c.args[0] === 'check').map(c => c.args), [
    ['check', '--offline'], ['check', '--offline', '--manifest-path', 'src-tauri/Cargo.toml'],
  ]);
});
test('locks still stale after fallback refuse rotation', t => {
  const f = fixture(t); f.options.needsCheck = true; f.options.keepStale = true;
  assert.throws(() => prepareVersion(f.root, '0.1.0', '2026-10-17', f.run), /Cargo.lock/);
  assert.equal(f.read('CHANGELOG.md'), files['CHANGELOG.md']);
});
for (const option of ['metadataDrift', 'metadataMissing']) test(`metadata rejects ${option}`, t => {
  const f = fixture(t); f.options[option] = true;
  assert.throws(() => checkVersion(f.root, '0.0.0', 'prepare', f.run), /metadata/);
});
test('runner nonzero status stops and redacts child stdout/stderr', t => {
  const f = fixture(t); f.options.fail = true; const before = snapshot(f);
  assert.throws(() => prepareVersion(f.root, '0.1.0', '2026-10-17', f.run), error => /exit 7/.test(error.message) && !error.message.includes('SECRET-SENTINEL'));
  assert.deepEqual(snapshot(f), before);
});
for (const version of ['v0.1.0', '0.1', '0.1.0-beta', '', '0.1.0\n']) test(`API rejects invalid version ${JSON.stringify(version)}`, t => {
  const f = fixture(t); const before = snapshot(f);
  assert.throws(() => prepareVersion(f.root, version, '2026-10-17', f.run), /version/);
  assert.deepEqual(snapshot(f), before); assert.equal(f.calls.length, 0);
});
for (const date of ['2026-2-03', '2026-02-30', '2025-02-29', '2026-13-01', '', '2026-10-17\n']) test(`API rejects invalid date ${JSON.stringify(date)}`, t => {
  const f = fixture(t); const before = snapshot(f);
  assert.throws(() => prepareVersion(f.root, '0.1.0', date, f.run), /date/);
  assert.deepEqual(snapshot(f), before); assert.equal(f.calls.length, 0);
});
for (const args of [[], ['unknown'], ['check'], ['prepare'], ['check', '--root'],
  ['check', '--root', '.', '--version', '0.0.0', '--phase', 'unknown'],
  ['check', '--root', '.', '--version', '0.0.0', '--phase', 'prepare', '--date', '2026-10-17'],
  ['prepare', '--root', '.', '--version', '0.1.0', '--date', '2026-02-30'],
  ['prepare', '--root', '.', '--version', '0.1.0', '--date', '2026-10-17', '--force'],
  ['check', '--root', '.', '--version', '0.0.0', '--phase', 'prepare', '--version', '0.0.0'],
]) test(`CLI rejects ${JSON.stringify(args)}`, () => {
  const result = spawnSync(process.execPath, [script, ...args], { encoding: 'utf8', shell: false });
  assert.equal(result.status, 1); assert.match(result.stderr, /(?:argument|version|date|phase|command)/);
});
test('CLI dispatch uses explicit root and phase without mutations', t => {
  const f = fixture(t); const before = snapshot(f);
  main(['check', '--root', f.root, '--version', '0.0.0', '--phase', 'prepare'], f.run);
  assert.deepEqual(snapshot(f), before); assert.equal(f.calls.length, 2);
});

// Commented headings are content, never an insertion point for a real release.
test('rotation ignores headings inside HTML comments', t => {
  const f = fixture(t);
  f.write('CHANGELOG.md', '<!--\n## Unreleased\n-->\n' + f.read('CHANGELOG.md'));
  prepareVersion(f.root, '0.1.0', '2026-10-17', f.run);
  assert.ok(f.read('CHANGELOG.md').startsWith('<!--\n## Unreleased\n-->\n'));
  checkChangelogs(f.read('CHANGELOG.md'), f.read('CHANGELOG.zh-CN.md'), 'frozen', '0.1.0');
});
test('CLI prepare dispatch respects explicit date', t => {
  const f = fixture(t);
  main(['prepare', '--root', f.root, '--version', '0.1.0', '--date', '2024-02-29'], f.run);
  assert.match(f.read('CHANGELOG.md'), /## v0\.1\.0 — 2024-02-29/);
});

for (const { name, tool, stderr = '', stdout = '', missing } of [
  { name: 'Cargo missing package', tool: 'cargo',
    stderr: 'error: no matching package named `fixture-cache-missing` found\nSECRET-SENTINEL\nhttps://example.invalid/TOKEN-SENTINEL', missing: 'fixture-cache-missing' },
  { name: 'Cargo missing download', tool: 'cargo',
    stderr: 'error: failed to download `fixture-cache-missing v1.2.3`\n\nCaused by:\n  attempting to make an HTTP request, but --offline was specified\nSECRET-SENTINEL', missing: 'fixture-cache-missing@1.2.3' },
  { name: 'pnpm missing unscoped tarball', tool: 'pnpm',
    stdout: ' ERR_PNPM_NO_OFFLINE_TARBALL  A package is missing from the store but cannot download it in offline mode. The missing package may be downloaded from https://registry.npmjs.org/fixture-cache-missing/-/fixture-cache-missing-1.2.3.tgz.\nSECRET-SENTINEL', missing: 'fixture-cache-missing@1.2.3' },
  { name: 'pnpm missing scoped tarball', tool: 'pnpm',
    stderr: ' ERR_PNPM_NO_OFFLINE_TARBALL  A package is missing from the store but cannot download it in offline mode. The missing package may be downloaded from https://registry.npmjs.org/@fixture/cache-missing/-/cache-missing-1.2.3.tgz.\nhttps://example.invalid/TOKEN-SENTINEL', missing: '@fixture/cache-missing@1.2.3' },
  { name: 'unknown Cargo error', tool: 'cargo', stderr: 'error: arbitrary SECRET-SENTINEL failure\nhttps://example.invalid/TOKEN-SENTINEL' },
  { name: 'Cargo missing name contains URL', tool: 'cargo', stderr: 'error: no matching package named `https://example.invalid/TOKEN-SENTINEL` found' },
  { name: 'Cargo missing name contains uppercase sentinel', tool: 'cargo', stderr: 'error: no matching package named `SECRET-SENTINEL` found' },
  { name: 'Cargo missing line has trailing secret', tool: 'cargo', stderr: 'error: no matching package named `fixture-cache-missing` found SECRET-SENTINEL' },
  { name: 'Cargo failed download is unrelated to offline cache', tool: 'cargo', stderr: 'error: failed to download `fixture-cache-missing v1.2.3`\ncaused by SECRET-SENTINEL' },
  { name: 'pnpm unknown error', tool: 'pnpm', stderr: 'ERR_PNPM_UNKNOWN SECRET-SENTINEL https://example.invalid/TOKEN-SENTINEL' },
  { name: 'pnpm unrecognized registry', tool: 'pnpm', stderr: 'ERR_PNPM_NO_OFFLINE_TARBALL A package is missing from the store but cannot download it in offline mode. The missing package may be downloaded from https://example.invalid/SECRET-SENTINEL/-/SECRET-SENTINEL-1.2.3.tgz.' },
  { name: 'pnpm registry query', tool: 'pnpm', stderr: 'ERR_PNPM_NO_OFFLINE_TARBALL A package is missing from the store but cannot download it in offline mode. The missing package may be downloaded from https://registry.npmjs.org/fixture-cache-missing/-/fixture-cache-missing-1.2.3.tgz?token=SECRET-SENTINEL' },
  { name: 'pnpm tarball name differs', tool: 'pnpm', stderr: 'ERR_PNPM_NO_OFFLINE_TARBALL A package is missing from the store but cannot download it in offline mode. The missing package may be downloaded from https://registry.npmjs.org/fixture-cache-missing/-/SECRET-SENTINEL-1.2.3.tgz.' },
]) test(`offline diagnostic: ${name}`, t => {
  const f = fixture(t);
  let failedCalls = 0;
  const run = (file, args, options) => {
    const target = tool === 'cargo' ? file === cargo : file === process.execPath;
    if (!target) return f.run(file, args, options);
    failedCalls++;
    assert.equal(options.cwd, f.root);
    assert.equal(options.shell, false);
    assert.ok(args.includes('--offline'));
    return { status: tool === 'cargo' ? 101 : 1, stdout, stderr };
  };
  const prefix = tool === 'cargo' ? 'cargo metadata exit 101' : 'project-pnpm.mjs scripts/project-pnpm.mjs exit 1';
  assert.throws(() => prepareVersion(f.root, '0.1.0', '2026-10-17', run), error => {
    assert.equal(error.message, prefix + (missing ? `; missing cache: ${missing}` : ''));
    assert.doesNotMatch(error.message, /SECRET-SENTINEL|TOKEN-SENTINEL|https?:|registry[.]npmjs[.]org/);
    return true;
  });
  assert.equal(failedCalls, 1);
  assert.equal(f.read('CHANGELOG.md'), files['CHANGELOG.md']);
  assert.equal(f.read('CHANGELOG.zh-CN.md'), files['CHANGELOG.zh-CN.md']);
});

for (const stage of ['root metadata', 'desktop metadata', 'root metadata after lock write',
  'root fallback check', 'pnpm lockfile-only', 'pnpm frozen']) test(`failed ${stage} restores owned versions and permits same-version retry`, t => {
  const f = fixture(t);
  f.write('pnpm-lock.yaml', f.read('pnpm-lock.yaml').replace('  .:\n', "  .:\n    version: '0.0.0'\n"));
  if (stage === 'root fallback check') f.options.needsCheck = true;
  const before = snapshot(f);
  let failures = 0;
  const run = (file, args, options) => {
    const unlocked = file === cargo && args[0] === 'metadata' && !args.includes('--locked');
    const desktop = args.includes('--manifest-path');
    const target = (stage === 'root metadata' && unlocked && !desktop) ||
      (stage === 'desktop metadata' && unlocked && desktop) ||
      (stage === 'root metadata after lock write' && unlocked && !desktop) ||
      (stage === 'root fallback check' && file === cargo && args[0] === 'check' && !desktop) ||
      (stage === 'pnpm lockfile-only' && file === process.execPath && args.includes('--lockfile-only')) ||
      (stage === 'pnpm frozen' && file === process.execPath && args.includes('--frozen-lockfile'));
    if (!target) return f.run(file, args, options);
    failures++;
    assert.match(f.read('Cargo.toml'), /version = "0.1.0"/);
    if (stage === 'desktop metadata' || stage.startsWith('pnpm'))
      assert.match(f.read('Cargo.lock'), /name = "towermd-core"\nversion = "0.1.0"/);
    if (stage === 'root metadata after lock write' || stage === 'root fallback check') f.run(file, args, options);
    return { status: 101, stdout: '', stderr: 'error: no matching package named `fixture-cache-missing` found\nSECRET-SENTINEL' };
  };
  assert.throws(() => prepareVersion(f.root, '0.1.0', '2026-10-17', run), /exit 101/);
  assert.equal(failures, 1);
  assert.deepEqual(snapshot(f), before);
  prepareVersion(f.root, '0.1.0', '2026-10-17', f.run);
  checkVersion(f.root, '0.1.0', 'frozen', f.run);
});

test('failure recovery preserves concurrent edits to an owned manifest and an already updated lock', t => {
  const f = fixture(t), before = snapshot(f);
  let externalManifest, externalLock;
  const run = (file, args, options) => {
    if (file === cargo && args[0] === 'metadata' && !args.includes('--locked') && args.includes('--manifest-path')) {
      const pkg = JSON.parse(f.read('package.json')); pkg.userField = 'SECRET-SENTINEL';
      externalManifest = JSON.stringify(pkg, null, 2) + '\n';
      externalLock = f.read('Cargo.lock') + '\n# concurrent SECRET-SENTINEL edit\n';
      f.write('package.json', externalManifest); f.write('Cargo.lock', externalLock);
      return { status: 101, stdout: '', stderr: 'error: no matching package named `fixture-cache-missing` found' };
    }
    return f.run(file, args, options);
  };
  assert.throws(() => prepareVersion(f.root, '0.1.0', '2026-10-17', run), error => {
    assert.match(error.message, /cargo metadata exit 101; missing cache: fixture-cache-missing/);
    assert.match(error.message, /recovery conflict/);
    assert.match(error.message, /package.json/); assert.match(error.message, /Cargo.lock/);
    assert.doesNotMatch(error.message, /SECRET-SENTINEL/); return true;
  });
  assert.equal(f.read('package.json'), externalManifest);
  assert.equal(f.read('Cargo.lock'), externalLock);
  for (const path of Object.keys(files).filter(path => !['package.json', 'Cargo.lock'].includes(path)))
    assert.equal(f.read(path), before[path]);
  let retryCalls = 0;
  assert.throws(() => prepareVersion(f.root, '0.1.0', '2026-10-17', (...args) => { retryCalls++; return f.run(...args); }), /package.json/);
  assert.equal(retryCalls, 0);
  assert.equal(f.read('package.json'), externalManifest); assert.equal(f.read('Cargo.lock'), externalLock);
});
