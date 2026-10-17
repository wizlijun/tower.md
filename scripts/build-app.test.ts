// @vitest-environment node
import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { delimiter, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, expect, test } from 'vitest';
import { buildApp } from './build-app.mjs';

const projectRoot = fileURLToPath(new URL('../', import.meta.url));
const targets = ['aarch64-apple-darwin', 'x86_64-apple-darwin'];
const roots: string[] = [];
afterEach(() => roots.splice(0).forEach(root => rmSync(root, { recursive: true, force: true })));

function fixture(control = {}) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'tower 构建 ')));
  roots.push(root);
  for (const directory of ['scripts/release', 'src-tauri', '.cargo/bin', '.tools/pnpm/node_modules/pnpm/bin']) {
    mkdirSync(join(root, directory), { recursive: true });
  }
  for (const path of ['scripts/project-pnpm.mjs', 'scripts/stage-cli.mjs', 'scripts/release/core.mjs', 'scripts/build-app.mjs', 'src-tauri/tauri.conf.json']) {
    copyFileSync(join(projectRoot, path), join(root, path));
  }
  const { packageManager } = JSON.parse(readFileSync(join(projectRoot, 'package.json'), 'utf8'));
  writeFileSync(join(root, 'package.json'), JSON.stringify({ type: 'module', packageManager }));
  writeFileSync(join(root, 'control.json'), JSON.stringify(control));
  writeFileSync(join(root, 'src-tauri/tauri.unsigned.conf.json'), JSON.stringify({ bundle: { active: true, targets: ['app', 'dmg'],
    createUpdaterArtifacts: false, externalBin: ['binaries/towermd'], macOS: { signingIdentity: '-', providerShortName: null } } }));
  const helpers = `const fs = require('node:fs'), path = require('node:path');
const root = ${JSON.stringify(root)};
const control = JSON.parse(fs.readFileSync(path.join(root, 'control.json'), 'utf8'));
const args = process.argv.slice(2);
function record(phase) {
  const env = Object.fromEntries(['PATH','CARGO_TARGET_DIR','APPLE_PASSWORD','APPLE_SIGNING_IDENTITY','TAURI_SIGNING_PRIVATE_KEY','TAURI_CONFIG','UNRELATED_SENTINEL'].map(key => [key, process.env[key] ?? null]));
  fs.appendFileSync(path.join(root, 'calls.jsonl'), JSON.stringify({phase,args,cwd:process.cwd(),env})+'\\n');
}
`;
  const cargo = join(root, '.cargo/bin/cargo');
  writeFileSync(cargo, `#!${process.execPath}
${helpers.replace("require('node:fs')", "(await import('node:fs'))").replace("require('node:path')", "(await import('node:path'))")}
const targetDirectory = path.resolve(root, process.env.CARGO_TARGET_DIR || 'core target');
if (args[0] === 'metadata') {
  record('metadata');
  console.log(JSON.stringify({target_directory:targetDirectory}));
} else {
  record('stage');
  if (control.stageExit) process.exit(control.stageExit);
  if (!control.missingCli) {
    const target = args[args.indexOf('--target')+1];
    const output = path.join(targetDirectory, target, 'release/towermd');
    fs.mkdirSync(path.dirname(output), {recursive:true});
    fs.writeFileSync(output, target, {mode:0o755});
  }
}
`);
  chmodSync(cargo, 0o755);
  mkdirSync(join(root, '.tools/pnpm/node_modules/.bin'), { recursive: true });
  writeFileSync(join(root, '.tools/pnpm/node_modules/.bin/lipo'), `#!${process.execPath}
${helpers.replace("require('node:fs')", "(await import('node:fs'))").replace("require('node:path')", "(await import('node:path'))")}
record('architecture');
console.log(control.wrongArch ? 'arm64 x86_64' : args.at(-1).includes('aarch64') ? 'arm64' : 'x86_64');
`, { mode: 0o755 });
  writeFileSync(join(root, '.tools/pnpm/node_modules/pnpm/bin/pnpm.cjs'), `${helpers}
if (args[0] === 'build') {
  record('frontend');
  if (control.frontendExit) process.exit(control.frontendExit);
  if (!control.missingDist) {
    const index = path.join(root, 'dist/index.html');
    fs.mkdirSync(path.dirname(index), {recursive:true});
    if (control.indexDirectory) fs.mkdirSync(index);
    else fs.writeFileSync(index, '<html>fixture</html>');
  }
  if (control.removeStagedCli) fs.rmSync(path.join(root, 'src-tauri/binaries'), {recursive:true});
} else if (args[0] === 'tauri') {
  record('tauri');
  if (control.tauriExit) process.exit(control.tauriExit);
} else process.exit(99);
`);
  const env = { HOME: root, TMPDIR: root, LANG: 'C', PATH: '/usr/bin:/bin',
    APPLE_PASSWORD: 'fixture-sentinel', APPLE_SIGNING_IDENTITY: 'fixture-sentinel',
    TAURI_SIGNING_PRIVATE_KEY: 'fixture-sentinel', TAURI_CONFIG: 'fixture-sentinel',
    UNRELATED_SENTINEL: 'fixture-sentinel' };
  const calls = () => {
    try { return readFileSync(join(root, 'calls.jsonl'), 'utf8').trim().split('\n').map(line => JSON.parse(line)); }
    catch (error) { if ((error as NodeJS.ErrnoException).code === 'ENOENT') return []; throw error; }
  };
  return { root, cargo, env, calls };
}

