import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { appendFileSync, existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, readlinkSync, realpathSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildApp, loadUnsignedConfig } from '../build-app.mjs';
import { unsignedEnv } from './core.mjs';
import { checkVersion } from './prepare.mjs';

const targets = ['aarch64-apple-darwin', 'x86_64-apple-darwin'];
const config = 'src-tauri/tauri.unsigned.conf.json';
const digest = data => createHash('sha256').update(data).digest('hex');
const inside = (root, path) => { const rel = relative(root, path); return rel === '' || (!isAbsolute(rel) && rel !== '..' && !rel.startsWith(`..${sep}`)); };
const runCommand = (file, args, options) => spawnSync(file, args, { ...options, encoding: 'utf8', maxBuffer: 128 * 1024 * 1024 });

function sources(root, command) {
  const entries = command('git', ['ls-tree', '-r', '-z', '--full-tree', 'HEAD']).split('\0').filter(Boolean);
  if (!entries.length) throw new Error('empty source tree');
  return entries.map(entry => {
    const match = /^(100644|100755|120000) blob ([a-f0-9]{40})\t([\s\S]+)$/.exec(entry);
    if (!match) throw new Error('unsupported source entry');
    const [, mode, blob, path] = match;
    if (!inside(root, resolve(root, path)) || /(?:^|\/)\.env(?:\.|$)|private[-_]?key|updater[-_]?key/i.test(path)) throw new Error('forbidden source path');
    let bytes;
    try {
      const file = join(root, path), stat = lstatSync(file);
      if (mode === '120000' ? !stat.isSymbolicLink() : !stat.isFile()) throw new Error();
      bytes = mode === '120000' ? readlinkSync(file, { encoding: 'buffer' }) : readFileSync(file);
    } catch { throw new Error(`source unreadable: ${path}`); }
    const actualBlob = createHash('sha1').update(`blob ${bytes.length}\0`).update(bytes).digest('hex');
    if (actualBlob !== blob) throw new Error(`source differs from tag: ${path}`);
    return { path, mode, blob, sha256: digest(bytes) };
  });
}

function uniqueBundle(directory, suffix, isDirectory) {
  let names;
  try {
    if (realpathSync(directory) !== directory) throw new Error('bundle directory escapes owned target');
    names = readdirSync(directory).filter(name => name.endsWith(suffix));
  }
  catch (error) { if (error.code !== 'ENOENT') throw error; names = []; }
  if (names.length !== 1) throw new Error(`bundle count mismatch: ${suffix}`);
  const path = join(directory, names[0]), stat = lstatSync(path);
  if (isDirectory ? !stat.isDirectory() : !stat.isFile()) throw new Error(`invalid bundle: ${suffix}`);
  return path;
}

