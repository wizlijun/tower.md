// Fixed P7 captured-role identity checks. No filesystem, process or product API.
import { createHash } from 'node:crypto';
import { inspectCaptured, cliPayload } from '../acceptance/collect.mjs';

const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const text = value => typeof value === 'string' && value.trim().length > 0;
const digest = value => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);
const oid = value => typeof value === 'string' && /^[a-f0-9]{40}$/.test(value);
const canonical = value => JSON.stringify(value, function (_, item) { return item && typeof item === 'object' && !Array.isArray(item) ? Object.fromEntries(Object.keys(item).sort().map(key => [key,item[key]])) : item; });
const same = (a,b) => canonical(a) === canonical(b);
const need = (value, field) => { if (!value) throw Error(`release acceptance raw ${field}`); };
const equal = (a, b, field) => need(same(a, b), field);
const json = (bytes, field) => { try { return JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(bytes)); } catch { throw Error(`release acceptance raw ${field} JSON`); } };
const accepted = event => Number.isSafeInteger(event?.seq) && event.seq > 0 && text(event.eid) && event.stale === false && event.legacy !== true && Number.isFinite(Date.parse(event.t));

export function checkHumanBindings(checks, read, { identity, input, project, implementations }) {
  for (const key of ['humanRecovery', 'humanBaselineApproval']) {
    const h = checks[key], event = json(read(h.event), `${key}.event`), binding = json(read(h.binding), `${key}.binding`), implementation = json(read(h.implementation), `${key}.implementation`);
    need(accepted(event) && event.eid === h.eventId && event.actor === h.actor && /^human:[^\s:/]+$/.test(event.actor), `${key}.event identity`);
    need(oid(implementation.commit) && oid(implementation.tree) && implementation.tree === h.implementationTree && [identity.commit, input.sha].includes(implementation.commit) && (implementation.commit !== identity.commit || implementation.tree === identity.tree) && implementations.some(actual => same(actual, implementation)) && Array.isArray(implementation.sources) && Array.isArray(implementation.commands) && implementation.commands.length > 0, `${key}.implementation identity`);
    if (key === 'humanRecovery') {
      const request = binding.request;
      need(event.type === 'hub.rebuild.approved' && event.layer === 'L3' && event.obj === 'main' && event.guards?.length === 0 && binding.human_actor === event.actor && binding.can_confirm === true && request?.project_id === project && request.status === 'awaiting_app_confirmation' && event.data?.request_id === request.request_id && event.data?.source_commit === request.source_commit && oid(request.source_commit) && Number.isSafeInteger(request.requested_at) && Number.isSafeInteger(request.expires_at) && Date.parse(event.t) >= request.requested_at && Date.parse(event.t) <= request.expires_at, `${key}.RecoveryView binding`);
    } else {
      const approval = event.data?.approval;
      need(event.type === 'baseline.approved' && event.layer === 'L1' && binding.human_actor === event.actor && text(binding.request_id) && binding.request_id === event.data?.p3_request?.request_id && typeof binding.comment === 'string' && binding.verdict === 'approved' && binding.comment === approval?.comment && event.data?.request_body?.actor === event.actor && event.data?.request_body?.project_id === project && approval?.actor === event.actor && approval?.approval_eid === event.eid && approval?.verdict === binding.verdict && approval?.binding?.project_id === project && approval?.binding?.target_baseline === 'towermd/B0' && text(approval?.binding?.object_id) && digest(approval?.binding?.content_hash) && oid(approval?.binding?.draft_revision) && event.obj === approval?.binding?.object_id, `${key}.PreparedReview identity`);
      equal(binding.binding, approval.binding, `${key}.PreparedReview binding`);
      equal(event.data?.p3_receipt?.result_ref, approval, `${key}.original receipt`);
    }
  }
}