for (const target of targets) {
  test(`stages ${target}, builds frontend, then invokes only its App bundle`, async () => {
    const f = fixture();
    await buildApp(target, f);
    const calls = f.calls();
    expect(calls.map(call => call.phase)).toEqual(['stage', 'metadata', 'frontend', 'tauri']);
    expect(calls.map(call => call.args)).toEqual([
      ['build', '-p', 'towermd', '--release', '--target', target],
      ['metadata', '--no-deps', '--format-version', '1'],
      ['build'], ['tauri', 'build', '--target', target, '--bundles', 'app'],
    ]);
    expect(calls.every(call => call.cwd === f.root)).toBe(true);
    expect(readFileSync(join(f.root, 'src-tauri/binaries', `towermd-${target}`), 'utf8')).toBe(target);
    for (const call of calls) {
      expect(call.env).toMatchObject({ APPLE_PASSWORD: null, APPLE_SIGNING_IDENTITY: null,
        TAURI_SIGNING_PRIVATE_KEY: null, TAURI_CONFIG: null, UNRELATED_SENTINEL: null });
      expect(call.env.PATH.split(delimiter)[0]).toBe(join(f.root, '.tools/pnpm/node_modules/.bin'));
    }
  });
}

test.each([undefined, 'universal-apple-darwin', 'aarch64-apple-darwin; echo unsafe'])('rejects unsupported target before subprocesses: %s', async target => {
  const f = fixture();
  await expect(buildApp(target, f)).rejects.toThrow('--target');
  expect(f.calls()).toEqual([]);
});

test.each([
  [{ stageExit: 7 }, ['stage'], 'CLI 构建失败'],
  [{ missingCli: true }, ['stage', 'metadata'], '缺少'],
  [{ frontendExit: 8 }, ['stage', 'metadata', 'frontend'], 'exit=8'],
  [{ missingDist: true }, ['stage', 'metadata', 'frontend'], 'dist/index.html'],
  [{ indexDirectory: true }, ['stage', 'metadata', 'frontend'], 'dist/index.html'],
  [{ removeStagedCli: true }, ['stage', 'metadata', 'frontend'], 'CLI'],
  [{ tauriExit: 9 }, ['stage', 'metadata', 'frontend', 'tauri'], 'exit=9'],
])('stops the build chain for %j', async (control, phases, message) => {
  const f = fixture(control);
  await expect(buildApp(targets[0], f)).rejects.toThrow(message);
  expect(f.calls().map(call => call.phase)).toEqual(phases);
});

test.each(['relative target', '/absolute placeholder'])('normalizes CARGO_TARGET_DIR for every subprocess: %s', async path => {
  const f = fixture();
  const supplied = path.startsWith('/') ? join(f.root, 'absolute target') : path;
  await buildApp(targets[0], { ...f, env: { ...f.env, CARGO_TARGET_DIR: supplied } });
  expect(f.calls()).toHaveLength(4);
  expect(f.calls().every(call => call.env.CARGO_TARGET_DIR === resolve(f.root, supplied))).toBe(true);
  expect(readFileSync(join(resolve(f.root, supplied), targets[0], 'release/towermd'), 'utf8')).toBe(targets[0]);
});

