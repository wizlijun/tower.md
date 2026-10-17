import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { cpSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { delimiter, join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const packageManager = 'pnpm@11.7.0+sha512.19cc852c120c7125760f2443ee6be0ca5b40f9f50598de1a09a1f177503e010e57c23c77646e01e761de59bf874fb22a3398c33ab9691fc13eb946b6f0f4d620';

test('packageManager 固定 Note.md 的完整 pnpm 标识', () => {
  assert.equal(JSON.parse(readFileSync(join(root, 'package.json'))).packageManager, packageManager);
});

test('首次安装使用项目 npm prefix，同一次调用直接运行固定入口并复用', () => {
  const project = realpathSync(mkdtempSync(join(tmpdir(), 'towermd-pnpm-')));
  try {
    mkdirSync(join(project, 'scripts'));
    mkdirSync(join(project, 'bin'));
    cpSync(join(root, 'scripts/project-pnpm.mjs'), join(project, 'scripts/project-pnpm.mjs'));
    writeFileSync(join(project, 'package.json'), JSON.stringify({ packageManager }));
    const pnpm = `const fs = require('node:fs');
if (process.argv[2] === '--version') console.log('11.7.0');
else { console.log(JSON.stringify({args:process.argv.slice(2),path:process.env.PATH})); process.exitCode = 23; }`;
    writeFileSync(join(project, 'bin/npm'), `#!${process.execPath}\nconst fs=require('node:fs'),p=require('node:path');
fs.appendFileSync('install.jsonl', JSON.stringify(process.argv.slice(2))+'\\n');
const prefix=process.argv[process.argv.indexOf('--prefix')+1];
fs.mkdirSync(p.join(prefix,'node_modules/pnpm/bin'),{recursive:true});
fs.writeFileSync(p.join(prefix,'node_modules/pnpm/bin/pnpm.cjs'),${JSON.stringify(pnpm)});\n`, {mode:0o755});
    for (const command of ['pnpm', 'corepack']) {
      writeFileSync(join(project, 'bin', command), `#!${process.execPath}\nprocess.exit(99);\n`, {mode:0o755});
    }
    const env = {...process.env, PATH:join(project, 'bin')};
    const run = (...args) => spawnSync(process.execPath, ['scripts/project-pnpm.mjs', ...args], {cwd:project,env,encoding:'utf8'});
    const first = run('--version');
    assert.equal(first.status, 0, first.stderr);
    assert.equal(first.stdout.trim(), '11.7.0');
    assert.deepEqual(JSON.parse(readFileSync(join(project, 'install.jsonl'), 'utf8').trim()), ['install','--prefix',join(project,'.tools/pnpm'),'--no-save','--package-lock=false','pnpm@11.7.0']);
    const second = run('test', '中文 空格', '$(unchanged)');
    assert.equal(second.status, 23);
    const forwarded = JSON.parse(second.stdout);
    assert.deepEqual(forwarded.args, ['test','中文 空格','$(unchanged)']);
    assert.equal(forwarded.path.split(delimiter)[0], join(project,'.tools/pnpm/node_modules/.bin'));
    assert.equal(readFileSync(join(project, 'install.jsonl'), 'utf8').trim().split('\n').length, 1);
    assert.deepEqual(run('--version').stdout.trim(), '11.7.0');
  } finally {
    rmSync(project,{recursive:true,force:true});
  }
});

// 独立进程验证 Ctrl-C/终止不会留下 pnpm 子进程。
test('终止信号转发给项目 pnpm 并保留信号退出语义', { timeout: 10000 }, async () => {
  const project = realpathSync(mkdtempSync(join(tmpdir(), 'towermd-pnpm-signal-')));
  let child;
  try {
    mkdirSync(join(project, 'scripts'));
    mkdirSync(join(project, '.tools/pnpm/node_modules/pnpm/bin'), { recursive: true });
    cpSync(join(root, 'scripts/project-pnpm.mjs'), join(project, 'scripts/project-pnpm.mjs'));
    writeFileSync(join(project, 'package.json'), JSON.stringify({ packageManager }));
    writeFileSync(join(project, '.tools/pnpm/node_modules/pnpm/bin/pnpm.cjs'), "console.log('ready'); setTimeout(() => process.exit(42), 3000);\n");
    child = spawn(process.execPath, ['scripts/project-pnpm.mjs'], { cwd: project, stdio: ['ignore', 'pipe', 'pipe'] });
    const exited = new Promise((resolve, reject) => {
      child.once('error', reject);
      child.once('exit', (code, signal) => resolve({ code, signal }));
    });
    await new Promise((resolve, reject) => {
      child.once('error', reject);
      child.stdout.once('data', resolve);
      child.once('exit', () => reject(new Error('pnpm 未启动就退出')));
    });
    child.kill('SIGTERM');
    assert.deepEqual(await exited, { code: null, signal: 'SIGTERM' });
  } finally {
    child?.kill('SIGKILL');
    rmSync(project, { recursive: true, force: true });
  }
});
