import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, realpathSync, readFileSync, readdirSync, cpSync, chmodSync, symlinkSync, existsSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { tmpdir } from 'node:os';
import { join, dirname, basename } from 'node:path';
import { inspectApp, assembleAssets, inspectDmg, readArchive, archiveSkills, main } from './assets.mjs';
import { expectedAssets, verifySums } from './core.mjs';

const target = 'aarch64-apple-darwin';
function appFixture(t) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'towermd-assets-test-')));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const app = join(root, 'Tower.md.app');
  mkdirSync(join(app, 'Contents/MacOS'), { recursive: true });
  writeFileSync(join(app, 'Contents/Info.plist'), 'fixture plist');
  writeFileSync(join(app, 'Contents/MacOS/towermd-app'), 'synthetic app', { mode: 0o755 });
  writeFileSync(join(app, 'Contents/MacOS/towermd'), 'synthetic cli', { mode: 0o755 });
  const run = (file, args) => {
    if (file === 'plutil') return { status: 0, stdout: JSON.stringify({ CFBundleShortVersionString: '0.1.0', CFBundleVersion: '0.1.0', CFBundleExecutable: 'towermd-app' }) };
    if (file === 'lipo') return { status: 0, stdout: 'arm64\n' };
    if (file === 'otool') return { status: 0, stdout: args[0] === '-L' ? `${args[1]}:\n\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1.0.0)\n` : '' };
    if (file === 'codesign') return { status: 0, stderr: 'Signature=adhoc\nTeamIdentifier=not set\n' };
    if (file === 'spctl') return { status: 3, stderr: `${app}: rejected\nsource=no usable signature\n` };
    if (file === join(app, 'Contents/MacOS/towermd')) return { status: 0, stdout: 'towermd 0.1.0\n' };
    throw new Error(`unexpected ${file}`);
  };
  return { root, app, run };
}
test('missing bundled CLI is rejected', t => {
  const f = appFixture(t); rmSync(join(f.app, 'Contents/MacOS/towermd'));
  assert.throws(() => inspectApp({ app: f.app, target, version: '0.1.0' }, f.run), /CLI|towermd/);
});
test('wrong main architecture is rejected', t => {
  const f = appFixture(t);
  assert.throws(() => inspectApp({ app: f.app, target, version: '0.1.0' }, (file, args, options) => file === 'lipo' ? { status: 0, stdout: 'x86_64\n' } : f.run(file, args, options)), /architecture/);
});
test('stale plist is rejected', t => {
  const f = appFixture(t);
  assert.throws(() => inspectApp({ app: f.app, target, version: '0.1.0' }, (file, args, options) => file === 'plutil' ? { status: 0, stdout: JSON.stringify({ CFBundleShortVersionString: '0.0.0' }) } : f.run(file, args, options)), /plist|version/);
});
test('missing actual build receipts cannot assemble an empty release', t => {
  const f = appFixture(t);
  assert.throws(() => assembleAssets({ root: f.root, tag: 'v0.1.0', output: join(f.root, 'out'), builds: [] }, f.run), /build|receipt/);
});

