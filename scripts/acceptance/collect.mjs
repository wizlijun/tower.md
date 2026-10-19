// Audit captured bytes and fixed Git reads only. This is not a second merge gate.
import { readFileSync, writeFileSync, mkdirSync, lstatSync, realpathSync } from 'node:fs';
import { resolve, dirname, join, relative, isAbsolute } from 'node:path';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const hash = value => createHash('sha256').update(value).digest('hex');
const text = value => typeof value === 'string' && value.trim().length > 0;
const oid = value => typeof value === 'string' && /^[a-f0-9]{40}$/.test(value);
const digest = value => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);
const canonical = value => JSON.stringify(Array.isArray(value) ? value.map(sorted) : sorted(value));
function sorted(value) {
  if (Array.isArray(value)) return value.map(sorted);
  if (value && typeof value === 'object') return Object.fromEntries(Object.keys(value).sort().map(key => [key, sorted(value[key])]));
  return value;
}
const same = (a, b) => canonical(a) === canonical(b);
const within = (parent, path) => { const name = relative(parent, path); return name === '' || (!isAbsolute(name) && name !== '..' && !name.startsWith('../')); };
function regular(path) {
  if (!lstatSync(path).isFile() || lstatSync(path).isSymbolicLink() || realpathSync(path) !== resolve(path)) throw Error('ordinary file required');
  const bytes = readFileSync(path);
  if (bytes.length > 64 * 1024 * 1024) throw Error('capture too large');
  return bytes;
}

// The CLI emits only strict leading one-line delivery envelopes before its
// ordinary entire JSON/text payload. Do not scan for a convenient later brace.
export function cliPayload(bytes) {
  let rest = bytes.toString('utf8');
  while (rest.startsWith('{"towermd_message":')) {
    const end = rest.indexOf('\n');
    if (end < 0) throw Error('unterminated delivery envelope');
    const item = JSON.parse(rest.slice(0, end));
    if (Object.keys(item).length !== 1 || !item.towermd_message || typeof item.towermd_message !== 'object') throw Error('invalid delivery envelope');
    rest = rest.slice(end + 1);
  }
  return rest;
}

