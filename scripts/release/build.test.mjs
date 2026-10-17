import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import fsp from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import { syncBuiltinESMExports } from 'node:module';
import { tmpdir, homedir } from 'node:os';
import { join, dirname } from 'node:path';
import { buildFrozen, main } from './build.mjs';

const targets = ['aarch64-apple-darwin', 'x86_64-apple-darwin'];
const overlay = { bundle: { active: true, targets: ['app', 'dmg'], createUpdaterArtifacts: false,
  externalBin: ['binaries/towermd'], macOS: { signingIdentity: '-', providerShortName: null } } };
const sha = '0'.repeat(40);
function fixture(t, control = {}) {
  const area = fs.realpathSync(fs.mkdtempSync(join(tmpdir(), 'tower frozen 中文 ')));
  t.after(() => fs.rmSync(area, { recursive: true, force: true }));
  const root = join(area, 'repo'), output = join(area, 'output'); fs.mkdirSync(root);
  const write = (path, text, mode) => { const p = join(root, path); fs.mkdirSync(dirname(p), { recursive: true }); fs.writeFileSync(p, text, mode ? { mode } : undefined); };
  const files = {
    'Cargo.toml': '[workspace]\nmembers = ["crates/towermd-core", "crates/towermd"]\nexclude = ["src-tauri"]\n[workspace.package]\nversion = "0.1.0"\n',
    'crates/towermd/Cargo.toml': '[package]\nname = "towermd"\nversion.workspace = true\n',
    'crates/towermd-core/Cargo.toml': '[package]\nname = "towermd-core"\nversion.workspace = true\n',
    'src-tauri/Cargo.toml': '[package]\nname = "towermd-app"\nversion = "0.1.0"\n[workspace]\n',
    'Cargo.lock': 'version = 4\n[[package]]\nname = "towermd"\nversion = "0.1.0"\n[[package]]\nname = "towermd-core"\nversion = "0.1.0"\n',
    'src-tauri/Cargo.lock': 'version = 4\n[[package]]\nname = "towermd-app"\nversion = "0.1.0"\n',
    'package.json': '{"version":"0.1.0"}',
    'pnpm-lock.yaml': "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies: {}\n",
    'src-tauri/tauri.conf.json': JSON.stringify({ productName: 'Tower.md', version: '0.1.0', ...overlay }),
    'src-tauri/tauri.unsigned.conf.json': JSON.stringify(overlay),
    '.gitignore': '/dist/\n/src-tauri/binaries/\n/.tools/\n',
    'CHANGELOG.md': '# Changelog\n\n## Unreleased\n\n## v0.1.0 — 2026-10-17\n\n- Built App.\n',
    'CHANGELOG.zh-CN.md': '# 更新日志\n\n## 未发布\n\n## v0.1.0 — 2026-10-17\n\n- 构建应用。\n',
  };
  for (const [path, text] of Object.entries(files)) write(path, text);
  write('.tools/pnpm/node_modules/pnpm/bin/pnpm.cjs', 'fixture');
  const gitEnv = { HOME: area, PATH: '/usr/bin:/bin', GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_NOSYSTEM: '1',
    GIT_AUTHOR_NAME: 'fixture', GIT_AUTHOR_EMAIL: 'fixture@example.invalid', GIT_COMMITTER_NAME: 'fixture', GIT_COMMITTER_EMAIL: 'fixture@example.invalid' };
  const git = (...args) => { const r = spawnSync('git', args, { cwd: root, env: gitEnv, encoding: 'utf8' }); assert.equal(r.status, 0, r.stderr); return r.stdout.trim(); };
  git('init', '-b', 'main'); git('add', '.'); git('-c', 'commit.gpgsign=false', 'commit', '-m', 'fixture');
  git('tag', 'v0.1.0'); git('checkout', '--detach', 'v0.1.0');
  const commit = git('rev-parse', 'HEAD'), calls = [];
  let tagReads = 0;
  const metadata = (desktop, env) => {
    const names = desktop ? ['towermd-app'] : ['towermd-core', 'towermd'];
    return { target_directory: env.CARGO_TARGET_DIR, workspace_root: join(root, desktop ? 'src-tauri' : ''), workspace_members: names,
      packages: names.map(name => ({ id: name, name, version: '0.1.0', manifest_path: join(root, name === 'towermd-app' ? 'src-tauri/Cargo.toml' : `crates/${name}/Cargo.toml`) })) };
  };
  const run = (file, args, options) => {
    calls.push({ file, args: [...args], options });
    assert.equal(options.cwd, root); assert.equal(options.shell, false);
    const ok = stdout => ({ status: 0, stdout: stdout ?? '', stderr: '' });
    if (file === 'git') {
      if (control.missingTag && args.some(a => a.includes('v0.1.0'))) return { status: 1, stderr: 'SENTINEL' };
      const result = spawnSync(file, args, { ...options, encoding: 'utf8' });
      if (control.wrongHead && args.includes('HEAD') && args[0] === 'rev-parse') result.stdout = sha + '\n';
      if (args.at(-1) === 'refs/tags/v0.1.0' && ++tagReads === 3 && control.lateTag) result.stdout = sha + '\n';
      return result;
    }
    if (args[0] === '--version' || args.includes('--version') || file === 'xcodebuild' || file === 'xcrun') return ok('fixture version\n');
    if (args[0] === 'metadata') return ok(JSON.stringify(metadata(args.includes('--manifest-path'), options.env)));
    if (args[0] === 'build' && file.endsWith('/cargo')) {
      if (control.stageExit) return { status: 7, stdout: 'SECRET', stderr: 'SECRET' };
      if (!control.missingCli) {
        const target = args[args.indexOf('--target') + 1], path = join(options.env.CARGO_TARGET_DIR, target, 'release/towermd');
        fs.mkdirSync(dirname(path), { recursive: true }); fs.writeFileSync(path, target, { mode: control.nonExecutableCli ? 0o644 : 0o755 });
      }
      return ok();
    }
    if (file === 'lipo') return ok(control.wrongArch ? 'x86_64 arm64\n' : args.at(-1).includes('aarch64') ? 'arm64\n' : 'x86_64\n');
    if (file === process.execPath && args.at(-1) === 'build' && !args.includes('tauri')) { write('dist/index.html', '<html>fixture</html>'); return ok(); }
    if (file === process.execPath && args.includes('tauri') && args.includes('build')) {
      if (control.buildExit) return { status: 8, stdout: 'SECRET', stderr: 'SECRET' };
      const target = args[args.indexOf('--target') + 1], bundle = join(options.env.CARGO_TARGET_DIR, target, 'release/bundle');
      if (!control.noApp) fs.mkdirSync(join(bundle, 'macos/Tower.md.app'), { recursive: true });
      if (control.twoApp) fs.mkdirSync(join(bundle, 'macos/Other.app'));
      if (!control.noDmg) { fs.mkdirSync(join(bundle, 'dmg'), { recursive: true }); fs.writeFileSync(join(bundle, 'dmg/Tower.md.dmg'), 'dmg'); }
      if (control.twoDmg) fs.writeFileSync(join(bundle, 'dmg/Other.dmg'), 'dmg');
      if (control.drift) write(control.drift, fs.readFileSync(join(root, control.drift), 'utf8') + '\n');
      if (control.bundleLink) {
        fs.rmSync(join(bundle, 'macos'), { recursive: true });
        fs.mkdirSync(join(area, 'outside/Tower.md.app'), { recursive: true });
        fs.symlinkSync(join(area, 'outside'), join(bundle, 'macos'));
      }
      return ok();
    }
    throw new Error(`unexpected fixture command ${file} ${args.join(' ')}`);
  };
  const args = { root, tag: 'v0.1.0', target: targets[0], output };
  return { root, area, output, write, git, commit, args, calls, run, control };
}
const tauriCalls = f => f.calls.filter(c => c.args.includes('tauri') && c.args.includes('build'));

