import { execFile, spawn } from 'node:child_process';
import { promisify } from 'node:util';
import { copyFile, mkdir, lstat, chmod } from 'node:fs/promises';
import { homedir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const projectRoot = fileURLToPath(new URL('../', import.meta.url));
const targets = ['aarch64-apple-darwin', 'x86_64-apple-darwin'];

export async function stageCli(target, {
  root = projectRoot,
  cargo = join(homedir(), '.cargo/bin/cargo'),
  env = process.env,
  locked = false,
  run,
} = {}) {
  if (!targets.includes(target)) {
    throw new Error(`必须显式指定 --target ${targets.join(' 或 ')}`);
  }
  if (typeof locked !== 'boolean') throw new Error('invalid --locked');
  const args = ['build', ...(locked ? ['--locked'] : []), '-p', 'towermd', '--release', '--target', target];
  if (run) {
    const result = await run(cargo, args, { cwd: root, env, shell: false });
    if (result?.error || result?.status !== 0) throw new Error(`CLI 构建失败：exit=${result?.status ?? 'unknown'}`);
  } else await new Promise((resolveBuild, reject) => {
    const child = spawn(cargo, args, {
      cwd: root, env, shell: false, stdio: ['ignore', 'inherit', 'inherit'],
    });
    child.once('error', reject);
    child.once('exit', (code, signal) => {
      if (code === 0 && !signal) resolveBuild();
      else reject(new Error(`CLI 构建失败：exit=${code}, signal=${signal}`));
    });
  });
  const metadataArgs = ['metadata', ...(locked ? ['--locked'] : []), '--no-deps', '--format-version', '1'];
  const result = run ? await run(cargo, metadataArgs, { cwd: root, env, shell: false })
    : await promisify(execFile)(cargo, metadataArgs, { cwd: root, env, shell: false });
  if (run && (result?.error || result?.status !== 0)) throw new Error(`CLI metadata 失败：exit=${result?.status ?? 'unknown'}`);
  const { stdout } = result;
  const { target_directory } = JSON.parse(stdout);
  const source = join(target_directory, target, 'release/towermd');
  const metadata = await lstat(source).catch(() => {
    throw new Error(`缺少 ${target} CLI 构建产物：${source}`);
  });
  if (!metadata.isFile() || !(metadata.mode & 0o111)) {
    throw new Error(`${target} CLI 产物不是普通可执行文件：${source}`);
  }
  const destination = join(root, 'src-tauri/binaries', `towermd-${target}`);
  await mkdir(dirname(destination), { recursive: true });
  await copyFile(source, destination);
  await chmod(destination, metadata.mode & 0o777);
  return destination;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const args = process.argv.slice(2);
    const locked = args.length === 3 && args[2] === '--locked';
    if (args[0] !== '--target' || (!locked && args.length !== 2)) {
      throw new Error('用法：node scripts/stage-cli.mjs --target <triple> [--locked]');
    }
    console.log(await stageCli(args[1], { locked }));
  } catch (error) {
    console.error(`CLI 暂存失败：${error.message}`);
    process.exitCode = 1;
  }
}
