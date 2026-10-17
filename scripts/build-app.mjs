import { spawn } from 'node:child_process';
import { readFile, stat } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { stageCli } from './stage-cli.mjs';
import { assertUnsigned, unsignedEnv } from './release/core.mjs';

const projectRoot = fileURLToPath(new URL('../', import.meta.url));
const targets = ['aarch64-apple-darwin', 'x86_64-apple-darwin'];

export async function buildApp(target, {
  root = projectRoot,
  cargo = join(homedir(), '.cargo/bin/cargo'),
  env = process.env,
} = {}) {
  if (!targets.includes(target)) {
    throw new Error(`必须显式指定 --target ${targets.join(' 或 ')}`);
  }
  assertUnsigned(JSON.parse(await readFile(join(root, 'src-tauri/tauri.conf.json'), 'utf8')));
  const buildEnv = unsignedEnv(env, join(root, '.tools/pnpm/node_modules/.bin'));
  if (env.CARGO_TARGET_DIR !== undefined) {
    buildEnv.CARGO_TARGET_DIR = resolve(root, env.CARGO_TARGET_DIR);
  }
  const runPnpm = args => new Promise((resolveBuild, reject) => {
    const child = spawn(process.execPath, [join(root, 'scripts/project-pnpm.mjs'), ...args], {
      cwd: root, env: buildEnv, shell: false, stdio: ['ignore', 'inherit', 'inherit'],
    });
    child.once('error', reject);
    child.once('exit', (code, signal) => {
      if (code === 0 && !signal) resolveBuild();
      else reject(new Error(`App 构建失败：exit=${code}, signal=${signal}`));
    });
  });
  const cli = await stageCli(target, { root, cargo, env: buildEnv });
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
  await runPnpm(['tauri', 'build', '--target', target, '--bundles', 'app']);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const args = process.argv.slice(2);
    if (args.length !== 2 || args[0] !== '--target') {
      throw new Error('用法：node scripts/build-app.mjs --target <triple>');
    }
    await buildApp(args[1]);
  } catch (error) {
    console.error(`App 构建失败：${error.message}`);
    process.exitCode = 1;
  }
}