export async function buildFrozen({ root, tag, target, output }, run = runCommand) {
  if (!targets.includes(target)) throw new Error('invalid target');
  if (typeof tag !== 'string' || tag.trim() !== tag || !/^v\d+\.\d+\.\d+$/.test(tag)) throw new Error('invalid tag');
  if (typeof output !== 'string' || !isAbsolute(output)) throw new Error('absolute output required');
  root = realpathSync(root); output = resolve(output);
  if (inside(root, output) || inside(output, root) || realpathSync(dirname(output)) !== dirname(output) ||
      existsSync(output) || existsSync(`${output}.logs`)) throw new Error('output must be a new directory outside source');
  const logs = `${output}.logs`, env = unsignedEnv(process.env, join(root, '.tools/pnpm/node_modules/.bin'));
  env.CARGO_TARGET_DIR = join(output, 'target');
  mkdirSync(logs, { mode: 0o700 });
  const commands = [];
  const loggedRun = (file, args) => {
    let result, threw = false;
    const started = new Date().toISOString();
    try { result = run(file, args, { cwd: root, env, shell: false }); }
    catch { threw = true; }
    const stdout = result?.stdout ?? '', stderr = result?.stderr ?? '';
    appendFileSync(join(logs, 'stdout.log'), stdout, { mode: 0o600 });
    appendFileSync(join(logs, 'stderr.log'), stderr, { mode: 0o600 });
    const record = { file, args: [...args], started, finished: new Date().toISOString(),
      status: result?.status ?? null, failed: threw || !!result?.error,
      stdoutSha256: digest(stdout), stderrSha256: digest(stderr) };
    commands.push(record);
    appendFileSync(join(logs, 'commands.jsonl'), JSON.stringify(record) + '\n', { mode: 0o600 });
    if (threw) throw new Error(`${basename(file)} ${args[0]} failed`);
    return result;
  };
  const command = (file, args) => {
    const result = loggedRun(file, args);
    if (result?.error || result?.status !== 0) throw new Error(`${basename(file)} ${args[0]} exit=${result?.status ?? 'unknown'}`);
    return String(result.stdout ?? '');
  };
  const git = args => command('git', args).trim();
  const identity = () => {
    const commit = git(['rev-parse', '--verify', `refs/tags/${tag}^{commit}`]);
    if (git(['rev-parse', '--verify', 'HEAD']) !== commit) throw new Error('HEAD differs from frozen tag');
    if (command('git', ['status', '--porcelain', '-z', '--untracked-files=all']) !== '') throw new Error('dirty frozen tree');
    return { commit, tree: git(['rev-parse', '--verify', 'HEAD^{tree}']), tagObject: git(['rev-parse', '--verify', `refs/tags/${tag}`]) };
  };
  const initial = identity(), before = sources(root, command);
  checkVersion(root, tag.slice(1), 'frozen', loggedRun);
  await loadUnsignedConfig(root, config);
  if (JSON.stringify(sources(root, command)) !== JSON.stringify(before)) throw new Error('source changed during checks');
  if (!existsSync(join(root, '.tools/pnpm/node_modules/pnpm/bin/pnpm.cjs'))) throw new Error('missing prepared project pnpm');
  mkdirSync(output, { mode: 0o700 });
  const cargo = join(homedir(), '.cargo/bin/cargo');
  const tools = {};
  for (const [name, file, args] of [
    ['node', process.execPath, ['--version']], ['pnpm', process.execPath, ['scripts/project-pnpm.mjs', '--version']],
    ['rustc', join(homedir(), '.cargo/bin/rustc'), ['--version']], ['cargo', cargo, ['--version']],
    ['tauri', process.execPath, ['scripts/project-pnpm.mjs', 'tauri', '--version']],
    ['sdk', 'xcrun', ['--show-sdk-path']], ['xcode', 'xcodebuild', ['-version']],
  ]) tools[name] = command(file, args).trim();
  const lockfiles = Object.fromEntries(['Cargo.lock', 'src-tauri/Cargo.lock', 'pnpm-lock.yaml']
    .map(path => [path, before.find(source => source.path === path)?.sha256]));
  if (Object.values(lockfiles).some(value => !value)) throw new Error('missing frozen lockfile');
  await buildApp(target, { root, cargo, env, locked: true, config, bundles: 'app,dmg', run: loggedRun });
  const final = identity();
  if (JSON.stringify(final) !== JSON.stringify(initial)) throw new Error('frozen tag identity changed');
  checkVersion(root, tag.slice(1), 'frozen', loggedRun);
  if (JSON.stringify(sources(root, command)) !== JSON.stringify(before)) throw new Error('source changed during build');
  if (JSON.stringify(identity()) !== JSON.stringify(initial)) throw new Error('frozen tag identity changed');
  const bundle = join(env.CARGO_TARGET_DIR, target, 'release/bundle');
  const app = uniqueBundle(join(bundle, 'macos'), '.app', true), dmg = uniqueBundle(join(bundle, 'dmg'), '.dmg', false);
  const receipt = { tag, ...initial, target, app, dmg, tools, lockfiles, sources: before, commands };
  writeFileSync(join(output, 'build-receipt.json'), JSON.stringify(receipt, null, 2) + '\n', { flag: 'wx', mode: 0o600 });
  return receipt;
}

export async function main(args, run) {
  const values = {};
  for (let i = 0; i < args.length; i += 2) {
    const key = args[i], value = args[i + 1];
    if (!['--root', '--tag', '--target', '--output'].includes(key) || values[key] !== undefined || !value || value.startsWith('--')) throw new Error('invalid argument');
    values[key] = value;
  }
  if (Object.keys(values).length !== 4) throw new Error('missing argument');
  return buildFrozen({ root: values['--root'], tag: values['--tag'], target: values['--target'], output: values['--output'] }, run);
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { await main(process.argv.slice(2)); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
