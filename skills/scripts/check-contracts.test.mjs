import { test } from 'node:test';
import assert from 'node:assert/strict';
import { cpSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkCli, checkPackage } from './check-contracts.mjs';

const packageRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
function fixture(t) {
  const parent = mkdtempSync(join(tmpdir(), 'towermd-skills-test-'));
  const root = join(parent, 'package');
  cpSync(packageRoot, root, { recursive: true });
  t.after(() => rmSync(parent, { recursive: true, force: true }));
  return root;
}
function changeJson(root, path, mutate) {
  const file = join(root, path), data = JSON.parse(readFileSync(file, 'utf8'));
  mutate(data);
  writeFileSync(file, JSON.stringify(data));
}
const contracts = root => JSON.parse(readFileSync(join(root, 'references/cli-contracts.json'), 'utf8')).contracts;
function helpRunner(root, calls, alter = output => output) {
  const version = JSON.parse(readFileSync(join(root, '.claude-plugin/plugin.json'), 'utf8')).version;
  return (binary, args, options) => {
    calls.push({ binary, args, options });
    assert(args.at(-1) === '--help' || args.join(' ') === '--version');
    const relevant = contracts(root).filter(c => JSON.stringify(c.help_argv) === JSON.stringify(args));
    const output = args[0] === '--version' ? `towermd ${version}\n` : relevant.flatMap(c => c.required_tokens).join('\n');
    return { status: 0, stdout: alter(output, args), stderr: '' };
  };
}

test('current package reports the real propose gap without hiding usable workflow structure', t => {
  const diagnostics = checkPackage(fixture(t));
  assert.equal(diagnostics.length, 1, JSON.stringify(diagnostics));
  assert.equal(diagnostics[0].code, 'missing_workflow');
  assert.equal(diagnostics[0].path, 'skills/towermd-propose/SKILL.md');
});

test('loadable scaffolds are rejected as missing workflows', t => {
  const root = fixture(t);
  changeJson(root, 'references/cli-contracts.json', data => { data.blocked_workflows = {}; });
  for (const name of ['init', 'baseline', 'claim', 'ask', 'propose', 'finish']) {
    writeFileSync(join(root, `skills/towermd-${name}/SKILL.md`), `---\nname: towermd-${name}\ndescription: A loadable fixture for ${name}.\n---\n\nRead [contract](../../references/session-contract.md).\n此技能尚未接入完整行为；停止。\n`);
  }
  assert.equal(checkPackage(root).filter(d => d.code === 'missing_workflow').length, 6);
});

test('an invented CLI command in a runnable fence is rejected', t => {
  const root = fixture(t), path = join(root, 'skills/towermd-baseline/SKILL.md');
  writeFileSync(path, readFileSync(path, 'utf8') + '\n```sh\n"$TOWERMD_CLI" baseline verify\n```\n');
  assert(checkPackage(root).some(d => d.code === 'unlisted_cli'));
});

test('a registered marker cannot authorize different flags', t => {
  const root = fixture(t), path = join(root, 'skills/towermd-ask/SKILL.md');
  writeFileSync(path, readFileSync(path, 'utf8').replace('review request --gate G4', 'review request --gate G4 --approve'));
  assert(checkPackage(root).some(d => d.code === 'unlisted_cli'));
});

test('escaping local links and missing resources are rejected', t => {
  const root = fixture(t), path = join(root, 'INSTALL.md');
  writeFileSync(path, readFileSync(path, 'utf8') + '\n[escape](../outside.md)\n[missing](missing.md)\n');
  assert.equal(checkPackage(root).filter(d => d.code === 'invalid_link').length, 2);
});

test('a linked outside resource is never accepted as a packaged contract', t => {
  const root = fixture(t), path = join(root, 'references/cli-contracts.json');
  rmSync(path);
  symlinkSync('/etc/hosts', path);
  assert(checkPackage(root).some(d => d.code === 'path_escape'));
  assert(checkCli(root, '/absolute/towermd').some(d => d.code === 'invalid_package'));
});

test('manifest cannot install hooks or MCP implicitly', t => {
  const root = fixture(t);
  changeJson(root, '.claude-plugin/plugin.json', data => { data.hooks = { SessionStart: 'claim' }; });
  assert(checkPackage(root).some(d => d.code === 'invalid_manifest'));
});

test('help probes use argv, bounded time, cache and never execute business examples', t => {
  const root = fixture(t), calls = [];
  assert.deepEqual(checkCli(root, '/absolute/towermd', helpRunner(root, calls)), []);
  assert.equal(calls.length, new Set(contracts(root).map(c => JSON.stringify(c.help_argv))).size);
  for (const call of calls) {
    assert.equal(call.options.shell, false);
    assert.equal(call.options.timeout, 5000);
    assert.equal(call.options.maxBuffer, 1024 * 1024);
    assert.equal(call.binary, '/absolute/towermd');
  }
});

test('missing real help flags, failures and timeouts cannot pass', t => {
  const root = fixture(t);
  assert(checkCli(root, '/absolute/towermd', helpRunner(root, [], output => output.replaceAll('--wait', ''))).some(d => d.code === 'missing_cli_contract'));
  assert(checkCli(root, '/absolute/towermd', () => ({ status: 1, stdout: '' })).length > 0);
  assert(checkCli(root, '/absolute/towermd', () => ({ status: null, error: new Error('ETIMEDOUT') })).length > 0);
});

test('unsafe human or gate-skipping flags and a different binary version are rejected', t => {
  const root = fixture(t);
  assert(checkCli(root, '/absolute/towermd', helpRunner(root, [], output => `${output}\n--actor <ACTOR>`)).some(d => d.code === 'unsafe_cli_contract'));
  assert(checkCli(root, '/absolute/towermd', helpRunner(root, [], (output, args) => args[0] === '--version' ? 'towermd 9.9.9\n' : output)).some(d => d.code === 'missing_cli_contract'));
});

test('a modified contract cannot execute a business command as a probe', t => {
  const root = fixture(t);
  changeJson(root, 'references/cli-contracts.json', data => { data.contracts[0].help_argv = ['task', 'claim', 'T01']; });
  let calls = 0;
  assert(checkCli(root, '/absolute/towermd', () => { calls += 1; }).some(d => d.code === 'invalid_package'));
  assert.equal(calls, 0);
});

test('CLI location must be explicit, not resolved from user PATH', t => {
  const root = fixture(t);
  assert.equal(checkCli(root, 'towermd')[0].code, 'invalid_cli');
});


test('pause and resume help probes require all and original request identity without running controls', t => {
  const root = fixture(t), calls = [];
  assert.deepEqual(checkCli(root, '/absolute/towermd', helpRunner(root, calls)), []);
  for (const command of ['pause', 'resume']) {
    assert.equal(calls.filter(call => JSON.stringify(call.args) === JSON.stringify([command, '--help'])).length, 1);
    for (const token of ['--all', '--request-id']) {
      const diagnostics = checkCli(root, '/absolute/towermd', helpRunner(root, [], (output, args) => args[0] === command ? output.replaceAll(token, '') : output));
      assert(diagnostics.some(d => d.code === 'missing_cli_contract' && d.path === `${command}-all`));
    }
  }
  assert(calls.every(call => call.args.at(-1) === '--help' || call.args.join(' ') === '--version'));
});