// Everything is a controlled source by default. Only these final audit/log
// documents may differ; a new or unknown path is compared as runtime input.
function auditOnly(path) {
  return path.startsWith('docs/verification/') || ['tasks/todo.md','tasks/lessons.md','CHANGELOG.md','CHANGELOG.zh-CN.md'].includes(path);
}
function checkReuse(receipt, read, { identity, input, sources, builds, sourceSnapshot, packageManifest }) {
  if (input.sha === identity.commit) return;
  need(sourceSnapshot?.identity?.commit === input.sha && oid(sourceSnapshot.identity.tree), 'actual C source identity');
  const reuse = json(read(receipt.p7.reuse), 'p7.reuse');
  need(reuse.format === 'towermd-p7-source-reuse-v1', 'reuse format');
  equal(reuse.source, sourceSnapshot.identity, 'reuse actual C identity');
  equal(reuse.release, {commit:identity.commit,tree:identity.tree}, 'reuse actual R identity');
  const before = new Map(sourceSnapshot.sources.map(source => [source.path,source])), after = new Map(sources.map(source => [source.path,source]));
  const changed = [...new Set([...before.keys(),...after.keys()])].filter(path => !same(before.get(path),after.get(path))).sort();
  equal(reuse.changed_paths, changed, 'reuse actual changed paths');
  need(changed.every(auditOnly), 'reuse runtime/source difference; rerun affected acceptance at R');
  need(reuse.review !== receipt.p7.reuse && read(reuse.review).length > 0, 'reuse actual review material');
  const releaseInput = json(read(reuse.release_input), 'reuse.release_input'), build = json(read(reuse.release_build), 'reuse.release_build');
  need(releaseInput.sha === identity.commit && builds.some(actual => same(actual,build)), 'reuse actual R build/input');
  need(packageManifest.targets.find(target => target.target === 'aarch64-apple-darwin')?.cliSha256 === releaseInput.cli_sha256, 'reuse exact staged/bundled R CLI');
  need(hash(read(reuse.release_cli)) === releaseInput.cli_sha256 && hash(read(reuse.release_app)) === releaseInput.app_sha256, 'reuse new package binary bytes');
  for (const [role,digest] of Object.entries(input.skill_hashes)) equal(releaseInput.skill_hashes?.[role],digest,'reuse unchanged native skill bytes');
  const kinds = ['installation','cliContracts','nativeLoading','endToEnd'];
  equal(Object.keys(reuse.reruns ?? {}).sort(),[...kinds].sort(),'reuse required new-package checks');
  for (const kind of kinds) {
    const observation = json(read(reuse.reruns[kind]), `reuse.${kind}`);
    need(observation.kind === kind && observation.source_sha === identity.commit && observation.cli_sha256 === releaseInput.cli_sha256 && observation.app_sha256 === releaseInput.app_sha256 && Array.isArray(observation.refs) && observation.refs.length > 0 && new Set(observation.refs).size === observation.refs.length && observation.refs.every(id => read(id).length > 0), `reuse.${kind} actual source/capture`);
    if (kind === 'nativeLoading') equal(observation.harnessClasses?.slice().sort(),['claudeCode','codex'],'reuse both native harness loading observations');
    if (kind === 'installation') need(receipt.assets.some(asset => asset.name === observation.sourceAsset && asset.name === `tower.md-${identity.tag.slice(1)}-aarch64.dmg` && asset.sha256 === observation.sourceAssetSha256), 'reuse exact new DMG bytes');
  }
}