test('embedded frontend layout reports static scope without requiring Resources/dist', t => {
  const f = appFixture(t), calls = [];
  const record = inspectApp({ app: f.app, target, version: '0.1.0' }, (file, args, options) => {
    calls.push({ file, args }); assert.equal(options.shell, false); return f.run(file, args, options);
  });
  assert.match(record.frontend, /not independently inspected/);
  assert.equal(record.gatekeeper.result, 'rejected');
  assert.equal(calls.filter(c => c.file === 'codesign').length, 3);
  assert(!calls.some(c => c.args.includes('find-identity')));
});
for (const [name, mutate, pattern] of [
  ['CLI is non-executable', f => chmodSync(join(f.app, 'Contents/MacOS/towermd'), 0o644), /file/],
  ['main is a symlink', f => { const p = join(f.app, 'Contents/MacOS/towermd-app'); rmSync(p); symlinkSync('towermd', p); }, /file/],
  ['bundle resource escapes', f => symlinkSync('/etc/hosts', join(f.app, 'Contents/escape')), /link escape/],
  ['bundle link is broken', f => symlinkSync('missing', join(f.app, 'Contents/broken')), /bundle link/],
]) test(`inspect rejects ${name}`, t => {
  const f = appFixture(t); mutate(f);
  assert.throws(() => inspectApp({ app: f.app, target, version: '0.1.0' }, f.run), pattern);
});
for (const [name, fileName, replace, pattern] of [
  ['CLI architecture differs', 'lipo', args => args[1].endsWith('/towermd') ? { status: 0, stdout: 'x86_64' } : null, /architecture/],
  ['fat architecture', 'lipo', () => ({ status: 0, stdout: 'arm64 x86_64' }), /architecture/],
  ['aarch64 is not a Mach-O name', 'lipo', () => ({ status: 0, stdout: 'aarch64' }), /architecture/],
  ['tool failure', 'lipo', () => ({ status: 1, stderr: 'sentinel' }), /lipo/],
  ['unsafe plist executable', 'plutil', () => ({ status: 0, stdout: JSON.stringify({ CFBundleShortVersionString: '0.1.0', CFBundleVersion: '0.1.0', CFBundleExecutable: '../towermd' }) }), /plist/],
  ['malformed plist JSON', 'plutil', () => ({ status: 0, stdout: '<plist>' }), /JSON/],
  ['publisher identity', 'codesign', args => args[0] === '-dv' ? { status: 0, stderr: 'Signature=adhoc\nTeamIdentifier=not set\nAuthority=Developer ID Application\n' } : null, /signature/],
  ['missing ad-hoc', 'codesign', args => args[0] === '-dv' ? { status: 0, stderr: 'TeamIdentifier=not set\n' } : null, /signature/],
  ['strict signature failure', 'codesign', args => args[0] === '--verify' ? { status: 1 } : null, /codesign/],
  ['spctl operational failure', 'spctl', () => ({ status: 1, stderr: 'internal error' }), /assessment/],
  ['spctl denial without rejected output', 'spctl', () => ({ status: 3, stderr: 'cannot assess' }), /assessment/],
  ['spctl deprecation', 'spctl', () => ({ status: 4, stderr: 'deprecated' }), /assessment/],
]) test(`inspect rejects ${name}`, t => {
  const f = appFixture(t);
  assert.throws(() => inspectApp({ app: f.app, target, version: '0.1.0' }, (file, args, options) => (file === fileName && replace(args)) || f.run(file, args, options)), pattern);
});
test('wrong host CLI version is rejected and a non-host target is never executed', t => {
  const f = appFixture(t), host = process.arch === 'arm64' ? target : 'x86_64-apple-darwin';
  const run = (file, args, options) => file === 'lipo' ? { status: 0, stdout: host === target ? 'arm64' : 'x86_64' }
    : file.endsWith('/towermd') ? { status: 0, stdout: 'towermd 0.0.0' } : f.run(file, args, options);
  assert.throws(() => inspectApp({ app: f.app, target: host, version: '0.1.0' }, run), /CLI version/);
  let executions = 0;
  const other = host === target ? 'x86_64-apple-darwin' : target;
  const record = inspectApp({ app: f.app, target: other, version: '0.1.0' }, (file, args, options) => {
    if (file === 'lipo') return { status: 0, stdout: other === target ? 'arm64' : 'x86_64' };
    if (file.endsWith('/towermd')) { executions++; throw new Error('must not execute'); }
    return f.run(file, args, options);
  });
  assert.equal(executions, 0); assert.match(record.cliVersion.scope, /static-only/);
});
test('spctl local policy acceptance is recorded without claiming notarization', t => {
  const f = appFixture(t);
  const r = inspectApp({ app: f.app, target, version: '0.1.0' }, (file, args, options) => file === 'spctl' ? { status: 0, stderr: `${f.app}: accepted\nsource=local policy\n` } : f.run(file, args, options));
  assert.equal(r.gatekeeper.result, 'accepted');
});
for (const [name, dependency, load, prepare, valid] of [
  ['developer absolute dylib', '/Users/developer/node/lib.dylib', '', () => {}, false],
  ['system prefix traversal usr', '/usr/lib/../../Users/developer/node/lib.dylib', '', () => {}, false],
  ['system prefix traversal framework', '/System/Library/../../Users/developer/rust/lib.dylib', '', () => {}, false],
  ['package loader dylib', '@loader_path/../Frameworks/lib.dylib', '', f => { mkdirSync(join(f.app, 'Contents/Frameworks')); writeFileSync(join(f.app, 'Contents/Frameworks/lib.dylib'), 'lib'); }, true],
  ['package rpath dylib', '@rpath/lib.dylib', 'Load command 0\n          cmd LC_RPATH\n      cmdsize 40\n         path @executable_path/../Frameworks (offset 12)\n', f => { mkdirSync(join(f.app, 'Contents/Frameworks')); writeFileSync(join(f.app, 'Contents/Frameworks/lib.dylib'), 'lib'); }, true],
  ['escaping loader dylib', '@loader_path/../../../escape.dylib', '', () => {}, false],
  ['developer rpath', '@rpath/lib.dylib', 'Load command 0\n cmd LC_RPATH\n path /Users/developer/lib (offset 12)\n', () => {}, false],
  ['missing package dylib', '@loader_path/lib.dylib', '', () => {}, false],
]) test(`dylib: ${name}`, t => {
  const f = appFixture(t); prepare(f);
  const inspect = () => inspectApp({ app: f.app, target, version: '0.1.0' }, (file, args, options) => file === 'otool' ? { status: 0, stdout: args[0] === '-L' ? `${args[1]}:\n\t${dependency} (compatibility version 1.0.0, current version 1.0.0)\n` : load } : f.run(file, args, options));
  if (valid) assert.equal(inspect().binaries.length, 2); else assert.throws(inspect, /dylib/);
});

