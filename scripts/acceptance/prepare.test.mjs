import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, realpathSync, writeFileSync, readFileSync, rmSync, existsSync, symlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { prepare, taskArguments } from './prepare.mjs';

function fixture(t) {
  const parent = realpathSync(mkdtempSync(join(tmpdir(), 'towermd-accept-prepare-')));
  t.after(() => rmSync(parent, { recursive: true, force: true }));
  const root = join(parent, 'new-fixture'), cli = join(parent, 'fixture-cli');
  writeFileSync(cli, 'fixture CLI bytes\n', { mode: 0o700 });
  const calls = [];
  const run = (file, args, options) => {
    calls.push({ file, args, options }); assert.equal(options.shell, false);
    if (file === cli) { assert.deepEqual(args, ['--version']); return { status: 0, stdout: 'towermd 0.0.0\n', stderr: '' }; }
    assert.ok(['git', 'python3'].includes(file));
    if (file === 'git') assert.ok(Object.keys(options.env).every(key => !key.startsWith('GIT_')));
    return spawnSync(file, args, options);
  };
  return { parent, root, cli, calls, run, input: { root, cli, sourceSha: 'a'.repeat(40) } };
}
test('new fixture has exactly three clean detached worktrees and six real seed tests', t => {
  const f = fixture(t), receipt = prepare(f.input, f.run);
  assert.equal(receipt.worktrees.length, 3); assert.ok(receipt.test_count >= 6);
  assert.ok(receipt.tasks.every(task => task.status === 'not_run'));
  assert.deepEqual(receipt.next.init_argv, [f.cli, 'init', receipt.project, '--docs-dir', 'tower-docs']);
  for (const tree of receipt.worktrees) {
    assert.equal(spawnSync('git', ['symbolic-ref', '-q', 'HEAD'], { cwd: tree }).status, 1);
    assert.equal(spawnSync('git', ['status', '--porcelain=v2', '--untracked-files=all'], { cwd: tree, encoding: 'utf8' }).stdout, '');
    assert.equal(existsSync(join(tree, '.towermd')), false, 'no product state before actual init');
    assert.equal(existsSync(join(tree, 'ledger/money.py')), false, 'T01 implementation is absent');
    assert.equal(existsSync(join(tree, 'reports/markdown.py')), false, 'T08 implementation is absent');
    assert.equal(existsSync(join(tree, 'filters/combine.py')), false, 'T09 implementation is absent');
  }
  assert.equal(f.calls.filter(call => call.file === f.cli).length, 1, 'only --version; no business or model call');
  assert.equal(f.calls.some(call => call.args.includes('--global')), false);
  assert.throws(() => prepare(f.input, f.run), /already exists/);
  const cards = JSON.parse(readFileSync(receipt.cards));
  assert.equal(cards.tasks.filter(card => card.harnessClass === 'codex').length, 5);
  assert.equal(cards.tasks.filter(card => card.harnessClass === 'claudeCode').length, 5);
  assert.deepEqual(cards.tasks[9].touches, ['ledger', 'filters']);
  assert.deepEqual(cards.tasks[9].allowed_paths, ['reports/**']);
});
test('existing or dangling symlink root refuses before any command or replacement', t => {
  const f = fixture(t); symlinkSync(join(f.parent, 'absent'), f.root);
  assert.throws(() => prepare(f.input, f.run), /already exists/); assert.equal(f.calls.length, 0);
});
test('tool failure before creation preserves the absent root', t => {
  const f = fixture(t);
  assert.throws(() => prepare(f.input, (file, args, options) => file === 'python3' ? { status: 127, stdout: '', stderr: 'missing' } : f.run(file, args, options)), /fixture tool failed/);
  assert.equal(existsSync(f.root), false);
});
test('failure after creation preserves only its partial fixture and command evidence', t => {
  const f = fixture(t);
  assert.throws(() => prepare(f.input, (file, args, options) => file === 'git' && args.includes('clone') ? { status: 2, stdout: '', stderr: '' } : f.run(file, args, options)), /fixture tool failed/);
  assert.ok(existsSync(join(f.root, 'origin.git')));
  assert.ok(existsSync(join(f.root, 'evidence/prepare-failed-commands.json')));
  assert.throws(() => prepare(f.input, f.run), /already exists/);
});
test('task argv requires actual B0 object IDs and never sends examples as Acceptance IDs', () => {
  const card = { id: 'T10', title: 'summary', module: 'reports', touches: ['ledger', 'filters'], allowed_paths: ['reports/**'], depends_on: ['T07', 'T08', 'T09'], acceptance_examples: ['actual readable example'] };
  assert.throws(() => taskArguments(card, {}), /actual B0/);
  const args = taskArguments(card, { moduleIds: { reports: 'A-reports', ledger: 'A-ledger', filters: 'A-filters' }, acceptanceIds: { T10: ['S-summary'] } });
  assert.deepEqual(args.slice(0, 8), ['task', 'add', '--id', 'T10', '--title', 'summary', '--module', 'A-reports']);
  assert.equal(args.includes('actual readable example'), false);
  assert.deepEqual(args.slice(-2), ['--acceptance', 'S-summary']);
  assert.equal(args.includes('--approve'), false);
});
