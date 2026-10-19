import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, readFileSync, rmSync, mkdirSync, realpathSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { createHash } from 'node:crypto';
import { collect, inspectCaptured, cliPayload } from './collect.mjs';

const hash = text => createHash('sha256').update(text).digest('hex');
function sorted(value) { return Array.isArray(value) ? value.map(sorted) : value && typeof value === 'object' ? Object.fromEntries(Object.keys(value).sort().map(key => [key, sorted(value[key])])) : value; }
const canonicalHash = value => hash(JSON.stringify(sorted(value)));
// Synthetic DTOs exercise mappings only; these tests are never P7 human/Task evidence.
function fixture() {
  const worktree = '/fixture/w2', sourceSha = 'a'.repeat(40), commit = 'b'.repeat(40), tree = 'c'.repeat(40), main = 'd'.repeat(40);
  const captures = new Map(), materials = {};
  const put = (role, value, sourceRef = role, format = 'json') => { const bytes = Buffer.from(typeof value === 'string' ? value : JSON.stringify(value)); captures.set(role, bytes); materials[role] = { path: role, sourceRef, sha256: hash(bytes), format }; };
  const fences = [{ name: 'module:A-reports', holder: 'R-actual', epoch: 7 }];
  const task = { type: 'Task', id: 'T-actual', title: 'actual task', module: 'A-reports', touches: [], allowed_paths: ['reports'], state: 'running' };
  const run = { type: 'Run', id: 'R-actual', task: task.id, host: 'host', worktree, branch: 't/T-actual', state: 'running', leases: fences, cost_usd: null, self_reported: { harness: 'codex', version: '1.0' } };
  const commands = [{ cwd: '.', argv: ['python3', '-m', 'unittest', 'discover', '-s', '.', '-p', 'test_*.py'] }];
  const frozen = { evidence_id: 'E-actual', project_id: 'project', host: 'host', root: worktree, common_dir: '/fixture/w1/.git', task_id: task.id, run_id: run.id, module_id: task.module, commit, tree, base: 'e'.repeat(40), authority: 'e'.repeat(40), baseline: 'towermd/B0', plan_hash: null, commands, commands_hash: canonicalHash(commands), fences, entered_verifying_at: 1_000, merge_deadline: 3_601_000 };
  const event = (type, seq, data, actor = 'process:towermd-verifier', guards = fences) => ({ type, seq, data, actor, guards, eid: `${type}-${seq}`, host: 'host', t: new Date(seq * 1_000).toISOString(), layer: 'L4', obj: task.id, epoch: 7, stale: false, cost_usd: null });
  const claimed = event('task.claimed', 1, { task, run }, 'process:towermd-cli');
  const started = event('finish.started', 2, { task, run, input: frozen, review_context: null });
  const log = 'Ran 6 tests in 0.001s\n\nOK\n';
  put('verificationLog', log, '/cache/verifier/stderr', 'text'); put('verificationStdout', '', '/cache/verifier/stdout', 'text');
  const report = { commit, tree, executions: [{ command: commands[0], result: 'passed', exit_code: 0, timed_out: false, stdout_log: '/cache/verifier/stdout', stderr_log: '/cache/verifier/stderr', stdout_sha256: hash(''), stderr_sha256: hash(log), diagnostic: null }] };
  const evidence = { id: frozen.evidence_id, kind: 'verification', task: task.id, run: run.id, module: task.module, ...Object.fromEntries(['commit', 'tree', 'base', 'authority', 'baseline', 'plan_hash', 'commands_hash', 'fences'].map(key => [key, frozen[key]])), result: 'passed', generated_by: 'process:towermd-verifier', log: { report_eid: 'finish.completed-3', report_sha256: canonicalHash(report) }, gates: {} };
  const completed = event('finish.completed', 3, { task, run, evidence, report, hold: null, review_context: null });
  const merge = event('merge.done', 5, { task: { ...task, state: 'merged' }, run: { ...run, state: 'ended', verification_evidence: evidence.id }, evidence, commit: main, tree, proof: { schema: 1, claim: claimed, started, completed, integration_gate: event('evidence.recorded', 4, { completed_eid: completed.eid, parents: [frozen.base], evidence: { gate: 'g5', result: 'passed', project_id: frozen.project_id, task: task.id, run: run.id, baseline: frozen.baseline, commands_hash: frozen.commands_hash, fences } }, 'process:towermd-gates'), review_contexts: [] } }, 'process:towermd-integrator', []);
  const session = { kind: 'human_opened', harness: 'codex', version: '1.0', id: 'actual-visible-session', skill_loaded: 'self_report', transcript_ref: 'transcript', cost_usd: null };
  put('cliBinary', 'fixture CLI bytes', 'packaged/cli', 'bytes'); put('appBinary', 'fixture App bytes', 'packaged/app', 'bytes'); put('cliVersion', 'towermd 0.0.0\n', 'cli-version', 'text');
  const skillHashes = {}; for (const role of ['skillInit', 'skillBaseline', 'skillClaim', 'skillAsk', 'skillPropose', 'skillFinish']) { put(role, `fixture ${role}`, role, 'text'); skillHashes[role] = materials[role].sha256; }
  put('prerequisite', { commit: sourceSha, sources: Object.entries(skillHashes).map(([role, sha256]) => ({ path: `skills/skills/towermd-${role.slice(5).toLowerCase()}/SKILL.md`, sha256 })), commands: [{ file: 'cargo', status: 0 }] });
  put('input', { sha: sourceSha, tag: null, cli_sha256: materials.cliBinary.sha256, app_sha256: materials.appBinary.sha256, cli_version: 'towermd 0.0.0', skill_hashes: skillHashes, prerequisite_receipt_ref: 'prerequisite' });
  put('task', task); put('session', session); put('harnessVersion', '1.0\n', 'harness-version', 'text'); put('transcript', 'fixture transcript only', 'transcript', 'text');
  put('finish', { input: frozen, evidence, report, actual: completed, job_state: 'completed', result: 'passed' }); put('proof', merge);
  const trailers = { 'Towermd-Task': task.id, 'Towermd-Run': run.id, 'Towermd-Module': task.module, 'Towermd-Evidence': evidence.id, 'Towermd-Epoch': '7' };
  const message = Object.entries(trailers).map(([key, value]) => `${key}: ${value}`).join('\n'); put('trailers', message, 'trailers', 'text');
  put('mainStdout', '', 'main-stdout', 'text'); put('mainStderr', log, 'main-stderr', 'text');
  put('mainTests', { commit: main, tree, argv: commands[0].argv, cwd: worktree, exit_code: 0, stdout_ref: 'main-stdout', stderr_ref: 'main-stderr' });
  const approval = event('baseline.approved', 1, { approval: { actor: 'human:owner', approval_eid: 'baseline.approved-1', verdict: 'approved', binding: { project_id: 'project', target_baseline: 'towermd/B0', content_hash: 'f'.repeat(64) } }, request_body: { project_id: 'project', actor: 'human:owner' } }, 'human:owner', []);
  put('b0Published', event('baseline.created', 2, { approval_eid: approval.eid, content_hash: 'f'.repeat(64), target: 'towermd/B0', tag: 'towermd/B0', commit: main, request_body: { project_id: 'project' } }, 'process:towermd-baseline', []));
  put('b0', approval); put('humanObservation', { performed_by: 'human', event_id: approval.eid, actor: approval.actor, source_sha: sourceSha, record_ref: 'transcript' }); put('checkpoint', { captured: 'actual output' });
  const manifest = { format: 'towermd-p7-captured-v1', origin: 'harness_capture', card: 'T02', attempt: 1, authorityRef: 'refs/remotes/origin/main', materials, human: [{ gate: 'B0', event: 'b0', observation: 'humanObservation', publication: 'b0Published' }], checkpoints: ['checkpoint'], failures: [] };
  const git = { worktree, mainCommit: main, mainTree: tree, verifiedTree: tree, authorityContains: true, mainMessage: message, baselines: { B0: main } };
  const options = { task: 'T02', worktree, expectedSourceSha: sourceSha };
  return { manifest, captures, git, options, put, result: () => inspectCaptured(manifest, captures, git, options), mutate: (role, edit) => put(role, edit(JSON.parse(captures.get(role)))) };
}
test('fixed actual DTO mappings agree in a synthetic mechanism fixture', () => { const f = fixture(); assert.equal(f.result().result, 'pass'); assert.deepEqual(f.result().missing, []); });
for (const [name, mutate, field] of [
  ['missing Evidence', f => f.mutate('finish', value => { delete value.evidence; return value; }), 'evidence.passed'],
  ['done self report without merge proof', f => f.put('proof', { done: true }), 'proof.canonical_merge'],
  ['wrong main tree', f => { f.git.mainTree = '0'.repeat(40); }, 'git.identities'],
  ['missing primary fence', f => f.mutate('finish', value => { value.input.fences = []; return value; }), 'binding.complete_fences'],
  ['unknown cost converted to zero', f => f.mutate('session', value => ({ ...value, cost_usd: 0 })), 'session.cost_source'],
  ['fixture used as real capture', f => { f.manifest.origin = 'fixture'; }, 'manifest.real_capture'],
  ['programmatic AppGate called human', f => f.mutate('humanObservation', value => ({ ...value, performed_by: 'agent_qa' })), 'human.B0.origin'],
  ['CLI impersonates App', f => f.mutate('b0', value => ({ ...value, actor: 'process:towermd-cli' })), 'human.B0.origin'],
  ['missing high-risk reviews', f => { f.manifest.card = 'T01'; f.options.task = 'T01'; }, 'human.required.G1'],
  ['wrong trailer Epoch', f => f.put('trailers', f.captures.get('trailers').toString().replace('Epoch: 7', 'Epoch: 8'), 'trailers', 'text'), 'trailers.Towermd-Epoch'],
  ['approval without published baseline', f => f.captures.delete('b0Published'), 'human.B0.publication'],
  ['wrong terminal completed Event', f => f.mutate('proof', value => { value.data.proof.integration_gate.data.completed_eid = 'another'; return value; }), 'proof.terminal_identity'],
  ['missing test execution log', f => f.captures.delete('mainStderr'), 'main.real_tests'],
]) test(`incomplete: ${name}`, () => { const f = fixture(); mutate(f); const result = f.result(); assert.equal(result.result, 'incomplete'); assert.ok(result.missing.includes(field), result.missing.join(',')); });
test('leading delivery envelopes preserve one entire pretty JSON payload', () => {
  assert.deepEqual(JSON.parse(cliPayload(Buffer.from('{"towermd_message":{"text":"pause"}}\n{\n"input": {}\n}'))), { input: {} });
  assert.throws(() => cliPayload(Buffer.from('{"towermd_message":{}}')), /unterminated/);
  assert.throws(() => JSON.parse(cliPayload(Buffer.from('noise\n{"input":{}}'))));
});
test('collector executes only finite read-only Git argv and copies external bytes once', async t => {
  const f = fixture(), root = realpathSync(mkdtempSync(join(tmpdir(), 'towermd-collect-test-')));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const tree = join(root, 'worktree'), evidence = join(root, 'evidence'), out = join(root, 'collected'); mkdirSync(tree); mkdirSync(evidence);
  // The mappings intentionally become incomplete after moving the synthetic path;
  // this test proves no CLI, HTTP, test, lease, delivery or write-to-worktree call.
  for (const [role, bytes] of f.captures) writeFileSync(join(evidence, role), bytes);
  const inputs = join(evidence, 'inputs.json'); writeFileSync(inputs, JSON.stringify(f.manifest));
  const calls = [];
  const run = (file, args, config) => {
    calls.push(args); assert.equal(file, 'git'); assert.equal(config.cwd, tree); assert.equal(config.shell, false);
    const answer = args[0] === 'symbolic-ref' ? 't/T-actual' : args[0] === 'worktree' ? `worktree ${tree}\n` : args.includes('--show-toplevel') ? tree : args[0] === 'log' ? f.git.mainMessage : args[0] === 'rev-list' ? f.git.mainCommit : args.at(-1).endsWith('^{tree}') ? f.git.mainTree : f.git.mainCommit;
    assert.ok(['rev-parse', 'symbolic-ref', 'worktree', 'log', 'rev-list'].includes(args[0])); return { status: 0, stdout: answer };
  };
  const result = await collect({ inputs, worktree: tree, task: 'T02', out, expectedSourceSha: f.options.expectedSourceSha }, run);
  assert.equal(result.result, 'incomplete'); assert.ok(calls.length > 4);
  assert.deepEqual(readFileSync(join(out, 'materials', 'proof')), f.captures.get('proof'));
  await assert.rejects(collect({ inputs, worktree: tree, task: 'T02', out, expectedSourceSha: f.options.expectedSourceSha }, run), /EEXIST/);
  await assert.rejects(collect({ inputs, worktree: tree, task: 'T02', out: join(tree, 'audit'), expectedSourceSha: f.options.expectedSourceSha }, run), /outside task worktree/);
});