function dmgFixture(t) {
  const f = appFixture(t), dmg = join(f.root, 'fixture.dmg'), mount = join(f.root, 'mounted');
  writeFileSync(dmg, 'synthetic dmg'); mkdirSync(mount); cpSync(f.app, join(mount, basename(f.app)), { recursive: true, verbatimSymlinks: true });
  const calls = [], entities = [{ 'dev-entry': '/dev/disk432' }, { 'dev-entry': '/dev/disk432s1' }, { 'dev-entry': '/dev/disk432s2', 'mount-point': mount }];
  const run = (file, args, options) => {
    calls.push({ file, args, options });
    if (file === 'hdiutil') return { status: 0, stdout: args[0] === 'attach' ? 'synthetic plist' : '' };
    if (file === 'plutil') { assert.equal(options.input, 'synthetic plist'); return { status: 0, stdout: JSON.stringify({ 'system-entities': entities }) }; }
    throw new Error('unexpected');
  };
  return { ...f, dmg, mount, calls, entities, run };
}
test('DMG full-tree comparison detaches only the returned whole device', t => {
  const f = dmgFixture(t); inspectDmg({ dmg: f.dmg, app: f.app }, f.run);
  assert.deepEqual(f.calls.filter(c => c.file === 'hdiutil').map(c => c.args), [['verify', f.dmg], ['attach', '-readonly', '-nobrowse', '-plist', f.dmg], ['detach', '/dev/disk432']]);
});
for (const [name, change, pattern] of [
  ['file bytes', f => writeFileSync(join(f.mount, 'Tower.md.app/Contents/MacOS/towermd'), 'changed'), /differs/],
  ['file mode', f => chmodSync(join(f.mount, 'Tower.md.app/Contents/MacOS/towermd'), 0o644), /differs/],
  ['extra resource', f => writeFileSync(join(f.mount, 'Tower.md.app/Contents/extra'), 'extra'), /differs/],
  ['bundle root mode', f => chmodSync(join(f.mount, 'Tower.md.app'), 0o700), /differs/],
  ['no App', f => rmSync(join(f.mount, 'Tower.md.app'), { recursive: true }), /count/],
  ['two Apps', f => mkdirSync(join(f.mount, 'Other.app')), /count/],
  ['mount count', f => f.entities.push({ 'dev-entry': '/dev/disk432s3', 'mount-point': f.mount }), /mount count/],
]) test(`DMG ${name} failure still detaches its own device`, t => {
  const f = dmgFixture(t); change(f);
  assert.throws(() => inspectDmg({ dmg: f.dmg, app: f.app }, f.run), pattern);
  assert.deepEqual(f.calls.at(-1).args, ['detach', '/dev/disk432']);
});
test('DMG comparison and detach errors are both preserved', t => {
  const f = dmgFixture(t); writeFileSync(join(f.mount, 'Tower.md.app/Contents/extra'), 'x');
  assert.throws(() => inspectDmg({ dmg: f.dmg, app: f.app }, (file, args, options) => args[0] === 'detach' ? { status: 1 } : f.run(file, args, options)), /differs; cleanup failed/);
});
for (const [name, file, operation, result, pattern, detached] of [
  ['verify fails', 'hdiutil', 'verify', { status: 1 }, /verify/, false],
  ['attach fails after returning device', 'hdiutil', 'attach', { status: 1, stdout: 'synthetic plist' }, /attach exit/, true],
  ['malformed attach plist', 'plutil', '-convert', { status: 0, stdout: 'broken' }, /plist/, false],
  ['empty entities', 'plutil', '-convert', { status: 0, stdout: '{"system-entities":[]}' }, /entities/, false],
]) test(`DMG ${name}`, t => {
  const f = dmgFixture(t);
  assert.throws(() => inspectDmg({ dmg: f.dmg, app: f.app }, (name, args, options) => name === file && args[0] === operation ? result : f.run(name, args, options)), pattern);
  assert.equal(f.calls.some(c => c.args[0] === 'detach'), detached);
});