// Reads already captured actual product DTOs. A canonical merge.done is supplied
// by the core's prove_merge path; this checker compares its identities and Git
// bytes, and does not implement G0–G5 or grant any product authority.
export function inspectCaptured(manifest, captures, git, { task, worktree, expectedSourceSha }) {
  const missing = [], need = (value, field) => { if (!value) missing.push(field); return !!value; };
  for (const [role, item] of Object.entries(manifest.materials ?? {})) need(text(item.sourceRef) && captures.has(role) && digest(item.sha256) && hash(captures.get(role)) === item.sha256, `capture.${role}.hash`);
  const parsed = role => {
    try {
      const bytes = captures.get(role); if (!bytes) throw Error('missing');
      return JSON.parse(manifest.materials[role].format === 'cli-json' ? cliPayload(bytes) : bytes.toString('utf8'));
    } catch { missing.push(`capture.${role}`); return {}; }
  };
  const ref = role => manifest.materials?.[role]?.sourceRef ?? '';
  const source = parsed('input'), session = parsed('session'), card = parsed('task');
  const finish = parsed('finish'), merged = parsed('proof'), tests = parsed('mainTests');
  const proof = merged.data?.proof ?? {}, claimed = proof.claim ?? {}, started = proof.started ?? {}, completed = proof.completed ?? {};
  const frozen = finish.input ?? {}, evidence = finish.evidence ?? {}, report = finish.report ?? {};
  const run = claimed.data?.run ?? {}, originalTask = claimed.data?.task ?? {}, finalTask = merged.data?.task ?? {}, finalRun = merged.data?.run ?? {};
  const actual = (event, kind) => event?.type === kind && Number.isSafeInteger(event.seq) && event.seq > 0 && text(event.eid) && event.stale === false && event.legacy !== true && Number.isFinite(Date.parse(event.t));
  need(manifest.format === 'towermd-p7-captured-v1' && manifest.card === task && /^T(?:0[1-9]|10)$/.test(task) && Number.isSafeInteger(manifest.attempt) && manifest.attempt > 0, 'manifest.identity');
  need(/^(?:refs\/heads\/main|refs\/remotes\/origin\/main)$/.test(manifest.authorityRef ?? ''), 'manifest.authority');
  need(manifest.origin === 'harness_capture', 'manifest.real_capture');
  need(captures.has('cliBinary') && hash(captures.get('cliBinary')) === source.cli_sha256 && captures.has('appBinary') && hash(captures.get('appBinary')) === source.app_sha256, 'source.binary_bytes');
  need(captures.has('cliVersion') && captures.get('cliVersion').toString('utf8').trim() === source.cli_version, 'source.cli_version');
  const skillRoles = ['skillInit', 'skillBaseline', 'skillClaim', 'skillAsk', 'skillPropose', 'skillFinish'];
  need(skillRoles.every(role => captures.has(role) && source.skill_hashes?.[role] === hash(captures.get(role))), 'source.skill_bytes');
  need(source.sha === expectedSourceSha && oid(source.sha) && digest(source.cli_sha256) && digest(source.app_sha256) && source.skill_hashes && Object.keys(source.skill_hashes).length === 6 && Object.values(source.skill_hashes).every(digest), 'source.identity');
  const prerequisiteRole = Object.keys(manifest.materials ?? {}).find(role => ref(role) === source.prerequisite_receipt_ref);
  const prerequisite = prerequisiteRole ? parsed(prerequisiteRole) : {};
  need(text(source.cli_version) && prerequisite.commit === expectedSourceSha && Array.isArray(prerequisite.sources) && Array.isArray(prerequisite.commands) && prerequisite.commands.length > 0 && skillRoles.every(role => {
    const name = role.slice(5).toLowerCase(); return prerequisite.sources.some(item => item.path === `skills/skills/towermd-${name}/SKILL.md` && item.sha256 === source.skill_hashes?.[role]);
  }), 'source.prerequisite');
  need(session.kind === 'human_opened' && ['harness', 'version', 'id', 'transcript_ref'].every(key => text(session[key])) && session.skill_loaded === 'self_report' && ref('transcript') === session.transcript_ref && captures.get('transcript')?.length > 0, 'session.loading');
  need(captures.has('harnessVersion') && captures.get('harnessVersion').toString('utf8').trim() === session.version, 'session.version_bytes');
  need(session.cost_usd === null || (Number.isFinite(session.cost_usd) && session.cost_usd >= 0), 'session.cost');
  need(session.cost_usd === run.cost_usd, 'session.cost_source');
  need(text(run.branch), 'binding.branch');
  need(run.self_reported?.harness === session.harness && run.self_reported?.version === session.version, 'session.claim_declaration');
  need(actual(claimed, 'task.claimed') && claimed.actor === 'process:towermd-cli' && actual(started, 'finish.started') && actual(completed, 'finish.completed') && started.actor === 'process:towermd-verifier' && completed.actor === 'process:towermd-verifier' && claimed.seq < started.seq && started.seq < completed.seq, 'proof.producer_events');
  need(actual(merged, 'merge.done') && merged.actor === 'process:towermd-integrator' && merged.layer === 'L4' && merged.guards?.length === 0 && merged.obj === frozen.task_id && merged.seq > completed.seq && proof.schema === 1 && proof.integration_gate && Array.isArray(proof.review_contexts), 'proof.canonical_merge');
  const terminal = proof.integration_gate ?? {}, gate = terminal.data?.evidence ?? {};
  need(actual(terminal, 'evidence.recorded') && terminal.actor === 'process:towermd-gates' && completed.seq < terminal.seq && terminal.seq < merged.seq && terminal.data?.completed_eid === completed.eid && Array.isArray(terminal.data?.parents) && gate.gate === 'g5' && gate.result === 'passed' && gate.project_id === frozen.project_id && gate.task === frozen.task_id && gate.run === frozen.run_id && gate.baseline === frozen.baseline && gate.commands_hash === frozen.commands_hash && same(gate.fences, frozen.fences) && same(terminal.guards, frozen.fences), 'proof.terminal_identity');
  need(same(finish.actual, completed) && same(started.data?.input, frozen) && same(completed.data?.evidence, evidence) && same(completed.data?.report, report) && same(merged.data?.evidence, evidence), 'proof.finish_wire');
  need(finish.job_state === 'completed' && finish.result === 'passed' && evidence.result === 'passed' && evidence.generated_by === 'process:towermd-verifier', 'evidence.passed');
  need(card.type === 'Task' && card.id === originalTask.id && card.module === originalTask.module && same(card.touches ?? [], originalTask.touches ?? []) && same(card.allowed_paths ?? [], originalTask.allowed_paths ?? []), 'task.card');
  need(finalTask.id === originalTask.id && finalTask.state === 'merged' && finalRun.id === run.id && finalRun.task === originalTask.id && finalRun.state === 'ended' && finalRun.verification_evidence === evidence.id, 'proof.final_projection');
  need(['project_id', 'host', 'root', 'common_dir', 'task_id', 'run_id', 'module_id', 'evidence_id'].every(key => text(frozen[key])) && frozen.root === worktree && run.worktree === worktree && run.host === frozen.host && run.id === frozen.run_id && run.task === frozen.task_id && card.id === frozen.task_id && card.module === frozen.module_id, 'binding.identities');
  const fences = frozen.fences ?? [], names = [card.module, ...(card.touches ?? [])].map(id => `module:${id}`).sort();
  need(fences.length === names.length && new Set(names).size === names.length && same(fences.map(f => f.name).sort(), names) && fences.every(f => f.holder === run.id && Number.isSafeInteger(f.epoch) && f.epoch > 0) && same(run.leases, fences) && same(evidence.fences, fences) && same(started.guards, fences) && same(completed.guards, fences), 'binding.complete_fences');
  need(evidence.id === frozen.evidence_id && evidence.task === frozen.task_id && evidence.run === frozen.run_id && evidence.module === frozen.module_id && ['commit', 'tree', 'base', 'authority', 'baseline', 'plan_hash', 'commands_hash'].every(key => evidence[key] === frozen[key]) && report.commit === frozen.commit && report.tree === frozen.tree, 'evidence.frozen_identity');
  need(oid(frozen.commit) && oid(frozen.tree) && oid(frozen.base) && oid(frozen.authority) && text(frozen.baseline) && Number.isSafeInteger(frozen.entered_verifying_at) && frozen.merge_deadline === frozen.entered_verifying_at + 60 * 60 * 1000 && Date.parse(completed.t) < frozen.merge_deadline, 'evidence.fixed_deadline');
  need(Array.isArray(frozen.commands) && frozen.commands.length > 0 && hash(canonical(frozen.commands)) === frozen.commands_hash && evidence.log?.report_eid === completed.eid && hash(canonical(report)) === evidence.log?.report_sha256, 'evidence.report_hash');
  const executions = report.executions ?? [];
  need(executions.length === frozen.commands?.length && executions.every((execution, i) => same(execution.command, frozen.commands[i]) && execution.result === 'passed' && execution.exit_code === 0 && execution.timed_out === false), 'evidence.executions');
  for (const execution of executions) for (const stream of ['stdout', 'stderr']) {
    const role = Object.keys(manifest.materials ?? {}).find(key => ref(key) === execution[`${stream}_log`]);
    need(role && captures.has(role) && hash(captures.get(role)) === execution[`${stream}_sha256`], `evidence.${stream}_bytes`);
  }
  const verificationRole = Object.keys(manifest.materials ?? {}).find(role => ref(role) === ref('verificationLog') && executions.some(execution => ['stdout', 'stderr'].some(stream => execution[`${stream}_log`] === ref(role) && execution[`${stream}_sha256`] === hash(captures.get(role) ?? ''))));
  need(verificationRole, 'evidence.verification_log');
  const trailers = {}, required = { 'Towermd-Task': frozen.task_id, 'Towermd-Run': frozen.run_id, 'Towermd-Module': frozen.module_id, 'Towermd-Evidence': frozen.evidence_id, 'Towermd-Epoch': String(fences.find(f => f.name === `module:${frozen.module_id}`)?.epoch ?? '') };
  let trailerText = ''; try { trailerText = cliPayload(captures.get('trailers')); } catch { missing.push('trailers.capture'); }
  for (const [key, expected] of Object.entries(required)) {
    const expression = new RegExp(`^${key}: (.+)$`, 'gm');
    const lines = [...trailerText.matchAll(expression)], mainLines = [...(git.mainMessage ?? '').matchAll(expression)];
    need(lines.length === 1 && lines[0][1] === expected && mainLines.length === 1 && mainLines[0][1] === expected, `trailers.${key}`); trailers[key] = expected;
  }
  need(oid(merged.data?.commit) && oid(merged.data?.tree) && git.mainCommit === merged.data?.commit && git.mainTree === merged.data?.tree && git.verifiedTree === frozen.tree && git.authorityContains === true && git.worktree === worktree, 'git.identities');
  const testOutput = ['stdout_ref', 'stderr_ref'].map(key => {
    const role = Object.keys(manifest.materials ?? {}).find(item => ref(item) === tests[key]); return role ? captures.get(role)?.toString('utf8') ?? '' : '';
  }).join('\n');
  const count = /(?:^|\n)Ran ([1-9]\d*) tests? in [^\n]+\n/.exec(testOutput);
  need(tests.commit === git.mainCommit && tests.tree === git.mainTree && tests.exit_code === 0 && same(tests.argv, ['python3', '-m', 'unittest', 'discover', '-s', '.', '-p', 'test_*.py']) && count && /(?:^|\n)OK\s*$/.test(testOutput), 'main.real_tests');
  const humanRefs = [];
  need(new Set((manifest.human ?? []).map(row => row.gate)).size === (manifest.human ?? []).length, 'human.duplicate_gate');
  for (const row of manifest.human ?? []) {
    const event = parsed(row.event), observation = parsed(row.observation); humanRefs.push(ref(row.event), ref(row.observation));
    need(actual(event, row.gate === 'B0' || row.gate === 'B1' ? 'baseline.approved' : row.gate === 'answer' ? 'hold.answered' : 'hold.reviewed') && /^human:[^\s:/]+$/.test(event.actor) && observation.performed_by === 'human' && observation.event_id === event.eid && observation.actor === event.actor && observation.source_sha === expectedSourceSha && text(observation.record_ref) && [...Object.values(manifest.materials ?? {})].some(item => item.sourceRef === observation.record_ref), `human.${row.gate}.origin`);
    need(event.data?.request_body?.project_id === frozen.project_id && event.data?.request_body?.actor === event.actor, `human.${row.gate}.project`);
    if (['B0', 'B1'].includes(row.gate)) {
      const approval = event.data?.approval, publication = parsed(row.publication); humanRefs.push(ref(row.publication));
      need(actual(publication, 'baseline.created') && publication.actor === 'process:towermd-baseline' && publication.seq > event.seq && publication.data?.request_body?.project_id === frozen.project_id && publication.data?.approval_eid === event.eid && publication.data?.content_hash === approval?.binding?.content_hash && publication.data?.target === `towermd/${row.gate}` && publication.data?.tag === publication.data?.target && oid(publication.data?.commit) && git.baselines?.[row.gate] === publication.data?.commit, `human.${row.gate}.publication`);
      need(approval?.actor === event.actor && approval?.approval_eid === event.eid && approval?.verdict === 'approved' && approval?.binding?.project_id === frozen.project_id && approval?.binding?.target_baseline === `towermd/${row.gate}` && digest(approval?.binding?.content_hash), `human.${row.gate}.binding`);
    }
    if (row.gate === 'answer') {
      const hold = event.data?.hold;
      need(hold?.body?.kind === 'ask' && hold?.reply?.id === event.eid && hold?.reply?.actor === event.actor && text(hold?.reply?.text) && hold?.recipients?.some(recipient => recipient.run === run.id && recipient.task === frozen.task_id), 'human.answer.binding');
    }
    if (['G1', 'G4'].includes(row.gate)) {
      const fact = event.data?.fact;
      need(proof.review_contexts?.some(context => context.gate === row.gate.toLowerCase() && same(context.reviewed, event)), `human.${row.gate}.canonical_source`);
      need(fact?.actor === event.actor && fact?.answer_eid === event.eid && fact?.verdict === 'approved' && fact.binding?.run === frozen.run_id && fact.binding?.task === frozen.task_id && fact.binding?.baseline === frozen.baseline && same(fact.binding?.fences, fences) && same(event.guards, fences) && (row.gate === 'G1' ? fact.binding?.subject?.sha256 === frozen.plan_hash : fact.binding?.subject?.tree === frozen.tree), `human.${row.gate}.binding`);
    }
  }
  const humanGates = new Set((manifest.human ?? []).map(row => row.gate));
  for (const gate of ['B0', ...(['T01', 'T04', 'T07', 'T10'].includes(task) ? ['G1', 'G4'] : []), ...(task === 'T08' ? ['B1'] : []), ...(task === 'T09' ? ['answer'] : [])]) need(humanGates.has(gate), `human.required.${gate}`);
  need(Array.isArray(manifest.checkpoints) && manifest.checkpoints.length > 0 && manifest.checkpoints.every(role => captures.has(role)), 'checkpoints.raw');
  const iso = value => Number.isSafeInteger(value) && Number.isFinite(new Date(value).valueOf()) ? new Date(value).toISOString() : '';
  return {
    task, attempt: manifest.attempt, result: missing.length ? 'incomplete' : 'pass',
    source: { sha: source.sha, tag: source.tag ?? null, cli_sha256: source.cli_sha256, app_sha256: source.app_sha256, skill_hashes: source.skill_hashes }, session,
    binding: { project: frozen.project_id, run: run.id, host: run.host, worktree: run.worktree, branch: run.branch, module: frozen.module_id, fences },
    verification: { commit: frozen.commit, tree: frozen.tree, evidence_id: evidence.id, captured_source_ref: ref('finish'), object_ref: null, object_sha256: '', commands_hash: frozen.commands_hash, log_ref: ref('verificationLog'), log_sha256: hash(captures.get('verificationLog') ?? ''), result: evidence.result, entered_at: iso(frozen.entered_verifying_at), deadline: iso(frozen.merge_deadline) },
    main: { authority_ref: manifest.authorityRef, commit: git.mainCommit, tree: git.mainTree, trailers, proof_ref: ref('proof'), merge_done_eid: merged.eid, task_state: finalTask.state, run_state: finalRun.state, test_count: count ? Number(count[1]) : 0, test_exit: tests.exit_code ?? null, test_log_ref: ref('mainTests') },
    human_refs: humanRefs, checkpoint_refs: (manifest.checkpoints ?? []).map(ref), failure_refs: (manifest.failures ?? []).map(ref), missing: [...new Set(missing)],
  };
}

