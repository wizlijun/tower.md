// @vitest-environment node
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, chmodSync, rmSync, statSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';
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
require('node:fs').writeFileSync(process.env.RECORD, JSON.stringify(process.argv.slice(2)));
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