const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const gitIdentity = ['-c', 'user.name=codex/gpt-6', '-c', 'user.email=codex@towermd.invalid', '-c', 'commit.gpgsign=false'];
function git(root, args) {
  const r = spawnSync('git', [...gitIdentity, ...args], { cwd: root });
  assert.equal(r.status, 0, String(r.stderr)); return r.stdout;
}
function put(root, path, bytes, mode = 0o644) {
  mkdirSync(dirname(join(root, path)), { recursive: true }); writeFileSync(join(root, path), bytes, { mode });
}
// Structural bytes only; real signature verification remains inspectApp.
function signedCli(target, large = false) {
  const arm = target === 'aarch64-apple-darwin', page = arm ? 16384 : 4096;
  const start = page + 64, size = large ? page + 64 : 64, bytes = Buffer.alloc(start + size);
  for (const [offset, value] of [[0, 0xfeedfacf], [4, arm ? 0x0100000c : 0x01000007], [12, 2], [16, 3], [20, 160]]) bytes.writeUInt32LE(value, offset);
  for (const [offset, name, fileoff, filesize] of [[32, '__TEXT', 0, page], [104, '__LINKEDIT', page, bytes.length - page]]) {
    bytes.writeUInt32LE(0x19, offset); bytes.writeUInt32LE(72, offset + 4); bytes.write(name, offset + 8);
    bytes.writeBigUInt64LE(0x100000000n + BigInt(fileoff), offset + 24);
    bytes.writeBigUInt64LE(BigInt(Math.ceil(filesize / page) * page), offset + 32);
    bytes.writeBigUInt64LE(BigInt(fileoff), offset + 40); bytes.writeBigUInt64LE(BigInt(filesize), offset + 48);
  }
  bytes.writeUInt32LE(0x1d, 176); bytes.writeUInt32LE(16, 180); bytes.writeUInt32LE(start, 184); bytes.writeUInt32LE(size, 188);
  bytes.fill(0x45, 192, start); bytes.fill(large ? 0xa1 : 0xb2, start);
  return bytes;
}
function releaseFixture(t) {
  const parent = realpathSync(mkdtempSync(join(tmpdir(), 'towermd-release-assets-test-')));
  t.after(() => rmSync(parent, { recursive: true, force: true }));
  const root = join(parent, 'source'); mkdirSync(root); git(root, ['init', '-b', 'main']);
  put(root, '.gitignore', '/src-tauri/binaries/\n');
  for (const path of ['Cargo.lock', 'src-tauri/Cargo.lock', 'pnpm-lock.yaml', 'src-tauri/tauri.conf.json', 'src-tauri/tauri.unsigned.conf.json', 'src-tauri/src/lib.rs', 'index.html', 'vite.config.ts',
    'scripts/project-pnpm.mjs', 'scripts/build-app.mjs', 'scripts/stage-cli.mjs', 'scripts/release/build.mjs']) put(root, path, `synthetic source: ${path}\n`);
  put(root, 'CHANGELOG.md', '# Changelog\n\n## Unreleased\n\n## v0.1.0 — 2026-10-17\n\n- Frozen English change.\n');
  put(root, 'CHANGELOG.zh-CN.md', '# 更新日志\n\n## 未发布\n\n## v0.1.0 — 2026-10-17\n\n- 冻结中文变化。\n');
  put(root, 'skills/.claude-plugin/plugin.json', JSON.stringify({ name: 'towermd', version: '0.1.0', description: 'Synthetic native package fixture' }));
  put(root, 'skills/INSTALL.md', 'Synthetic test package. Read [contract](references/session-contract.md).\n');
  put(root, 'skills/references/session-contract.md', 'Synthetic fixture for tools; not actual P7 skills.\n');
  put(root, 'skills/references/cli-contracts.json', '{}\n');
  put(root, 'skills/scripts/check-contracts.mjs', '// Synthetic fixture, never executed.\n', 0o755);
  put(root, 'skills/references/a space.md', 'space filename\n');
  symlinkSync('session-contract.md', join(root, 'skills/references/internal-link'));
  for (const name of ['init', 'baseline', 'claim', 'ask', 'propose', 'finish']) put(root, `skills/skills/towermd-${name}/SKILL.md`, `---\nname: towermd-${name}\ndescription: Synthetic ${name} native skill fixture.\n---\n\nRead [contract](../../references/session-contract.md). Use the CLI for ${name}; wait for the App when required.\n`);
  const freeze = () => {
    git(root, ['add', '--', '.']); git(root, ['commit', '-m', 'synthetic asset fixture']); git(root, ['tag', '-f', 'v0.1.0']);
  };
  freeze();
  const commit = git(root, ['rev-parse', 'HEAD']).toString().trim(), tree = git(root, ['rev-parse', 'HEAD^{tree}']).toString().trim();
  const sources = git(root, ['ls-tree', '-rz', '--full-tree', 'HEAD']).toString().split('\0').filter(Boolean).map(row => {
    const [, mode, blob, path] = /^(\d+) blob ([a-f0-9]+)\t([\s\S]+)$/.exec(row);
    return { path, mode, blob, sha256: digest(git(root, ['cat-file', 'blob', blob])) };
  });
  const builds = [], mounts = new Map();
  const emptyHash = digest('');
  const command = (file, args) => ({ file, args, started: '2026-10-17T00:00:00.000Z', finished: '2026-10-17T00:00:01.000Z', status: 0, failed: false, stdoutSha256: emptyHash, stderrSha256: emptyHash });
  for (const target of ['aarch64-apple-darwin', 'x86_64-apple-darwin']) {
    const output = join(parent, `build-${target}`), app = join(output, 'target', target, 'release/bundle/macos/Tower.md.app'), dmg = join(output, 'target', target, 'release/bundle/dmg/Tower.dmg');
    const cli = signedCli(target, true), stage = join(root, 'src-tauri/binaries', `towermd-${target}`), script = join(root, 'scripts/project-pnpm.mjs');
    put(parent, app.slice(parent.length + 1) + '/Contents/MacOS/towermd-app', `synthetic main ${target}`, 0o755);
    put(parent, app.slice(parent.length + 1) + '/Contents/MacOS/towermd', signedCli(target), 0o755);
    put(parent, app.slice(parent.length + 1) + '/Contents/Info.plist', 'synthetic plist');
    put(parent, app.slice(parent.length + 1) + '/Contents/Resources/icon.icns', 'icon');
    put(root, stage.slice(root.length + 1), cli, 0o755);
    put(output, `target/${target}/release/towermd`, cli, 0o755);
    put(parent, dmg.slice(parent.length + 1), `synthetic dmg ${target}`);
    const mount = join(parent, `mount-${target}`); mkdirSync(mount); cpSync(app, join(mount, basename(app)), { recursive: true, verbatimSymlinks: true }); mounts.set(dmg, mount);
    const receipt = { tag: 'v0.1.0', commit, tree, tagObject: commit, target, app, dmg,
      tools: Object.fromEntries(['node', 'pnpm', 'cargo', 'rustc', 'tauri', 'sdk', 'xcode'].map(k => [k, `synthetic ${k}`])),
      lockfiles: Object.fromEntries(['Cargo.lock', 'src-tauri/Cargo.lock', 'pnpm-lock.yaml'].map(p => [p, sources.find(f => f.path === p).sha256])), sources,
      commands: [command('/synthetic/cargo', ['build', '--locked', '-p', 'towermd', '--release', '--target', target]), command('/synthetic/cargo', ['metadata', '--locked', '--no-deps', '--format-version', '1']), command('lipo', ['-archs', stage]), command(process.execPath, [script, 'build']), command(process.execPath, [script, 'tauri', 'build', '--target', target, '--bundles', 'app,dmg', '--config', 'src-tauri/tauri.unsigned.conf.json', '--', '--locked'])] };
    builds.push(receipt); writeFileSync(join(output, 'build-receipt.json'), JSON.stringify(receipt, null, 2) + '\n');
  }
  const calls = [], zipped = new Map(); let attached;
  const run = (file, args, options = {}) => {
    calls.push({ file, args, cwd: options.cwd }); assert.equal(options.shell, false);
    if (file === 'git' || file === 'tar' || file === 'gzip') return spawnSync(file, args, options);
    if (file === 'plutil') return { status: 0, stdout: args.at(-1) === '-' ? JSON.stringify({ 'system-entities': [{ 'dev-entry': '/dev/disk876' }, { 'dev-entry': '/dev/disk876s2', 'mount-point': attached }] }) : JSON.stringify({ CFBundleShortVersionString: '0.1.0', CFBundleVersion: '0.1.0', CFBundleExecutable: 'towermd-app' }) };
    if (file === 'lipo') return { status: 0, stdout: args[1].includes('aarch64') ? 'arm64\n' : 'x86_64\n' };
    if (file === 'otool') return { status: 0, stdout: args[0] === '-L' ? `${args[1]}:\n\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1.0.0)\n` : '' };
    if (file === 'codesign') return { status: 0, stderr: 'Signature=adhoc\nTeamIdentifier=not set\n' };
    if (file === 'spctl') return { status: 3, stderr: `${args.at(-1)}: rejected\nsource=no usable signature\n` };
    if (file === 'hdiutil') { if (args[0] === 'attach') attached = mounts.get(args.at(-1)); return { status: 0, stdout: args[0] === 'attach' ? 'synthetic mount plist' : '' }; }
    if (file === 'ditto') {
      if (args[0] === '-c') { zipped.set(args.at(-1), args.at(-2)); writeFileSync(args.at(-1), `synthetic ZIP ${args.at(-2)}`); }
      else cpSync(zipped.get(args.at(-2)), join(args.at(-1), 'Tower.md.app'), { recursive: true, verbatimSymlinks: true });
      return { status: 0 };
    }
    if (file.endsWith('/Contents/MacOS/towermd')) return { status: 0, stdout: 'towermd 0.1.0\n' };
    throw new Error(`unexpected subprocess: ${file}`);
  };
  const args = { root, tag: 'v0.1.0', output: join(parent, 'assets'), builds };
  const save = () => builds.forEach(b => writeFileSync(join(b.app, '../../../../../../build-receipt.json'), JSON.stringify(b, null, 2) + '\n'));
  const rebind = () => {
    const commit = git(root, ['rev-parse', 'HEAD']).toString().trim(), tree = git(root, ['rev-parse', 'HEAD^{tree}']).toString().trim();
    const sources = git(root, ['ls-tree', '-rz', '--full-tree', 'HEAD']).toString().split('\0').filter(Boolean).map(row => {
      const [, mode, blob, path] = /^(\d+) blob ([a-f0-9]+)\t([\s\S]+)$/.exec(row);
      return { path, mode, blob, sha256: digest(git(root, ['cat-file', 'blob', blob])) };
    });
    builds.forEach(b => Object.assign(b, { commit, tree, tagObject: commit, sources })); save();
  };
  return { parent, root, commit, builds, mounts, calls, run, args, save, freeze, rebind };
}

