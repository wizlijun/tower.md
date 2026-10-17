// @vitest-environment node
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, chmodSync, rmSync, statSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { homedir, tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, expect, test } from 'vitest';
import { stageCli } from './stage-cli.mjs';

const roots: string[] = [];
afterEach(() => roots.splice(0).forEach((root) => rmSync(root, { recursive: true, force: true })));
function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'tower-stage-'));
  roots.push(root);
  const cargo = join(root, '.cargo/bin/cargo');
  mkdirSync(join(root, '.cargo/bin'), { recursive: true });
  writeFileSync(cargo, `#!${process.execPath}
const args = process.argv.slice(2);
if (args[0] === 'metadata') {
  require('node:fs').writeFileSync(process.env.RECORD + '.metadata', JSON.stringify(args));
  console.log(JSON.stringify({ target_directory: require('node:path').resolve(process.cwd(), process.env.CARGO_TARGET_DIR || 'target') }));
} else {
  require('node:fs').writeFileSync(process.env.RECORD, JSON.stringify(args));
}
`);
  chmodSync(cargo, 0o755);
  const targetDir = join(root, 'custom target');
  return { root, cargo, env: { ...process.env, RECORD: join(root, 'args.json'), CARGO_TARGET_DIR: targetDir }, targetDir };
}

for (const target of ['aarch64-apple-darwin', 'x86_64-apple-darwin']) {
  test(`stages only the exact ${target} release artifact and preserves execution`, async () => {
    const f = fixture();
    for (const triple of ['aarch64-apple-darwin', 'x86_64-apple-darwin', '']) {
      const output = join(f.targetDir, triple, 'release');
      mkdirSync(output, { recursive: true });
      writeFileSync(join(output, 'towermd'), triple || 'host must not be used');
      chmodSync(join(output, 'towermd'), 0o755);
    }
    const destination = await stageCli(target, f);
    expect(destination).toBe(join(f.root, 'src-tauri/binaries', `towermd-${target}`));
    expect(readFileSync(destination, 'utf8')).toBe(target);
    expect(statSync(destination).mode & 0o111).not.toBe(0);
    expect(JSON.parse(readFileSync(f.env.RECORD, 'utf8'))).toEqual(['build', '-p', 'towermd', '--release', '--target', target]);
    expect(JSON.parse(readFileSync(f.env.RECORD + '.metadata', 'utf8'))).toEqual(['metadata', '--no-deps', '--format-version', '1']);
  });
}

test('missing target artifact rejects instead of falling back to the host artifact', async () => {
  const f = fixture();
  mkdirSync(join(f.targetDir, 'release'), { recursive: true });
  writeFileSync(join(f.targetDir, 'release/towermd'), 'host');
  await expect(stageCli('x86_64-apple-darwin', f)).rejects.toThrow('x86_64-apple-darwin');
});

test.each([undefined, 'aarch64-apple-darwin; echo unsafe', 'universal-apple-darwin'])('requires an explicit supported target: %s', async (target) => {
  await expect(stageCli(target, fixture())).rejects.toThrow('--target');
});

test('script exits nonzero for a missing architecture artifact', () => {
  const f = fixture();
  mkdirSync(join(f.targetDir, 'release'), { recursive: true });
  writeFileSync(join(f.targetDir, 'release/towermd'), 'wrong host artifact');
  const result = spawnSync(process.execPath, [fileURLToPath(new URL('./stage-cli.mjs', import.meta.url)), '--target', 'x86_64-apple-darwin'], {
    env: { ...f.env, HOME: f.root }, encoding: 'utf8',
  });
  expect(result.status).toBe(1);
  expect(result.stderr).toContain('缺少 x86_64-apple-darwin CLI 构建产物');
});

test('default target directory and relative CARGO_TARGET_DIR resolve against the project root', async () => {
  for (const targetDir of [undefined, 'relative target']) {
    const f = fixture();
    const output = join(f.root, targetDir ?? 'target', 'aarch64-apple-darwin/release');
    mkdirSync(output, { recursive: true });
    writeFileSync(join(output, 'towermd'), 'exact target');
    chmodSync(join(output, 'towermd'), 0o755);
    const env = { ...f.env };
    if (targetDir === undefined) delete env.CARGO_TARGET_DIR;
    else env.CARGO_TARGET_DIR = targetDir;
    expect(readFileSync(await stageCli('aarch64-apple-darwin', { ...f, env }), 'utf8')).toBe('exact target');
  }
});

test('a failed cargo build rejects even when an old architecture artifact exists', async () => {
  const f = fixture();
  writeFileSync(f.cargo, `#!${process.execPath}\nprocess.exit(7);\n`);
  const output = join(f.targetDir, 'aarch64-apple-darwin/release');
  mkdirSync(output, { recursive: true });
  writeFileSync(join(output, 'towermd'), 'stale');
  chmodSync(join(output, 'towermd'), 0o755);
  await expect(stageCli('aarch64-apple-darwin', f)).rejects.toThrow('exit=7');
});

test('real Cargo configured target-dir wins over a stale default-target CLI', async () => {
  const root = mkdtempSync(join(tmpdir(), 'tower-stage-configured-'));
  roots.push(root);
  const target = process.arch === 'arm64' ? 'aarch64-apple-darwin' : 'x86_64-apple-darwin';
  mkdirSync(join(root, '.cargo'));
  mkdirSync(join(root, 'src'));
  writeFileSync(join(root, 'Cargo.toml'), '[package]\nname="towermd"\nversion="0.0.0"\nedition="2021"\n');
  writeFileSync(join(root, 'src/main.rs'), 'fn main() {}\n');
  writeFileSync(join(root, '.cargo/config.toml'), '[build]\ntarget-dir="configured-target"\n');
  const stale = join(root, 'target', target, 'release/towermd');
  mkdirSync(join(root, 'target', target, 'release'), { recursive: true });
  writeFileSync(stale, 'stale CLI that must never be staged', { mode: 0o755 });
  const env = { ...process.env, CARGO_NET_OFFLINE: 'true' };
  delete env.CARGO_TARGET_DIR;
  const destination = await stageCli(target, { root, cargo: join(homedir(), '.cargo/bin/cargo'), env });
  const sha = (path: string) => createHash('sha256').update(readFileSync(path)).digest('hex');
  expect(sha(destination)).toBe(sha(join(root, 'configured-target', target, 'release/towermd')));
  expect(readFileSync(stale, 'utf8')).toBe('stale CLI that must never be staged');
}, 15_000);
