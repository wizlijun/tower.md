// Synthetic finite DTO mechanism fixtures only. Never real P7/human evidence.
import { createHash } from 'node:crypto';
const clone = value => JSON.parse(JSON.stringify(value));
export const fixtureHash = value => createHash('sha256').update(JSON.stringify(value, function (_, item) { return item && typeof item === 'object' && !Array.isArray(item) ? Object.fromEntries(Object.keys(item).sort().map(key => [key,item[key]])) : item; })).digest('hex');
const oid = 'd'.repeat(40), digest = 'a'.repeat(64);
const event = (type, seq, data, project, task, actor, guards = [], host = 'host') => ({ type, seq, data, eid: `synthetic-${task}-${type}-${seq}`, host, actor, guards, layer: 'L5', obj: task, epoch: guards[0]?.epoch ?? null, t: new Date(seq * 1000).toISOString(), stale: false, cost_usd: null });
const receipt = (body, id, result) => { const meta = { request_id: id, body_hash: fixtureHash(body) }; return { p3_request: meta, p3_receipt: { ...meta, result_ref: result } }; };
const observation = (put, gate, actual, sourceSha, transcriptRef) => { put(`${gate}Event`, actual); put(`${gate}Observation`, { performed_by: 'human', event_id: actual.eid, actor: actual.actor, source_sha: sourceSha, record_ref: transcriptRef }); return { gate, event: `${gate}Event`, observation: `${gate}Observation` }; };

export function fixturePublication(project, approvalId, seq, commit = oid, host = 'host') {
  const value = event('baseline.created', seq, { publication_id: approvalId, approval_eid: approvalId, content_hash: digest, target: 'towermd/B1', tag: 'towermd/B1', commit, request_body: { project_id: project } }, project, approvalId, 'process:towermd-baseline', [], host);
  value.layer = 'L1'; return value;
}
export function fixtureRecheck({ put, project, task, publication, sourceSha, transcriptRef, seq, host = 'host', running = null }) {
  const fact = { task: task.id, old_baseline: 'towermd/B0', new_baseline: 'towermd/B1', publication_id: publication.obj, reasons: ['synthetic approved module change'] };
  const requiredBody = { project_id: project, actor: 'process:towermd-baseline', host, run: null, action: { action: 'require_task_recheck', fact, source_commit: oid, created_eid: publication.eid } };
  const required = event('task.recheck_required', seq - 1, { schema: 1, fact, source_commit: oid, created: publication, request_body: requiredBody, ...receipt(requiredBody, `synthetic-required-${task.id}`, fact) }, project, task.id, requiredBody.actor, [], host);
  const taskBefore = { ...task, state: running ? 'running' : 'ready' };
  const binding = { task_id: task.id, target_baseline: 'towermd/B1', task_content_hash: fixtureHash(taskBefore), publication_ids: [publication.obj], running };
  const body = { project_id: project, actor: 'human:fixture', host, run: running?.run ?? null, action: { action: 'resolve_task_recheck', binding, comment: 'synthetic reviewed scope' } };
  const actual = event('task.recheck_resolved', seq, {}, project, task.id, body.actor, running?.fences ?? [], host);
  const result = { request_id: `synthetic-recheck-${task.id}`, binding, actor: actual.actor, resolved_eid: actual.eid };
  actual.data = { schema: 1, source_commit: oid, source_seq: seq - 1, subject: { binding, task_text: JSON.stringify(taskBefore), reasons: [fact] }, request_body: body, ...receipt(body, result.request_id, result) };
  put('recheckRequired', required); put('recheckPublication', publication);
  return { ...observation(put, 'recheck', actual, sourceSha, transcriptRef), publication: 'recheckPublication', required: ['recheckRequired'] };
}

export function fixtureProposal({ put, project, task, run, fences, approval, seq }) {
  const approved = approval.data.approval.binding;
  approved.object_id = 'synthetic-proposal'; approved.draft_revision = oid; approved.content_hash = digest;
  approval.obj = approved.object_id; approval.layer = 'L1';
  if (typeof approval.data.approval.comment !== 'string') approval.data.approval.comment = 'synthetic reviewed proposal';
  const reviewBody = { project_id: project, host: run.host, actor: approval.actor, run: null, action: { kind: 'review_baseline', binding: approved, verdict: 'approved', comment: approval.data.approval.comment } };
  Object.assign(approval.data, { request_body: reviewBody, ...receipt(reviewBody, `synthetic-review-${task.id}`, clone(approval.data.approval)) });
  const input = { source: { kind: 'draft', id: approved.object_id, revision: 'e'.repeat(40) }, reason: 'synthetic Rename ledger', recommendation: 'synthetic approved rename', alternatives: [{ name: 'retain', consequences: 'retain prior label' }], affected_intents: [], affected_modules: ['ledger'], affected_interfaces: [], affected_tasks: [], affected_paths: ['ledger/**'] };
  const binding = { task: task.id, run: run.id, baseline: 'towermd/B0', fences };
  const result = { draft: { draft_id: approved.object_id, revision: oid, reference: `refs/towermd/drafts/${approved.object_id}`, content_hash: digest, delivery: { state: 'local_saved' } }, binding, carrier: { schema: 1, input, impact: { intents: [], modules: ['ledger'], interfaces: [], acceptances: [], tasks: [], paths: ['ledger/**'] }, baseline: binding.baseline, main_revision: oid, run: run.id, payload_hash: digest, binding } };
  const body = { project_id: project, host: run.host, actor: 'process:towermd-cli', run: null, action: { kind: 'propose', input } };
  const actual = event('proposal.created', seq, { result, request_body: body, ...receipt(body, `synthetic-propose-${task.id}`, result) }, project, approved.object_id, body.actor, fences, run.host);
  actual.layer = 'L4'; put('proposal', actual); return 'proposal';
}