test('real Git archive preserves frozen bytes, executable mode, internal symlink and space name', t => {
  const f = releaseFixture(t), workspace = join(f.parent, 'archive'); mkdirSync(workspace);
  const defaultTar = join(f.parent, 'default.tar');
  assert.equal(f.run('git', ['archive', '--format=tar', `--prefix=${prefix}`, '--output', defaultTar, f.commit, 'skills'], { cwd: f.root, shell: false }).status, 0);
  const defaultEntries = readArchive(readFileSync(defaultTar), prefix);
  t.diagnostic(`Actual git archive default modes: regular=${defaultEntries.find(e => e.type === 'file' && e.path.endsWith('INSTALL.md')).mode.toString(8)}, executable=${defaultEntries.find(e => e.path.endsWith('check-contracts.mjs')).mode.toString(8)}; Task5 fixes tar.umask=0022 to 644/755.`);
  put(f.root, 'skills/untracked.md', 'must never enter frozen package');
  const result = archiveSkills({ root: f.root, commit: f.commit, version: '0.1.0', workspace, destination: join(f.parent, 'skills.tar.gz') }, f.run);
  assert(result.files > 6);
  assert.equal(readFileSync(join(workspace, 'extracted/towermd-skills-0.1.0/skills/references/a space.md'), 'utf8'), 'space filename\n');
  assert(f.calls.some(c => c.file === 'git' && c.args.slice(0, 3).join(' ') === '-c tar.umask=0022 archive'));
  assert(!existsSync(join(workspace, 'extracted/towermd-skills-0.1.0/skills/untracked.md')));
});
test('complete synthetic two-target assembly yields exactly nine acyclic assets', t => {
  const f = releaseFixture(t), result = assembleAssets(f.args, f.run);
  assert.deepEqual(readdirSync(f.args.output).sort(), expectedAssets('0.1.0'));
  assert.equal(result.manifest.artifacts.length, 5);
  for (const build of f.builds) {
    const bundled = readFileSync(join(build.app, 'Contents/MacOS/towermd'));
    const staged = readFileSync(build.commands.find(c => c.file === 'lipo').args[1]);
    assert.equal(result.manifest.targets.find(i => i.target === build.target).cliSha256, digest(bundled));
    assert.notEqual(digest(bundled), digest(staged));
  }
  assert(result.manifest.artifacts.every(a => !/MANIFEST|NOTES|SUMS/.test(a.name)));
  for (const artifact of result.manifest.artifacts) {
    const bytes = readFileSync(join(f.args.output, artifact.name));
    assert.equal(artifact.size, bytes.length); assert.equal(artifact.sha256, digest(bytes));
  }
  const sums = readFileSync(join(f.args.output, 'SHA256SUMS'), 'utf8'); assert.equal(sums.trim().split('\n').length, 8);
  const buffers = new Map(expectedAssets('0.1.0').filter(n => n !== 'SHA256SUMS').map(n => [n, readFileSync(join(f.args.output, n))]));
  verifySums(sums, buffers); buffers.set('RELEASE-NOTES.en.md', Buffer.from('tampered'));
  assert.throws(() => verifySums(sums, buffers), /checksum mismatch/);
  assert.match(readFileSync(join(f.args.output, 'RELEASE-NOTES.en.md'), 'utf8'), /Frozen English change/);
  assert.match(readFileSync(join(f.args.output, 'RELEASE-NOTES.zh-CN.md'), 'utf8'), /冻结中文变化/);
  assert(existsSync(join(f.builds[0].app, 'Contents/MacOS/towermd-app')));
  assert(existsSync(f.args.output + '.logs/commands.jsonl'));
});
for (const [name, mutate, pattern] of [
  ['missing target build', f => f.args.builds.pop(), /two build/],
  ['duplicate target', f => { f.builds[1].target = target; }, /two build/],
  ['wrong commit', f => { f.builds[0].commit = 'f'.repeat(40); f.save(); }, /identity/],
  ['disk receipt drift', f => { f.builds[0].tools.node = 'changed'; }, /differs from disk/],
  ['lock drift', f => { f.builds[0].lockfiles['Cargo.lock'] = '0'.repeat(64); f.save(); }, /lockfiles/],
  ['source drift', f => { f.builds[0].sources = []; f.save(); }, /sources/],
  ['failed build command', f => { f.builds[0].commands[0].status = 1; f.save(); }, /successful build/],
  ['frontend command missing', f => { f.builds[0].commands.splice(3, 1); f.save(); }, /source chain/],
  ['wrong stage triple', f => { f.builds[0].commands[2].args[1] = join(f.root, 'src-tauri/binaries/towermd-wrong'); f.save(); }, /staged CLI path/],
  ['CLI hash drift', f => writeFileSync(join(f.builds[0].app, 'Contents/MacOS/towermd'), 'changed'), /CLI hash/],
  ['target/stage CLI hash drift', f => writeFileSync(f.builds[0].commands.find(c => c.file === 'lipo').args[1], signedCli(target)), /target\/staged CLI hash/],
  ['structured bundled CLI code drift', f => {
    const path = join(f.builds[0].app, 'Contents/MacOS/towermd'), bytes = readFileSync(path); bytes[256] ^= 1; writeFileSync(path, bytes);
  }, /non-signature bytes changed/],
  ['missing DMG', f => rmSync(f.builds[1].dmg), /bundle count/],
  ['multiple DMGs', f => writeFileSync(join(dirname(f.builds[0].dmg), 'old.dmg'), 'old'), /bundle count/],
  ['P7 directory missing', f => { git(f.root, ['rm', '-r', '--', 'skills']); f.freeze(); f.rebind(); }, /missing native skill input/],
  ['actual build source changed', f => writeFileSync(join(f.root, 'index.html'), 'changed after receipt'), /build source/],
]) test(`assembly refuses ${name} without a successful manifest`, t => {
  const f = releaseFixture(t); mutate(f);
  assert.throws(() => assembleAssets(f.args, f.run), pattern);
  assert(!existsSync(join(f.args.output, 'BUILD-MANIFEST.json')));
});
for (const name of ['latest.json', 'unexpected.sig']) test(`assembly rejects extra ${name}`, t => {
  const f = releaseFixture(t);
  const run = (file, args, options) => { const r = f.run(file, args, options); if (file === 'ditto' && args[0] === '-c') writeFileSync(join(f.args.output, name), 'extra'); return r; };
  assert.throws(() => assembleAssets(f.args, run), /asset set/); assert(!existsSync(join(f.args.output, 'BUILD-MANIFEST.json')));
});
for (const attribute of ['export-ignore', 'export-subst']) test(`Git ${attribute} is rejected before extraction when it changes frozen bytes`, t => {
  const f = releaseFixture(t), workspace = join(f.parent, 'archive'); mkdirSync(workspace);
  put(f.root, '.gitattributes', `"skills/references/a space.md" ${attribute}\n`);
  if (attribute === 'export-subst') put(f.root, 'skills/references/a space.md', '$Format:%H$\n');
  f.freeze(); const commit = git(f.root, ['rev-parse', 'HEAD']).toString().trim();
  assert.throws(() => archiveSkills({ root: f.root, commit, version: '0.1.0', workspace, destination: join(f.parent, 'skills.gz') }, f.run), /archive differs|member set/);
  assert(!f.calls.some(c => c.file === 'tar'));
});
for (const [name, change, pattern] of [
  ['missing six-skill member', f => git(f.root, ['rm', '--', 'skills/skills/towermd-ask/SKILL.md']), /six native|missing/],
  ['placeholder skill', f => put(f.root, 'skills/skills/towermd-ask/SKILL.md', 'stub'), /skill body/],
  ['executable seed', f => put(f.root, 'skills/skills/towermd-seed/SKILL.md', 'seed'), /six native/],
  ['seed script entry', f => put(f.root, 'skills/scripts/towermd-seed.sh', 'exit 0\n', 0o755), /seed entry/],
  ['escaping link', f => symlinkSync('../../outside', join(f.root, 'skills/escape')), /reference|symlink/],
  ['missing resource', f => git(f.root, ['rm', '--', 'skills/references/session-contract.md']), /missing/],
]) test(`native archive rejects ${name}`, t => {
  const f = releaseFixture(t); change(f); f.freeze(); const workspace = join(f.parent, 'archive'); mkdirSync(workspace);
  assert.throws(() => archiveSkills({ root: f.root, commit: git(f.root, ['rev-parse', 'HEAD']).toString().trim(), version: '0.1.0', workspace, destination: join(f.parent, 'skills.gz') }, f.run), pattern);
  assert(!f.calls.some(c => c.file === 'tar'));
});