export function checkRawAcceptance(receipt, { identity, materials, sources, builds, sourceSnapshot, packageManifest }) {
  const read = id => {
    need(text(id) && materials.has(id) && receipt.materials?.[id]?.sha256 === hash(materials.get(id)), 'controlled material identity'); return materials.get(id);
  };
  const input = json(read(receipt.p7.input), 'p7.input'), projects = new Set(), implementations = [...builds], harnesses = new Map([['claudeCode',new Set()],['codex',new Set()]]);
  need(input.cli_version === `towermd ${identity.tag.slice(1)}`, 'actual CLI version for frozen runtime');
  checkReuse(receipt, read, { identity, input, sources, builds, sourceSnapshot, packageManifest });
  if (input.sha === identity.commit) {
    need(packageManifest.targets.find(target => target.target === 'aarch64-apple-darwin')?.cliSha256 === input.cli_sha256, 'actual staged/bundled frozen CLI bytes');
    for (const [role, digest] of Object.entries(input.skill_hashes ?? {})) {
      const path = `skills/skills/towermd-${role.slice(5).toLowerCase()}/SKILL.md`;
      need(sources.some(source => source.path === path && source.sha256 === digest), 'input frozen skill source');
    }
  }
  const b1Card = receipt.p7.tasks.find(card => card.card === 'T08');
  const b1Manifest = json(read(b1Card?.captureManifest), 'T08.captureManifest'), b1Row = b1Manifest.human?.find(row => row.gate === 'B1');
  const b1Publication = json(read(b1Card?.captured?.[b1Row?.publication]), 'T08.B1 publication');
  for (const card of receipt.p7.tasks) {
    const record = json(read(card.record), `${card.card}.record`), manifest = json(read(card.captureManifest), `${card.card}.captureManifest`), git = json(read(card.git), `${card.card}.git`);
    need(card.captured && manifest.materials && same(Object.keys(card.captured).sort(), Object.keys(manifest.materials).sort()), `${card.card}.fixed captured role set`);
    const captures = new Map();
    for (const role of Object.keys(manifest.materials)) {
      const bytes = read(card.captured[role]); need(hash(bytes) === manifest.materials[role].sha256, `${card.card}.${role} raw bytes`); captures.set(role, bytes);
    }
    for (const [role, material] of Object.entries({ input: receipt.p7.input, task: card.task, session: card.session, finish: card.evidence, proof: card.proof, trailers: card.trailers, mainTests: card.mainTests })) equal(card.captured[role], material, `${card.card}.${role} original role`);
    const bound = [...Object.entries(card.bindings).filter(([key]) => !['human','checkpoints','failures'].includes(key)).map(([,value]) => value), ...card.bindings.human, ...card.bindings.checkpoints, ...card.bindings.failures];
    for (const binding of bound) {
      const role = Object.keys(manifest.materials).find(role => manifest.materials[role].sourceRef === binding.sourceRef);
      need(role && hash(read(binding.material)) === hash(captures.get(role)), `${card.card}.sourceRef captured bytes`);
    }
    const prerequisite = Object.keys(manifest.materials).find(role => manifest.materials[role].sourceRef === input.prerequisite_receipt_ref);
    need(prerequisite, `${card.card}.actual prerequisite role`);
    const originalBuild = json(captures.get(prerequisite), `${card.card}.prerequisite`);
    if (input.sha !== identity.commit) {
      need(originalBuild.tree === sourceSnapshot.identity.tree, `${card.card}.actual C build tree`);
      equal(originalBuild.sources, sourceSnapshot.sources, `${card.card}.complete actual C build source`);
    } else {
      need(builds.some(actual => same(actual,originalBuild)), `${card.card}.actual frozen R build receipt`);
    }
    implementations.push(originalBuild);
    for (const row of manifest.human ?? []) if (row.gate === 'recheck') {
      equal(json(captures.get(row.publication), `${card.card}.recheck publication`), b1Publication, `${card.card}.same actual T08 B1 publication`);
    }
    const derived = inspectCaptured(manifest, captures, git, { task: card.card, worktree: record.binding.worktree, expectedSourceSha: record.source.sha });
    need(derived.result === 'pass' && derived.missing.length === 0, `${card.card}.incomplete mappings: ${derived.missing.join(',')}`);
    for (const field of ['task', 'attempt', 'source', 'session', 'binding', 'main', 'human_refs', 'checkpoint_refs', 'failure_refs']) equal(record[field], derived[field], `${card.card}.record ${field}`);
    const verification = value => Object.fromEntries(Object.entries(value).filter(([key]) => !['object_ref', 'object_sha256'].includes(key)));
    equal(verification(record.verification), verification(derived.verification), `${card.card}.record verification`);
    const task = json(read(card.task), `${card.card}.task`); equal(card.taskId, task.id, `${card.card}.product taskId`);
    equal(json(read(card.session), `${card.card}.session`), record.session, `${card.card}.session raw`);
    need(read(card.mainMessage).toString('utf8').trimEnd() === git.mainMessage.trimEnd(), `${card.card}.actual main message`);
    // Only the two already captured CLI payload roles are decoded; paths and
    // sourceRefs remain inert text, including optional original Git paths.
    need(JSON.parse(cliPayload(read(card.evidence))).evidence?.id === record.verification.evidence_id, `${card.card}.actual Evidence`);
    need(harnesses.has(card.harnessClass), `${card.card}.actual harness class`);
    harnesses.get(card.harnessClass).add(record.session.harness);
    projects.add(record.binding.project);
  }
  need([...harnesses.values()].every(names => names.size > 0) && [...harnesses.get('claudeCode')].every(name => !harnesses.get('codex').has(name)), 'distinct declared harnesses for actual classes');
  need(projects.size === 1, 'single actual P7 project');
  checkHumanBindings(receipt.checks, read, { identity, input, project: [...projects][0], implementations });
}
