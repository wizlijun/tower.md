import { spawn } from 'node:child_process';
import { copyFile, mkdir, stat, chmod } from 'node:fs/promises';
import { homedir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const projectRoot = fileURLToPath(new URL('../', import.meta.url));
const targets = ['aarch64-apple-darwin', 'x86_64-apple-darwin'];

export async function stageCli(target, {
  root = projectRoot,
  cargo = join(homedir(), '.cargo/bin/cargo'),
  env = process.env,
} = {}) {
  if (!targets.includes(target)) {
    throw new Error(`必须显式指定 --target ${targets.join(' 或 ')}`);
  }
  await new Promise((resolveBuild, reject) => {
    const child = spawn(cargo, ['build', '-p', 'towermd', '--release', '--target', target], {
      cwd: root, env, stdio: ['ignore', 'inherit', 'inherit'],
    });
    child.once('error', reject);
    child.once('exit', (code, signal) => {
      if (code === 0 && !signal) resolveBuild();
      else reject(new Error(`CLI 构建失败：exit=${code}, signal=${signal}`));
    });
  });
  const targetDir = resolve(root, env.CARGO_TARGET_DIR || 'target');
  const source = join(targetDir, target, 'release/towermd');
  const metadata = await stat(source).catch(() => {
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
    if (args.length !== 2 || args[0] !== '--target') {
      throw new Error('用法：node scripts/stage-cli.mjs --target <triple>');
    }
    console.log(await stageCli(args[1]));
  } catch (error) {
    console.error(`CLI 暂存失败：${error.message}`);
    process.exitCode = 1;
  }
}
