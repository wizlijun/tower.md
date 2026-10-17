import { spawn } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { delimiter, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const { packageManager } = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
const version = packageManager.match(/^pnpm@([^+]+)\+sha512\.[a-f0-9]+$/)?.[1];
if (!version) throw new Error('packageManager 必须固定 pnpm 版本和完整 integrity');
const prefix = join(root, '.tools/pnpm');
const entry = join(prefix, 'node_modules/pnpm/bin/pnpm.cjs');
const env = { ...process.env, PATH: `${join(prefix, 'node_modules/.bin')}${delimiter}${process.env.PATH ?? ''}` };

function run(command, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd: root, env, stdio: 'inherit' });
    const handlers = ['SIGINT', 'SIGTERM', 'SIGHUP'].map((signal) => {
      const forward = () => child.kill(signal);
      process.on(signal, forward);
      return [signal, forward];
    });
    const cleanup = () => handlers.forEach(([signal, forward]) => process.off(signal, forward));
    child.once('error', (error) => { cleanup(); reject(error); });
    child.once('exit', (code, signal) => { cleanup(); resolve({ code, signal }); });
  });
}

function finish({ code, signal }) {
  if (signal) process.kill(process.pid, signal);
  else process.exitCode = code ?? 1;
}

try {
  let installed = { code: 0, signal: null };
  if (!existsSync(entry)) {
    installed = await run(process.platform === 'win32' ? 'npm.cmd' : 'npm', [
      'install', '--prefix', prefix, '--no-save', '--package-lock=false', `pnpm@${version}`,
    ]);
  }
  if (installed.code !== 0 || installed.signal) finish(installed);
  else finish(await run(process.execPath, [entry, ...process.argv.slice(2)]));
} catch (error) {
  console.error(`项目 pnpm 启动失败：${error.message}`);
  process.exitCode = 1;
}
