// Creates one new disposable fixture only. No B0, Task, human event or harness.
import { lstatSync, realpathSync, mkdirSync, writeFileSync, readFileSync, accessSync, constants } from 'node:fs';
import { resolve, dirname, join, isAbsolute } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';

const cardsPath = fileURLToPath(new URL('./task-cards.json', import.meta.url));
const cards = () => JSON.parse(readFileSync(cardsPath, 'utf8'));
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const oid = value => typeof value === 'string' && /^[a-f0-9]{40}$/.test(value);
const token = value => typeof value === 'string' && /^[A-Za-z0-9][A-Za-z0-9._:-]*$/.test(value);

// IDs come from the actually approved B0. Natural-language examples are not
// Acceptance IDs, and these argv are still validated by the real CLI/daemon.
export function taskArguments(card, { moduleIds, acceptanceIds }) {
  if (!token(card.id) || !token(moduleIds?.[card.module]) || !Array.isArray(acceptanceIds?.[card.id]) || !acceptanceIds[card.id].length || !acceptanceIds[card.id].every(token) || !card.touches.every(key => token(moduleIds[key]))) throw Error('actual B0 Module/Acceptance ID mapping required');
  return ['task', 'add', '--id', card.id, '--title', card.title, '--module', moduleIds[card.module],
    ...card.touches.flatMap(key => ['--touches', moduleIds[key]]),
    ...card.allowed_paths.flatMap(path => ['--allow', path]),
    ...card.depends_on.flatMap(id => ['--depends-on', id]),
    ...acceptanceIds[card.id].flatMap(id => ['--acceptance', id])];
}

const seed = {
  '.gitignore': '/.agents/skills/\n/.claude/skills/\n__pycache__/\n*.pyc\n',
  'README.md': '# Tower.md M1 acceptance fixture\n\nDisposable three-module Python project. The ten task implementations are intentionally absent.\n',
  'ledger/__init__.py': '',
  'ledger/base.py': 'def sum_minor(values):\n    return sum(values)\n',
  'ledger/test_base.py': 'import unittest\nfrom ledger.base import sum_minor\n\nclass SumMinor(unittest.TestCase):\n    def test_positive_and_negative(self):\n        self.assertEqual(sum_minor([1, 2, -1]), 2)\n    def test_empty(self):\n        self.assertEqual(sum_minor([]), 0)\n',
  'reports/__init__.py': '',
  'reports/base.py': 'def heading(text):\n    return "# " + text\n',
  'reports/test_base.py': 'import unittest\nfrom reports.base import heading\n\nclass Heading(unittest.TestCase):\n    def test_text(self):\n        self.assertEqual(heading("x"), "# x")\n    def test_empty(self):\n        self.assertEqual(heading(""), "# ")\n',
  'filters/__init__.py': '',
  'filters/base.py': 'def equal(left, right):\n    return left == right\n',
  'filters/test_base.py': 'import unittest\nfrom filters.base import equal\n\nclass Equal(unittest.TestCase):\n    def test_same(self):\n        self.assertTrue(equal(1, 1))\n    def test_different(self):\n        self.assertFalse(equal(1, 2))\n',
};

