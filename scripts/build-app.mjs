import { execFile, spawn } from 'node:child_process';
import { readFile, stat, lstat } from 'node:fs/promises';
import { promisify } from 'node:util';
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { stageCli } from './stage-cli.mjs';
import { assertUnsigned, unsignedEnv } from './release/core.mjs';

const projectRoot = fileURLToPath(new URL('../', import.meta.url));
const targets = ['aarch64-apple-darwin', 'x86_64-apple-darwin'];
const unsignedConfig = 'src-tauri/tauri.unsigned.conf.json';

// Tauri CLI 2.11.1 helpers/config.rs: base -> platform -> CLI JSON Merge Patch.
function mergePatch(document, patch) {
  if (!patch || typeof patch !== 'object' || Array.isArray(patch)) return patch;
  const out = document && typeof document === 'object' && !Array.isArray(document) ? { ...document } : {};
  for (const [key, value] of Object.entries(patch)) {
    if (value === null) delete out[key];
    else Object.defineProperty(out, key, { value: mergePatch(out[key], value), enumerable: true, configurable: true, writable: true });
  }
  return out;
}

export async function loadUnsignedConfig(root, config) {
  if (config !== undefined && config !== unsignedConfig) throw new Error('invalid --config');
  const read = async path => {
    try {
      if (!(await lstat(join(root, path))).isFile()) throw new Error();
      return JSON.parse(await readFile(join(root, path), 'utf8'));
    }
    catch { throw new Error(`invalid config: ${path}`); }
  };
  let effective = await read('src-tauri/tauri.conf.json');
  const platform = 'src-tauri/tauri.macos.conf.json';
  try { await lstat(join(root, platform)); effective = mergePatch(effective, await read(platform)); }
  catch (error) {
    if (error.code !== 'ENOENT') throw error;
    for (const other of ['src-tauri/tauri.macos.conf.json5', 'src-tauri/Tauri.macos.toml']) {
      try { await lstat(join(root, other)); }
      catch (missing) { if (missing.code === 'ENOENT') continue; throw missing; }
      throw new Error(`unsupported config: ${other}`);
    }
  }
  if (config) effective = mergePatch(effective, await read(config));
  assertUnsigned(effective);
  return effective;
}

export async function buildApp(target, {
  root = projectRoot,
  cargo = join(homedir(), '.cargo/bin/cargo'),
  env = process.env,
  locked = false,
  config,
  bundles = 'app',
  run,
} = {}) {
  if (!targets.includes(target)) {
    throw new Error(`必须显式指定 --target ${targets.join(' 或 ')}`);
  }
  if (typeof locked !== 'boolean' || !['app', 'app,dmg'].includes(bundles)) throw new Error('invalid --locked/--bundles');
  await loadUnsignedConfig(root, config);
  const buildEnv = unsignedEnv(env, join(root, '.tools/pnpm/node_modules/.bin'));
  if (env.CARGO_TARGET_DIR !== undefined) {
    buildEnv.CARGO_TARGET_DIR = resolve(root, env.CARGO_TARGET_DIR);
  }
  const runPnpm = args => run ? Promise.resolve(run(process.execPath, [join(root, 'scripts/project-pnpm.mjs'), ...args], {
    cwd: root, env: buildEnv, shell: false,
  })).then(result => {
    if (result?.error || result?.status !== 0) throw new Error(`App 构建失败：exit=${result?.status ?? 'unknown'}`);
  }) : new Promise((resolveBuild, reject) => {
    const child = spawn(process.execPath, [join(root, 'scripts/project-pnpm.mjs'), ...args], {
      cwd: root, env: buildEnv, shell: false, stdio: ['ignore', 'inherit', 'inherit'],
    });
    child.once('error', reject);
    child.once('exit', (code, signal) => {
      if (code === 0 && !signal) resolveBuild();
      else reject(new Error(`App 构建失败：exit=${code}, signal=${signal}`));
    });
  });
  const cli = await stageCli(target, { root, cargo, env: buildEnv, locked, run });
  if (locked || config || bundles !== 'app') {
    const args = ['-archs', cli];
    const result = run ? await run('lipo', args, { cwd: root, env: buildEnv, shell: false })
      : await promisify(execFile)('lipo', args, { cwd: root, env: buildEnv, shell: false });
    if ((run && (result?.error || result?.status !== 0)) ||
        result.stdout.trim() !== (target === targets[0] ? 'arm64' : 'x86_64')) throw new Error('CLI architecture mismatch');
  }
  await runPnpm(['build']);
  const cliMetadata = await stat(cli).catch(() => {
    throw new Error(`缺少 ${target} 随包 CLI：${cli}`);
  });
  if (!cliMetadata.isFile() || !(cliMetadata.mode & 0o111)) {
    throw new Error(`${target} 随包 CLI 不是普通可执行文件：${cli}`);
  }
  const index = join(root, 'dist/index.html');
  const frontend = await stat(index).catch(() => {
    throw new Error(`缺少前端 dist/index.html 构建产物：${index}`);
  });
  if (!frontend.isFile()) throw new Error(`前端 dist/index.html 不是普通文件：${index}`);
  await runPnpm(['tauri', 'build', '--target', target, '--bundles', bundles,
    ...(config ? ['--config', config] : []), ...(locked ? ['--', '--locked'] : [])]);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const args = process.argv.slice(2);
    const options = {};
    for (let i = 0; i < args.length; i++) {
      const key = args[i];
      if (!['--target', '--config', '--bundles', '--locked'].includes(key) || options[key] !== undefined) throw new Error('invalid --target/config/bundles/locked argument');
      const value = key === '--locked' ? true : args[++i];
      if (!value || (typeof value === 'string' && value.startsWith('--'))) throw new Error('missing --target/config/bundles argument');
      options[key] = value;
    }
    await buildApp(options['--target'], { locked: options['--locked'], config: options['--config'], bundles: options['--bundles'] });
  } catch (error) {
    console.error(`App 构建失败：${error.message}`);
    process.exitCode = 1;
  }
}