for (const target of targets) test(`frozen ${target} returns real paths and receipt after one locked stage`, async t => {
  const f = fixture(t); const receipt = await buildFrozen({ ...f.args, target }, f.run);
  assert.equal(receipt.tag, 'v0.1.0'); assert.equal(receipt.commit, f.commit); assert.equal(receipt.target, target);
  assert.ok(fs.statSync(receipt.app).isDirectory()); assert.ok(fs.statSync(receipt.dmg).isFile());
  assert.deepEqual(JSON.parse(fs.readFileSync(join(f.output, 'build-receipt.json'))), receipt);
  assert.equal(f.calls.filter(c => c.file.endsWith('/cargo') && c.args[0] === 'build').length, 1);
  assert.deepEqual(tauriCalls(f)[0].args.slice(1), ['tauri', 'build', '--target', target, '--bundles', 'app,dmg', '--config', 'src-tauri/tauri.unsigned.conf.json', '--', '--locked']);
  assert.ok(f.calls.filter(c => c.args[0] === 'metadata').every(c => c.args.includes('--locked')));
  assert.deepEqual(Object.keys(receipt.lockfiles).sort(), ['Cargo.lock', 'pnpm-lock.yaml', 'src-tauri/Cargo.lock']);
  assert.ok(receipt.sources.length >= 10); assert.ok(receipt.tools.cargo);
  assert.ok(Array.isArray(receipt.commands), 'receipt records fixed command executions');
  assert.equal(receipt.commands.length, f.calls.length);
  assert.deepEqual(fs.readFileSync(`${f.output}.logs/commands.jsonl`, 'utf8').trim().split('\n').map(JSON.parse), receipt.commands);
  assert.ok(receipt.commands.every(c => Array.isArray(c.args) && c.started && c.finished && c.status === 0 && /^[a-f0-9]{64}$/.test(c.stdoutSha256)));
  assert.ok(fs.existsSync(`${f.output}.logs/stdout.log`)); assert.ok(!fs.existsSync(join(f.output, 'stdout.log')));
  const dirs = new Set(f.calls.filter(c => c.file !== 'git').map(c => c.options.env.CARGO_TARGET_DIR));
  assert.deepEqual([...dirs], [join(f.output, 'target')]);
});
for (const [name, change, pattern] of [
  ['unsupported target', { target: 'universal-apple-darwin' }, /target/],
  ['tag traversal', { tag: '../v0.1.0' }, /tag/],
  ['prerelease tag', { tag: 'v0.1.0-beta' }, /tag/],
  ['newline tag', { tag: 'v0.1.0\n' }, /tag/],
  ['relative escape', { output: '../escape' }, /output/],
]) test(`rejects ${name} before build`, async t => {
  const f = fixture(t); await assert.rejects(buildFrozen({ ...f.args, ...change }, f.run), pattern); assert.equal(tauriCalls(f).length, 0);
});
for (const [name, control, pattern] of [
  ['missing tag', { missingTag: true }, /git/], ['HEAD different', { wrongHead: true }, /HEAD/],
  ['CLI missing', { missingCli: true }, /CLI/], ['CLI not executable', { nonExecutableCli: true }, /CLI/], ['CLI wrong arch', { wrongArch: true }, /architecture/],
  ['stage failure', { stageExit: true }, /exit=7/],
]) test(`rejects ${name} before Tauri`, async t => {
  const f = fixture(t, control); await assert.rejects(buildFrozen(f.args, f.run), pattern); assert.equal(tauriCalls(f).length, 0);
  if (name === 'CLI wrong arch') assert.equal(f.calls.filter(c => c.file === 'lipo').length, 1);
});
for (const path of ['Cargo.lock', 'src-tauri/Cargo.lock', 'package.json', 'src-tauri/tauri.conf.json', 'crates/towermd/Cargo.toml']) test(`post-build byte drift refuses receipt: ${path}`, async t => {
  const f = fixture(t, { drift: path }); await assert.rejects(buildFrozen(f.args, f.run), /source|dirty|version|lock/);
  assert.equal(tauriCalls(f).length, 1); assert.ok(!fs.existsSync(join(f.output, 'build-receipt.json')));
});
for (const control of [{ noApp: true }, { twoApp: true }, { noDmg: true }, { twoDmg: true }, { buildExit: true }]) test(`rejects absent/ambiguous/failed bundles ${JSON.stringify(control)}`, async t => {
  const f = fixture(t, control); await assert.rejects(buildFrozen(f.args, f.run), /bundle|exit=8/); assert.equal(tauriCalls(f).length, 1);
});
test('dirty detached tree and old output are not reused', async t => {
  const f = fixture(t); f.write('untracked', 'other task');
  await assert.rejects(buildFrozen(f.args, f.run), /dirty/); assert.equal(tauriCalls(f).length, 0);
  fs.rmSync(join(f.root, 'untracked')); fs.mkdirSync(f.output); fs.writeFileSync(join(f.output, 'old.dmg'), 'old');
  await assert.rejects(buildFrozen(f.args, f.run), /output/); assert.equal(fs.readFileSync(join(f.output, 'old.dmg'), 'utf8'), 'old');
});
test('version and merged updater are checked before stage', async t => {
  const f = fixture(t); f.write('src-tauri/tauri.macos.conf.json', JSON.stringify({ plugins: { updater: {} } }));
  f.git('add', '.'); f.git('-c', 'commit.gpgsign=false', 'commit', '-m', 'unsafe fixture'); f.git('tag', '-f', 'v0.1.0');
  await assert.rejects(buildFrozen(f.args, f.run), /updater/); assert.equal(tauriCalls(f).length, 0);
  assert.equal(f.calls.filter(c => c.args[0] === 'build').length, 0);
});
test('a committed stale project lock is rejected by the existing frozen checker', async t => {
  const f = fixture(t); f.write('src-tauri/Cargo.lock', fs.readFileSync(join(f.root, 'src-tauri/Cargo.lock'), 'utf8').replace('0.1.0', '0.0.0'));
  f.git('add', '.'); f.git('-c', 'commit.gpgsign=false', 'commit', '-m', 'stale fixture'); f.git('tag', '-f', 'v0.1.0');
  await assert.rejects(buildFrozen(f.args, f.run), /Cargo.lock/);
  assert.equal(f.calls.filter(c => c.args[0] === 'build').length, 0);
});
test('all process environments omit secrets without reading getter traps or secret files', async t => {
  const f = fixture(t), originalEnv = process.env;
  const env = { HOME: homedir(), TMPDIR: tmpdir(), LANG: 'C', PATH: '/usr/bin:/bin', CARGO_TARGET_DIR: '/must-not-inherit' };
  for (const key of ['APPLE_PASSWORD', 'APPLE_SIGNING_IDENTITY', 'TAURI_SIGNING_PRIVATE_KEY', 'TAURI_CONFIG', 'GITHUB_TOKEN']) Object.defineProperty(env, key, { enumerable: true, get() { throw new Error(`secret getter ${key}`); } });
  process.env = env; t.after(() => { process.env = originalEnv; });
  assert.throws(() => env.APPLE_PASSWORD, /secret getter/);
  const denied = path => /(?:\.env\.release|updater.*key|private.*key)/i.test(String(path));
  const read = fs.readFileSync, readAsync = fsp.readFile;
  t.mock.method(fs, 'readFileSync', (path, ...rest) => { assert.ok(!denied(path), 'secret file read'); return read(path, ...rest); });
  t.mock.method(fsp, 'readFile', (path, ...rest) => { assert.ok(!denied(path), 'secret file read'); return readAsync(path, ...rest); });
  syncBuiltinESMExports(); t.after(() => { t.mock.restoreAll(); syncBuiltinESMExports(); });
  assert.throws(() => fs.readFileSync(join(f.root, '.env.release')), /secret file read/);
  const receipt = await buildFrozen(f.args, f.run);
  assert.equal(receipt.commit, f.commit); assert.equal(tauriCalls(f).length, 1); assert.ok(f.calls.length > 10);
  for (const { options } of f.calls) {
    for (const key of ['APPLE_PASSWORD', 'APPLE_SIGNING_IDENTITY', 'TAURI_SIGNING_PRIVATE_KEY', 'TAURI_CONFIG', 'GITHUB_TOKEN']) assert.equal(options.env[key], undefined);
    assert.equal(options.env.PATH.split(':')[0], join(f.root, '.tools/pnpm/node_modules/.bin'));
  }
});
test('failure message redacts subprocess bytes while separate owned logs preserve them', async t => {
  const f = fixture(t, { stageExit: true });
  await assert.rejects(buildFrozen(f.args, f.run), error => /exit=7/.test(error.message) && !error.message.includes('SECRET'));
  assert.match(fs.readFileSync(`${f.output}.logs/stdout.log`, 'utf8'), /SECRET/);
  assert.match(fs.readFileSync(`${f.output}.logs/stderr.log`, 'utf8'), /SECRET/);
});
test('a tag object changed at the final recheck cannot receive a success receipt', async t => {
  const f = fixture(t, { lateTag: true });
  await assert.rejects(buildFrozen(f.args, f.run), /tag identity/);
  assert.equal(tauriCalls(f).length, 1); assert.ok(!fs.existsSync(join(f.output, 'build-receipt.json')));
});
test('bundle directory symlink cannot import an old App outside the owned target', async t => {
  const f = fixture(t, { bundleLink: true });
  await assert.rejects(buildFrozen(f.args, f.run), /bundle/);
  assert.equal(tauriCalls(f).length, 1); assert.ok(!fs.existsSync(join(f.output, 'build-receipt.json')));
});
test('CLI rejects missing, unknown and duplicate arguments', async t => {
  const f = fixture(t);
  for (const args of [[], ['--force'], ['--root'], ['--root', f.root, '--root', f.root]]) await assert.rejects(Promise.resolve().then(() => main(args, f.run)), /argument/);
  assert.equal(f.calls.length, 0);
});