export async function collect({ inputs, worktree, task, out, expectedSourceSha }, run = spawnSync) {
  if (![inputs, worktree, out].every(value => isAbsolute(value ?? '') && resolve(value) === value) || !oid(expectedSourceSha)) throw Error('absolute paths and frozen source SHA required');
  worktree = realpathSync(worktree);
  const base = realpathSync(dirname(inputs)), inputBytes = regular(inputs), manifest = JSON.parse(inputBytes);
  if (realpathSync(dirname(out)) !== dirname(out)) throw Error('canonical output parent required');
  if (within(worktree, out) || within(worktree, base)) throw Error('captures and output must be outside task worktree');
  if (!/^(?:refs\/heads\/main|refs\/remotes\/origin\/main)$/.test(manifest.authorityRef ?? '')) throw Error('fixed recorded main authority ref required');
  const captures = new Map(), copies = {}, refs = new Map();
  for (const [role, item] of Object.entries(manifest.materials ?? {})) {
    if (!/^[A-Za-z][A-Za-z0-9_-]*$/.test(role) || !text(item.sourceRef) || !text(item.path) || isAbsolute(item.path) || !digest(item.sha256)) throw Error('invalid capture role');
    const path = resolve(base, item.path);
    if (!within(base, path)) throw Error('capture path escapes evidence directory');
    const bytes = regular(path); if (hash(bytes) !== item.sha256) throw Error(`capture hash differs: ${role}`);
    if (refs.has(item.sourceRef) && refs.get(item.sourceRef) !== item.sha256) throw Error('contradictory capture sourceRef');
    refs.set(item.sourceRef, item.sha256);
    captures.set(role, bytes); copies[role] = { path: `materials/${role}`, sourceRef: item.sourceRef, sha256: item.sha256 };
  }
  const call = args => {
    const result = run('git', args, { cwd: worktree, shell: false, encoding: 'utf8', timeout: 5000, maxBuffer: 8 * 1024 * 1024 });
    if (result?.status !== 0 || result.error) throw Error('read-only Git command failed'); return result.stdout.trimEnd();
  };
  let event = {}; try { event = JSON.parse(captures.get('proof')); } catch { /* Missing proof remains incomplete; no inferred main. */ }
  const commit = event.data?.commit, verified = (() => { try { return JSON.parse(cliPayload(captures.get('finish'))).input?.commit; } catch { return null; } })();
  const git = { worktree: call(['rev-parse', '--show-toplevel']), head: call(['rev-parse', '--verify', 'HEAD']), headTree: call(['rev-parse', '--verify', 'HEAD^{tree}']), branch: call(['rev-parse', '--abbrev-ref', 'HEAD']), worktrees: call(['worktree', 'list', '--porcelain']) };
  if (oid(commit) && oid(verified)) {
    git.mainCommit = call(['rev-parse', '--verify', `${commit}^{commit}`]); git.mainTree = call(['rev-parse', '--verify', `${commit}^{tree}`]);
    git.verifiedTree = call(['rev-parse', '--verify', `${verified}^{tree}`]); git.mainMessage = call(['log', '-1', '--format=%B', commit]);
    const history = call(['rev-list', '--first-parent', manifest.authorityRef]); git.authorityContains = history.split('\n').includes(commit);
  }
  git.baselines = {};
  for (const gate of new Set((manifest.human ?? []).map(row => row.gate).filter(gate => ['B0', 'B1'].includes(gate)))) git.baselines[gate] = call(['rev-parse', '--verify', `refs/tags/towermd/${gate}^{commit}`]);
  const treePaths = git.worktrees.split('\n').filter(line => line.startsWith('worktree ')).map(line => line.slice(9));
  if (treePaths.some(path => within(path, out) || within(path, base))) throw Error('audit destination must be outside every worktree');
  const record = inspectCaptured(manifest, captures, git, { task, worktree, expectedSourceSha });
  // Never overwrite an earlier failed attempt or a completed evidence bundle.
  mkdirSync(out, { recursive: false }); mkdirSync(join(out, 'materials'));
  writeFileSync(join(out, 'inputs.json'), inputBytes, { flag: 'wx' });
  for (const [role, bytes] of captures) writeFileSync(join(out, 'materials', role), bytes, { flag: 'wx' });
  for (const [name, value] of Object.entries({ 'record.json': record, 'git.json': git, 'capture-index.json': { format: manifest.format, input_sha256: hash(inputBytes), materials: copies } })) writeFileSync(join(out, name), JSON.stringify(value, null, 2) + '\n', { flag: 'wx' });
  return record;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const values = {}, names = ['--inputs', '--worktree', '--task', '--out', '--source-sha'];
    const args = process.argv.slice(2);
    for (let i = 0; i < args.length; i += 2) { if (!names.includes(args[i]) || values[args[i]] || !args[i + 1] || args[i + 1].startsWith('--')) throw Error('invalid argument'); values[args[i]] = args[i + 1]; }
    if (names.some(name => !values[name])) throw Error('missing argument');
    const record = await collect({ inputs: values['--inputs'], worktree: values['--worktree'], task: values['--task'], out: values['--out'], expectedSourceSha: values['--source-sha'] });
    process.stdout.write(JSON.stringify(record, null, 2) + '\n'); if (record.result !== 'pass') process.exitCode = 1;
  } catch (error) { process.stderr.write(`collect: ${error.message}\n`); process.exitCode = 1; }
}
