import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, readFileSync, rmSync, mkdirSync, realpathSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { createHash } from 'node:crypto';
import { collect, inspectCaptured, cliPayload } from './collect.mjs';
import { fixturePublication, fixtureRecheck, fixtureProposal, fixtureInterject } from './producer-fixtures.test-support.mjs';

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


function producerFixture(kind, running = null) {
  const f = fixture(), read = role => JSON.parse(f.captures.get(role)), finish = read('finish'), proof = read('proof');
  const baseline = kind === 'interject' ? 'towermd/B0' : 'towermd/B1';
  const replaceBaseline = value => {
    if (Array.isArray(value)) return value.map(replaceBaseline);
    if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, replaceBaseline(item)]));
    return value === 'towermd/B0' ? baseline : value;
  };
  const nextFinish = replaceBaseline(finish), nextProof = replaceBaseline(proof), actual = nextProof.data.proof;
  for (const [item, seq] of [[actual.claim,10],[actual.started,40],[actual.completed,50],[actual.integration_gate,70],[nextProof,80]]) { item.seq = seq; item.t = new Date(seq * 1000).toISOString(); }
  nextFinish.input.entered_verifying_at = 40_000; nextFinish.input.merge_deadline = 3_640_000;
  actual.started.data.input = structuredClone(nextFinish.input); nextFinish.actual = structuredClone(actual.completed);
  f.put('finish', nextFinish); f.put('proof', nextProof);
  const task = actual.claim.data.task, run = actual.claim.data.run, common = { put: f.put, project: 'project', task, sourceSha: f.options.expectedSourceSha, transcriptRef: 'transcript' };
  if (kind === 'recheck') {
    const publication = fixturePublication('project', 'B1-approval', 2, f.git.mainCommit);
    f.git.baselines.B1 = publication.data.commit;
    f.manifest.human.push(fixtureRecheck({ ...common, publication, running, seq: 5 }));
  } else if (kind === 'proposal') {
    f.options.task = f.manifest.card = 'T08';
    const b1 = structuredClone(read('b0')); b1.seq = 20; b1.t = new Date(20_000).toISOString(); b1.eid = 'B1-approval'; b1.data.approval.approval_eid = b1.eid; b1.data.approval.binding.target_baseline = 'towermd/B1';
    const proposal = fixtureProposal({ ...common, run, fences: run.leases, approval: b1, seq: 15 });
    const publication = fixturePublication('project', b1.eid, 21, f.git.mainCommit);
    f.put('b1', b1); f.put('b1Published', publication); f.put('b1Observation', { performed_by: 'human', event_id: b1.eid, actor: b1.actor, source_sha: f.options.expectedSourceSha, record_ref: 'transcript' });
    f.manifest.human.push({ gate: 'B1', event: 'b1', observation: 'b1Observation', publication: 'b1Published', proposal }); f.git.baselines.B1 = publication.data.commit;
  } else {
    f.options.task = f.manifest.card = 'T06';
    f.manifest.human.push(fixtureInterject({ ...common, run, claim: actual.claim, frozen: nextFinish.input, seq: 20 })); f.manifest.checkpoints.push('promptStatus');
  }
  return f;
}
for (const [name, kind, running] of [
  ['ready recheck has no Run and does not bind final claim', 'recheck', null],
  ['historical running recheck keeps its own complete fences', 'recheck', { run: 'older-run', fences: [{ name: 'module:A-reports', holder: 'older-run', epoch: 6 }], lease_expires_at: { 'module:A-reports': 900_000 } }],
  ['T08 actual proposal links the original Run and reviewed saved draft', 'proposal', null],
  ['T06 exact input opening delivered and leading status remain separate facts', 'interject', null],
]) test(`finite producer mapping: ${name}`, () => { const f = producerFixture(kind, running), result = f.result(); assert.equal(result.result, 'pass', result.missing.join(',')); });
for (const card of ['T07', 'T10']) test(`${card} cannot omit actual required recheck materials`, () => {
  const f = fixture(); f.options.task = f.manifest.card = card; assert(f.result().missing.includes('human.required.recheck'));
});
for (const [name, kind, mutate, field] of [
  ['recheck marker missing', 'recheck', f => f.captures.delete('recheckRequired'), 'human.recheck.required_markers'],
  ['recheck marker belongs to another publication', 'recheck', f => f.mutate('recheckRequired', value => { value.data.fact.publication_id = 'another'; return value; }), 'human.recheck.required_markers'],
  ['recheck request targets another Task', 'recheck', f => f.mutate('recheckEvent', value => { value.data.subject.binding.task_id = 'another'; return value; }), 'human.recheck.binding'],
  ['recheck process is not human', 'recheck', f => f.mutate('recheckEvent', value => ({ ...value, actor: 'process:towermd-cli' })), 'human.recheck.origin'],
  ['recheck receipt altered', 'recheck', f => f.mutate('recheckEvent', value => { value.data.p3_receipt.result_ref.resolved_eid = 'another'; return value; }), 'human.recheck.receipt'],
  ['ready recheck cannot carry Run guards', 'recheck', f => f.mutate('recheckEvent', value => ({ ...value, guards: [{ name: 'module:A-reports', holder: 'R-actual', epoch: 7 }] })), 'human.recheck.ready'],
  ['B1 lacks the original proposal', 'proposal', f => f.captures.delete('proposal'), 'human.B1.proposal_binding'],
  ['proposal originated in another Run', 'proposal', f => f.mutate('proposal', value => { value.data.result.binding.run = 'another'; return value; }), 'human.B1.proposal_binding'],
  ['B1 reviewed a different saved revision', 'proposal', f => f.mutate('proposal', value => { value.data.result.draft.revision = '9'.repeat(40); return value; }), 'human.B1.proposal_subject'],
  ['proposal request body changed', 'proposal', f => f.mutate('proposal', value => { value.data.request_body.action.input.reason = 'different'; return value; }), 'human.B1.proposal_receipt'],
  ['B1 original approval receipt changed', 'proposal', f => f.mutate('b1', value => { value.data.p3_receipt.result_ref.comment = 'different'; return value; }), 'human.B1.approval_receipt'],
  ['interjection lost original opening', 'interject', f => f.captures.delete('promptOpening'), 'human.interject.opening'],
  ['opening names a different prompt', 'interject', f => f.mutate('promptOpening', value => { value.data.prompt.eid = 'another'; return value; }), 'human.interject.opening'],
  ['delivery names a different message', 'interject', f => f.mutate('promptDelivered', value => { value.data.message_ids = ['another']; return value; }), 'human.interject.delivered'],
  ['delivered Hold changes original prompt', 'interject', f => f.mutate('promptDelivered', value => { value.data.hold.body.text = 'different'; return value; }), 'human.interject.delivered'],
  ['opaque checkpoint cannot replace status', 'interject', f => f.mutate('promptStatus', () => ({ result: true })), 'human.interject.next_status'],
  ['non-status command cannot prove next status', 'interject', f => f.mutate('promptStatus', value => ({ ...value, argv: ['task', 'show'] })), 'human.interject.next_status'],
  ['status repeats one stable ID with conflicting text', 'interject', f => { const role = 'promptStatusStdout', raw = f.captures.get(role).toString('utf8'), item = JSON.parse(raw.split('\n')[0]); item.towermd_message.text = 'conflicting'; f.put(role, JSON.stringify(item) + '\n' + raw, f.manifest.materials[role].sourceRef, 'text'); }, 'human.interject.next_status'],
  ['status without leading message', 'interject', f => { const role = 'promptStatusStdout'; f.put(role, cliPayload(f.captures.get(role)), f.manifest.materials[role].sourceRef, 'text'); }, 'human.interject.next_status'],
]) test(`finite producer incomplete: ${name}`, () => { const f = producerFixture(kind); mutate(f); const result = f.result(); assert.equal(result.result, 'incomplete'); assert(result.missing.includes(field), result.missing.join(',')); });
test('unknown human event roles cannot fall through to a review', () => {
  const f = fixture(); f.manifest.human.push({ gate: 'arbitrary', event: 'b0', observation: 'humanObservation' }); assert(f.result().missing.includes('human.unknown_gate'));
});