export function fixtureInterject({ put, project, task, run, claim, frozen, sourceSha, transcriptRef, seq }) {
  const scope = { kind: 'task', task: task.id }, text = 'synthetic original checkpoint instruction';
  const body = { project_id: project, host: run.host, actor: 'human:fixture', run: null, action: { action: 'interject', scope, text } };
  const meta = { request_id: `synthetic-prompt-${task.id}`, body_hash: fixtureHash(body) }, holdId = `H-${meta.request_id}`;
  const recipient = { run: run.id, task: task.id, host: run.host, worktree: run.worktree, branch: run.branch, fences: run.leases };
  const hold = { type: 'Hold', id: holdId, body: { kind: 'prompt', scope, text }, recipients: [recipient], opened_ms: seq * 1000, opened_seq: 0, reply: null, waits: [], delivered: {}, attention: false };
  const input = event('human.prompt', seq, { schema: 1, hold, scope, text, produced_action: body.action, request_body: body, p3_request: meta, p3_receipt: null, prompt: null, pause: null, resume_request: null, claims: [claim], source_barrier: 0 }, project, holdId, body.actor, [], run.host);
  const opened = event('hold.opened', seq + 1, { ...clone(input.data), prompt: input, p3_receipt: { ...meta, result_ref: { hold: holdId, opening_eid: `synthetic-${holdId}-hold.opened-${seq + 1}`, pause_revision: null, runs: [run.id] } } }, project, holdId, body.actor, [], run.host);
  const message = { id: `${holdId}/prompt/${run.id}`, hold: holdId, run: run.id, text, kind: 'prompt', actionable: true };
  const groupKey = `delivery:${fixtureHash([holdId, run.id])}`, group = { hold: holdId, recipient_run: run.id, opening: opened, source: opened, message_ids: [message.id] };
  const deliveryBody = { project_id: project, host: run.host, actor: 'process:towermd-cli', run: null, action: { directory: run.worktree, common_dir: frozen.common_dir, branch: run.branch, recipients: [recipient], groups: [group] } };
  const deliveredHold = { ...clone(hold), opened_seq: opened.seq, delivered: { [run.id]: [message.id] } };
  const delivered = event('prompt.delivered', seq + 2, { hold: deliveredHold, recipient_run: run.id, message_ids: [message.id], group_key: groupKey, request_body: deliveryBody, ...receipt(deliveryBody, `synthetic-delivery-${task.id}`, { group_key: groupKey, message_ids: [message.id] }) }, project, holdId, deliveryBody.actor, [], run.host);
  const stdoutRef = `synthetic:${task.id}:status:stdout`, stderrRef = `synthetic:${task.id}:status:stderr`;
  put('promptOpening', opened); put('promptDelivered', delivered);
  put('promptStatusStdout', JSON.stringify({ towermd_message: message }) + '\n' + JSON.stringify({ project_id: project, seq: delivered.seq, runs: [run], tasks: [task] }) + '\n', stdoutRef, 'text');
  put('promptStatusStderr', '', stderrRef, 'text');
  put('promptStatus', { argv: ['status'], cwd: run.worktree, exit_code: 0, stdout_ref: stdoutRef, stderr_ref: stderrRef });
  return { ...observation(put, 'interject', input, sourceSha, transcriptRef), opening: 'promptOpening', delivered: 'promptDelivered', checkpoint: 'promptStatus' };
}

export function fixtureHumanRoles(row) {
  return [row.event, row.observation, ...(row.publication ? [row.publication] : []), ...(row.proposal ? [row.proposal] : []), ...(row.required ?? []), ...(row.gate === 'interject' ? [row.opening, row.delivered, row.checkpoint, 'promptStatusStdout', 'promptStatusStderr'] : [])];
}