function tarEntry(path, kind = '0', body = Buffer.alloc(0), target = '', mode = kind === '5' ? 0o755 : kind === '2' ? 0o777 : 0o644) {
  const header = Buffer.alloc(512); header.write(path, 0, 100);
  const octal = (value, start, length) => header.write(value.toString(8).padStart(length - 1, '0') + '\0', start, length);
  octal(mode, 100, 8); octal(0, 108, 8); octal(0, 116, 8); octal(body.length, 124, 12); octal(0, 136, 12);
  header.fill(32, 148, 156); header[156] = kind.charCodeAt(0); header.write(target, 157, 100); header.write('ustar\0', 257, 6); header.write('00', 263, 2);
  const sum = [...header].reduce((a, b) => a + b, 0); header.write(sum.toString(8).padStart(6, '0') + '\0 ', 148, 8);
  return Buffer.concat([header, body, Buffer.alloc((512 - body.length % 512) % 512)]);
}
function paxField(key, value) {
  const row = `${key}=${value}\n`; let length = Buffer.byteLength(row) + 2;
  while (Buffer.byteLength(`${length} ${row}`) !== length) length = Buffer.byteLength(`${length} ${row}`);
  return Buffer.from(`${length} ${row}`);
}
const prefix = 'towermd-skills-0.1.0/';
const tarBytes = records => Buffer.concat([...records, Buffer.alloc(1024)]);
for (const [name, records] of [
  ['absolute member', [tarEntry('/escape')]],
  ['dotdot member', [tarEntry(prefix + 'skills/../../escape')]],
  ['wrong prefix', [tarEntry('elsewhere/skills/a')]],
  ['hardlink', [tarEntry(prefix + 'skills/a', '1', Buffer.alloc(0), '/etc/hosts')]],
  ['device', [tarEntry(prefix + 'skills/a', '3')]],
  ['duplicate', [tarEntry(prefix + 'skills/a'), tarEntry(prefix + 'skills/a')]],
  ['absolute symlink', [tarEntry(prefix + 'skills/a', '2', Buffer.alloc(0), '/etc/hosts')]],
  ['escaping symlink', [tarEntry(prefix + 'skills/a', '2', Buffer.alloc(0), '../../../escape')]],
  ['link cycle', [tarEntry(prefix + 'skills/a', '2', Buffer.alloc(0), 'b'), tarEntry(prefix + 'skills/b', '2', Buffer.alloc(0), 'a')]],
  ['symlink parent write', [tarEntry(prefix + 'skills/a', '2', Buffer.alloc(0), 'b'), tarEntry(prefix + 'skills/b'), tarEntry(prefix + 'skills/a/file')]],
  ['symlink directory parent write', [tarEntry(prefix + 'skills/a', '2', Buffer.alloc(0), 'b'), tarEntry(prefix + 'skills/b/', '5'), tarEntry(prefix + 'skills/a/file')]],
  ['PAX path override', [tarEntry('pax', 'x', paxField('path', '/escape')), tarEntry(prefix + 'skills/safe')]],
  ['PAX linkpath override', [tarEntry('pax', 'x', paxField('linkpath', '/escape')), tarEntry(prefix + 'skills/safe', '2')]],
  ['global PAX path', [tarEntry('pax', 'g', paxField('path', '/escape')), tarEntry(prefix + 'skills/safe')]],
]) test(`tar preflight rejects ${name}`, () => assert.throws(() => readArchive(tarBytes(records), prefix), /archive|PAX/));
test('tar parser accepts PAX long paths and valid internal relative symlinks', () => {
  const long = prefix + 'skills/' + 'a'.repeat(140), bytes = tarBytes([tarEntry('pax', 'x', paxField('path', long)), tarEntry('short', '0', Buffer.from('file')), tarEntry('pax', 'x', paxField('linkpath', 'a'.repeat(140))), tarEntry(prefix + 'skills/link', '2')]);
  assert.equal(readArchive(bytes, prefix).length, 2);
});
test('malicious archive injected after Git is rejected before any extract call', t => {
  const f = releaseFixture(t), workspace = join(f.parent, 'archive'); mkdirSync(workspace);
  assert.throws(() => archiveSkills({ root: f.root, commit: f.commit, version: '0.1.0', workspace, destination: join(f.parent, 'skills.gz') }, (file, args, options) => {
    const result = f.run(file, args, options);
    if (file === 'git' && args.includes('archive')) writeFileSync(args[args.indexOf('--output') + 1], tarBytes([tarEntry('/escape')]));
    return result;
  }), /archive path/);
  assert(!f.calls.some(c => c.file === 'tar'));
});
test('original App mutation during assembly invalidates the result', t => {
  const f = releaseFixture(t);
  assert.throws(() => assembleAssets(f.args, (file, args, options) => {
    const result = f.run(file, args, options);
    if (file === 'gzip') writeFileSync(join(f.builds[0].app, 'Contents/MacOS/towermd-app'), 'late mutation');
    return result;
  }), /bundle inputs changed/);
  assert(!existsSync(join(f.args.output, 'BUILD-MANIFEST.json')));
});
for (const position of [0, 2, 3, 4]) test(`metadata sequence rejects position ${position} outside build-before-lipo interval`, t => {
  const f = releaseFixture(t);
  for (const build of f.builds) {
    const index = build.commands.findIndex(c => c.args[0] === 'metadata');
    const [metadata] = build.commands.splice(index, 1); build.commands.splice(position, 0, metadata);
  }
  f.save();
  assert.throws(() => assembleAssets(f.args, f.run), /source chain|metadata|sequence/);
  assert(!existsSync(join(f.args.output, 'BUILD-MANIFEST.json')));
  assert(!existsSync(join(f.args.output, 'SHA256SUMS')));
});
for (const name of [
  'tower.md-0.1.0-aarch64.app.zip', 'tower.md-0.1.0-aarch64.dmg',
  'tower.md-0.1.0-x86_64.app.zip', 'tower.md-0.1.0-x86_64.dmg', 'towermd-skills-0.1.0.tar.gz',
]) test(`late recorded binary drift rejects ${name}`, t => {
  const f = releaseFixture(t); let changed = false;
  assert.throws(() => assembleAssets(f.args, (file, args, options) => {
    const result = f.run(file, args, options), path = join(f.args.output, name);
    const ready = name.endsWith('.tar.gz') ? file === 'git' && args[0] === 'rev-parse' && existsSync(path) : file === 'gzip';
    if (!changed && ready) {
      if (name.endsWith('aarch64.app.zip')) writeFileSync(path, 'synthetic later changed ZIP');
      else { const bytes = readFileSync(path); bytes[0] ^= 1; writeFileSync(path, bytes); }
      changed = true;
    }
    return result;
  }), /binary artifact changed/);
  assert(changed); assert(!existsSync(join(f.args.output, 'BUILD-MANIFEST.json')));
  assert(!existsSync(join(f.args.output, 'SHA256SUMS')));
});
test('frozen secret-like paths are rejected before reading their blob', t => {
  const f = releaseFixture(t), workspace = join(f.parent, 'archive'); mkdirSync(workspace);
  put(f.root, '.env.release', 'synthetic sentinel must not be read'); f.freeze();
  const oid = git(f.root, ['rev-parse', 'HEAD:.env.release']).toString().trim();
  assert.throws(() => archiveSkills({ root: f.root, commit: git(f.root, ['rev-parse', 'HEAD']).toString().trim(), version: '0.1.0', workspace, destination: join(f.parent, 'skills.gz') }, f.run), /forbidden source/);
  assert(!f.calls.some(c => c.file === 'git' && c.args[0] === 'cat-file' && c.args[2] === oid));
});
test('CLI reads the two actual build directories and dispatches the same assembly contract', t => {
  const f = releaseFixture(t);
  const result = main(['--root', f.root, '--tag', 'v0.1.0', '--output', f.args.output,
    '--arm-build', join(f.builds[0].app, '../../../../../..'), '--intel-build', join(f.builds[1].app, '../../../../../..')], f.run);
  assert.equal(result.assets.length, 9);
});
for (const args of [[], ['--force'], ['--root', 'x', '--root', 'y'], ['--root'], ['--tag', '../v0.1.0']]) test(`CLI rejects ${JSON.stringify(args)}`, () => assert.throws(() => main(args), /argument/));