export function prepare({ root, sourceSha, cli }, run = spawnSync) {
  if (!isAbsolute(root ?? '') || resolve(root) !== root || !oid(sourceSha) || !isAbsolute(cli ?? '') || resolve(cli) !== cli) throw Error('new absolute root, CLI and source SHA required');
  // lstat also catches a dangling symlink; do not follow or remove any target.
  try { lstatSync(root); throw Error('root already exists'); } catch (error) { if (error.code !== 'ENOENT') throw error; }
  if (realpathSync(dirname(root)) !== dirname(root) || realpathSync(cli) !== cli || !lstatSync(cli).isFile()) throw Error('canonical parent and ordinary CLI required');
  accessSync(cli, constants.X_OK);
  const records = [];
  const call = (file, args, cwd, accepted = [0]) => {
    const started = new Date().toISOString();
    const result = run(file, args, { cwd, shell: false, encoding: 'utf8', timeout: 30_000, maxBuffer: 4 * 1024 * 1024, ...(file === 'git' ? { env: Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith('GIT_'))) } : {}) });
    records.push({ file, args, cwd, started, finished: new Date().toISOString(), status: result?.status ?? null, stdout: result?.stdout ?? '', stderr: result?.stderr ?? '' });
    if (result?.error || !accepted.includes(result?.status)) throw Error(`fixture tool failed: ${file} ${args[0]}`);
    return result;
  };
  const parent = dirname(root), version = call(cli, ['--version'], parent).stdout.trim();
  if (!/^towermd \d+\.\d+\.\d+$/.test(version)) throw Error('unexpected CLI version');
  call('git', ['--version'], parent); call('python3', ['--version'], parent);
  const cliHash = hash(readFileSync(cli));
  const list = cards();
  if (!Array.isArray(list.tasks) || list.tasks.length !== 10 || list.tasks.some((card, i) => card.id !== `T${String(i + 1).padStart(2, '0')}`)) throw Error('fixed ten task cards required');
  mkdirSync(root, { mode: 0o700 });
  const origin = join(root, 'origin.git'), worktrees = [1, 2, 3].map(i => join(root, `w${i}`)), evidence = join(root, 'evidence');
  mkdirSync(evidence, { mode: 0o700 });
  const save = name => writeFileSync(join(evidence, name), JSON.stringify(records, null, 2) + '\n');
  const git = (cwd, args, accepted) => call('git', ['-c', 'core.hooksPath=/dev/null', ...args], cwd, accepted);
  try {
    git(root, ['init', '--bare', '--initial-branch=main', origin]);
    git(root, ['clone', origin, worktrees[0]]);
    for (const [path, body] of Object.entries(seed)) { const target = join(worktrees[0], path); mkdirSync(dirname(target), { recursive: true }); writeFileSync(target, body, { flag: 'wx' }); }
    const testArgv = ['-m', 'unittest', 'discover', '-s', '.', '-p', 'test_*.py'];
    const result = call('python3', testArgv, worktrees[0]), output = result.stdout + '\n' + result.stderr;
    const count = /(?:^|\n)Ran ([1-9]\d*) tests? in [^\n]+\n/.exec(output);
    if (!count || Number(count[1]) < 6 || !/(?:^|\n)OK\s*$/.test(output)) throw Error('seed requires at least six real unittest tests');
    git(worktrees[0], ['add', '--', ...Object.keys(seed)]);
    git(worktrees[0], ['-c', 'user.name=Tower.md-Acceptance', '-c', 'user.email=acceptance@localhost', '-c', 'commit.gpgsign=false', 'commit', '-m', 'Seed disposable M1 acceptance fixture']);
    git(worktrees[0], ['push', 'origin', 'HEAD:refs/heads/main']);
    const commit = git(worktrees[0], ['rev-parse', '--verify', 'HEAD']).stdout.trim();
    if (!oid(commit)) throw Error('invalid seed commit');
    git(worktrees[0], ['switch', '--detach', commit]);
    for (const tree of worktrees.slice(1)) git(worktrees[0], ['worktree', 'add', '--detach', tree, commit]);
    for (const tree of worktrees) {
      if (git(tree, ['symbolic-ref', '-q', 'HEAD'], [1]).stdout !== '') throw Error('fixture worktrees must be detached');
      if (git(tree, ['status', '--porcelain=v2', '--untracked-files=all']).stdout !== '') throw Error('fixture worktree is dirty');
    }
    if (hash(readFileSync(cli)) !== cliHash) throw Error('CLI changed during fixture preparation');
    const taskCards = join(evidence, 'task-cards.json'); writeFileSync(taskCards, JSON.stringify(list, null, 2) + '\n', { flag: 'wx' });
    const receipt = { format: 'towermd-p7-preparation-v1', source_sha: sourceSha, cli, cli_sha256: cliHash, cli_version: version, project: worktrees[0], worktrees, origin, evidence, cards: taskCards, seed_commit: commit, test_count: Number(count[1]), tasks: list.tasks.map(card => ({ id: card.id, status: 'not_run' })), next: { init_argv: [cli, 'init', worktrees[0], '--docs-dir', 'tower-docs'], note: '实际init后人工检查生成文件再提交；真实S6/B0批准后解析Module与Acceptance ID，逐卡经CLI登记。ledger risk high，三模块verify均为全量unittest；未启动harness或造B0/Task/human状态。' } };
    save('prepare-commands.json'); writeFileSync(join(evidence, 'prepare-receipt.json'), JSON.stringify(receipt, null, 2) + '\n', { flag: 'wx' }); return receipt;
  } catch (error) {
    // Preserve only our new fixture and failure evidence for an explicit retry
    // at a new root. Never recursively remove a user path to make a test pass.
    save('prepare-failed-commands.json'); throw error;
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const values = {}, args = process.argv.slice(2), allowed = ['--root', '--source-sha', '--cli'];
    for (let i = 0; i < args.length; i += 2) { if (!allowed.includes(args[i]) || values[args[i]] || !args[i + 1] || args[i + 1].startsWith('--')) throw Error('invalid argument'); values[args[i]] = args[i + 1]; }
    if (allowed.some(name => !values[name])) throw Error('missing argument');
    process.stdout.write(JSON.stringify(prepare({ root: values['--root'], sourceSha: values['--source-sha'], cli: values['--cli'] }), null, 2) + '\n');
  } catch (error) { process.stderr.write(`prepare: ${error.message}\n`); process.exitCode = 1; }
}