test.each([
  { bundle: { createUpdaterArtifacts: true } },
  { bundle: { macOS: { signingIdentity: 'Developer ID' } } },
  { bundle: { macOS: { providerShortName: 'provider' } } },
  { bundle: { externalBin: ['binaries/other'] } },
  { plugins: { updater: {} } },
])('rejects unsafe configuration before subprocesses: %j', async change => {
  const f = fixture();
  const configPath = join(f.root, 'src-tauri/tauri.conf.json');
  const config = JSON.parse(readFileSync(configPath, 'utf8'));
  const modified = { ...config, ...change, bundle: { ...config.bundle, ...change.bundle,
    macOS: { ...config.bundle.macOS, ...change.bundle?.macOS } } };
  writeFileSync(configPath, JSON.stringify(modified));
  await expect(buildApp(targets[0], f)).rejects.toThrow();
  expect(f.calls()).toEqual([]);
});

test('base config explicitly disables updater, uses ad-hoc and keeps the real frontend hook', () => {
  const config = JSON.parse(readFileSync(join(projectRoot, 'src-tauri/tauri.conf.json'), 'utf8'));
  expect(config.bundle.createUpdaterArtifacts).toBe(false);
  expect(config.bundle.macOS.signingIdentity).toBe('-');
  expect(config.bundle.externalBin).toEqual(['binaries/towermd']);
  expect(config.build.beforeBuildCommand).toBe('node scripts/project-pnpm.mjs build');
  expect(config.build.frontendDist).toBe('../dist');
});

test.each([[], ['--target'], ['--target', targets[0], '--locked', 'true'], ['--target', targets[0], '--target', targets[1]], ['--root', '/tmp'], ['--target', 'universal-apple-darwin']].map(args => [args]))('CLI rejects non-contract arguments: %j', args => {
  const f = fixture();
  const result = spawnSync(process.execPath, [join(f.root, 'scripts/build-app.mjs'), ...args], { cwd: tmpdir(), env: f.env, encoding: 'utf8' });
  expect(result.status).toBe(1);
  expect(result.stderr).toContain('--target');
  expect(f.calls()).toEqual([]);
});

test('CLI resolves its own project rather than the caller cwd', () => {
  const f = fixture();
  const result = spawnSync(process.execPath, [join(f.root, 'scripts/build-app.mjs'), '--target', targets[1]], { cwd: tmpdir(), env: f.env, encoding: 'utf8' });
  expect(result.status, result.stderr).toBe(0);
  expect(f.calls().map(call => call.phase)).toEqual(['stage', 'metadata', 'frontend', 'tauri']);
  expect(f.calls().every(call => call.cwd === f.root)).toBe(true);
});

for (const target of targets) test(`release flags keep one locked stage and check ${target} before Tauri`, async () => {
  const f = fixture();
  await buildApp(target, { ...f, locked: true, config: 'src-tauri/tauri.unsigned.conf.json', bundles: 'app,dmg' });
  const calls = f.calls();
  expect(calls.map(call => call.phase)).toEqual(['stage', 'metadata', 'architecture', 'frontend', 'tauri']);
  expect(calls[0].args).toEqual(['build', '--locked', '-p', 'towermd', '--release', '--target', target]);
  expect(calls[1].args).toEqual(['metadata', '--locked', '--no-deps', '--format-version', '1']);
  expect(calls.at(-1).args).toEqual(['tauri', 'build', '--target', target, '--bundles', 'app,dmg', '--config', 'src-tauri/tauri.unsigned.conf.json', '--', '--locked']);
});

test('release wrong CLI architecture stops before frontend and Tauri', async () => {
  const f = fixture({ wrongArch: true });
  await expect(buildApp(targets[0], { ...f, locked: true, config: 'src-tauri/tauri.unsigned.conf.json', bundles: 'app,dmg' })).rejects.toThrow('architecture');
  expect(f.calls().map(call => call.phase)).toEqual(['stage', 'metadata', 'architecture']);
});

test('release configuration merges base, macOS and overlay including null deletion', async () => {
  const f = fixture();
  const path = join(f.root, 'src-tauri/tauri.conf.json');
  const config = JSON.parse(readFileSync(path, 'utf8'));
  config.bundle.macOS.signingIdentity = 'Developer ID base';
  config.bundle.macOS.providerShortName = 'base provider';
  writeFileSync(path, JSON.stringify(config));
  writeFileSync(join(f.root, 'src-tauri/tauri.macos.conf.json'), JSON.stringify({ bundle: { macOS: { signingIdentity: 'Developer ID platform' } } }));
  await buildApp(targets[0], { ...f, locked: true, config: 'src-tauri/tauri.unsigned.conf.json', bundles: 'app,dmg' });
  expect(f.calls().at(-1).phase).toBe('tauri');
});

test('updater plugin surviving merged release config is rejected before stage', async () => {
  const f = fixture();
  writeFileSync(join(f.root, 'src-tauri/tauri.macos.conf.json'), JSON.stringify({ plugins: { updater: {} } }));
  await expect(buildApp(targets[0], { ...f, locked: true, config: 'src-tauri/tauri.unsigned.conf.json', bundles: 'app,dmg' })).rejects.toThrow('updater');
  expect(f.calls()).toEqual([]);
});

test('configuration symlinks are rejected before reading an external file', async () => {
  const f = fixture();
  const external = join(f.root, 'outside-config.json');
  writeFileSync(external, readFileSync(join(f.root, 'src-tauri/tauri.unsigned.conf.json')));
  rmSync(join(f.root, 'src-tauri/tauri.unsigned.conf.json'));
  symlinkSync(external, join(f.root, 'src-tauri/tauri.unsigned.conf.json'));
  await expect(buildApp(targets[0], { ...f, locked: true, config: 'src-tauri/tauri.unsigned.conf.json', bundles: 'app,dmg' })).rejects.toThrow('config');
  expect(f.calls()).toEqual([]);
});

test.each([
  { locked: 'yes' }, { config: '../outside.json' }, { config: 'src-tauri/other.conf.json' },
  { bundles: 'dmg' }, { bundles: 'app,dmg,updater' },
])('rejects unsupported build option before subprocesses: %j', async change => {
  const f = fixture();
  await expect(buildApp(targets[0], { ...f, ...change })).rejects.toThrow();
  expect(f.calls()).toEqual([]);
});

test('merged overlay publisher identity is rejected before subprocesses', async () => {
  const f = fixture();
  writeFileSync(join(f.root, 'src-tauri/tauri.unsigned.conf.json'), JSON.stringify({ bundle: { macOS: { signingIdentity: 'Developer ID fixture' } } }));
  await expect(buildApp(targets[0], { ...f, locked: true, config: 'src-tauri/tauri.unsigned.conf.json', bundles: 'app,dmg' })).rejects.toThrow('ad-hoc');
  expect(f.calls()).toEqual([]);
});

test.each(['tauri.macos.conf.json5', 'Tauri.macos.toml'])('rejects unsupported effective platform configuration before subprocesses: %s', async name => {
  const f = fixture();
  writeFileSync(join(f.root, 'src-tauri', name), name.endsWith('toml') ? '[plugins.updater]\n' : '{ plugins: { updater: {} } }');
  await expect(buildApp(targets[0], { ...f, locked: true, config: 'src-tauri/tauri.unsigned.conf.json', bundles: 'app,dmg' })).rejects.toThrow('unsupported config');
  expect(f.calls()).toEqual([]);
});

test('CLI accepts explicit release flags and forwards one locked build', () => {
  const f = fixture();
  const result = spawnSync(process.execPath, [join(f.root, 'scripts/build-app.mjs'), '--target', targets[0],
    '--config', 'src-tauri/tauri.unsigned.conf.json', '--bundles', 'app,dmg', '--locked'], { cwd: tmpdir(), env: f.env, encoding: 'utf8' });
  expect(result.status, result.stderr).toBe(0);
  expect(f.calls().map(call => call.phase)).toEqual(['stage', 'metadata', 'architecture', 'frontend', 'tauri']);
  expect(f.calls().at(-1).args.slice(-2)).toEqual(['--', '--locked']);
});
