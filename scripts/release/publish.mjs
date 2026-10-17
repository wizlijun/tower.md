import { assertRemoteReady, assertAssetSet, expectedAssets, verifySums, uploadAction, checkChangelogs, parseChangelog } from './core.mjs';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { appendFileSync, existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, writeFileSync } from 'node:fs';
import { basename, dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const targets = ['aarch64-apple-darwin', 'x86_64-apple-darwin'];
const locks = ['Cargo.lock', 'src-tauri/Cargo.lock', 'pnpm-lock.yaml'];
const readinessPath = 'docs/verification/2026-10-17-release-readiness.md';
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const fail = field => { throw new Error(`release ${field}`); };
const object = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const nonempty = value => typeof value === 'string' && value.trim().length > 0 && !/[\x00-\x1f\x7f]/.test(value);
const oid = value => typeof value === 'string' && /^[a-f0-9]{40}$/.test(value);
const digest = value => typeof value === 'string' && /^[a-f0-9]{64}$/.test(value);
const canonical = value => JSON.stringify(value, function (_, v) { return object(v) ? Object.fromEntries(Object.keys(v).sort().map(k => [k, v[k]])) : v; });
const equal = (a, b, field) => { if (canonical(a) !== canonical(b)) fail(field); };
const need = (value, field) => { if (!value) fail(field); };
const decode = bytes => { try { return new TextDecoder('utf-8', { fatal: true }).decode(bytes); } catch { fail('UTF-8'); } };
const json = (bytes, field) => { try { return JSON.parse(decode(Buffer.from(bytes))); } catch { fail(field); } };
const runCommand = (file, args, options) => spawnSync(file, args, { ...options, encoding: null, maxBuffer: 128 * 1024 * 1024 });

function parseOrigin(value) {
  if (!nonempty(value) || /[\s%?#]/.test(value)) fail('origin URL');
  const match = /^(?:git@github\.com:|ssh:\/\/git@github\.com\/)([A-Za-z0-9_-]+)\/([A-Za-z0-9_.-]+)$/.exec(value);
  if (!match) fail('origin URL');
  const owner = match[1], repo = match[2].replace(/\.git$/, '');
  if (!repo || repo === '.' || repo === '..') fail('origin URL');
  return `${owner}/${repo}`;
}
function parseRefs(raw, allowed) {
  const refs = new Map();
  if (raw === '') return refs;
  for (const row of raw.replace(/\n$/, '').split('\n')) {
    const m = /^([a-f0-9]{40})\t([^\s]+)$/.exec(row);
    if (!m || !allowed.includes(m[2]) || refs.has(m[2])) fail('remote refs');
    refs.set(m[2], m[1]);
  }
  return refs;
}
function anonymousEnv(env = process.env) {
  const out = { PATH: '/usr/bin:/bin:/usr/sbin:/sbin' };
  for (const key of ['HOME', 'TMPDIR', 'LANG', 'LC_ALL']) if (env[key] !== undefined) out[key] = env[key];
  return out;
}
function checkUrl(value, repo, tag, name) {
  const expected = `https://github.com/${repo}/releases/download/${tag}/${name}`;
  if (value !== expected) fail('asset URL');
  return value;
}
function regular(path, field = 'file') {
  try {
    if (realpathSync(dirname(path)) !== dirname(path) || !lstatSync(path).isFile()) fail(field);
    return readFileSync(path);
  } catch { fail(field); }
}
function directory(path, field = 'directory') {
  try { if (realpathSync(path) !== path || !lstatSync(path).isDirectory()) fail(field); } catch { fail(field); }
}
function checkCommand(c, field) {
  need(object(c) && nonempty(c.file) && Array.isArray(c.args) && c.args.every(a => typeof a === 'string') && c.failed === false &&
    Number.isInteger(c.status) && digest(c.stdoutSha256) && digest(c.stderrSha256) &&
    Number.isFinite(Date.parse(c.started)) && Number.isFinite(Date.parse(c.finished)) && Date.parse(c.finished) >= Date.parse(c.started), field);
}
function checkProvenance(before, after, commands, manifest, identity, sources) {
  equal(before, after, 'provenance before/after drift');
  for (const k of ['tag', 'commit', 'tree', 'tagObject']) equal(before?.[k], identity[k], `provenance ${k}`);
  equal(before.sources, sources, 'provenance sources');
  need(Array.isArray(before.builds) && before.builds.length === 2 && new Set(before.builds.map(b => b.target)).size === 2, 'provenance builds');
  const lockfiles = Object.fromEntries(locks.map(path => [path, sources.find(s => s.path === path)?.sha256]));
  need(Object.values(lockfiles).every(digest), 'provenance lock sources'); equal(manifest.lockfiles, lockfiles, 'manifest lockfiles');
  equal(Object.keys(manifest.tools ?? {}).sort(), [...targets].sort(), 'manifest tools targets');
  for (const build of before.builds) {
    need(targets.includes(build.target), 'build target');
    for (const k of ['tag', 'commit', 'tree', 'tagObject']) equal(build[k], identity[k], `build ${k}`);
    equal(build.sources, sources, 'build sources'); equal(build.lockfiles, lockfiles, 'build lockfiles');
    need(object(build.tools) && ['node','pnpm','rustc','cargo','tauri','sdk','xcode'].every(k => nonempty(build.tools[k])), 'build tools');
    equal(build.tools, manifest.tools[build.target], 'manifest tools');
    need(isAbsolute(build.app ?? '') && isAbsolute(build.dmg ?? '') && build.app.endsWith(`/target/${build.target}/release/bundle/macos/Tower.md.app`) &&
      dirname(build.dmg) === resolve(build.app, '../../dmg') && build.dmg.endsWith('.dmg'), 'build paths');
    const cs = build.commands; need(Array.isArray(cs) && cs.length > 0, 'build commands');
    cs.forEach(c => { checkCommand(c, 'build command'); need(c.status === 0, 'build command status'); });
    const find = (predicate, field) => { const found = cs.map((c,i) => predicate(c) ? i : -1).filter(i => i !== -1); need(found.length === 1, field); return found[0]; };
    const cargo = find(c => basename(c.file) === 'cargo' && canonical(c.args) === canonical(['build','--locked','-p','towermd','--release','--target',build.target]), 'build cargo command');
    const metadata = find(c => c.file === cs[cargo].file && canonical(c.args) === canonical(['metadata','--locked','--no-deps','--format-version','1']), 'build metadata command');
    const lipo = find(c => c.file === 'lipo' && c.args.length === 2 && c.args[0] === '-archs' && isAbsolute(c.args[1]) && c.args[1].endsWith(`/src-tauri/binaries/towermd-${build.target}`), 'build staged CLI command');
    const script = join(resolve(cs[lipo].args[1], '../../..'), 'scripts/project-pnpm.mjs');
    const frontend = find(c => c.file === process.execPath && canonical(c.args) === canonical([script,'build']), 'build frontend command');
    const tauri = find(c => c.file === process.execPath && canonical(c.args) === canonical([script,'tauri','build','--target',build.target,'--bundles','app,dmg','--config','src-tauri/tauri.unsigned.conf.json','--','--locked']), 'build Tauri command');
    need(cargo < metadata && metadata < lipo && lipo < frontend && frontend < tauri, 'build command source chain');
  }
  need(Array.isArray(commands) && commands.length > 0, 'assembly commands');
  const policies = new Set();
  for (const c of commands) {
    checkCommand(c, 'assembly command');
    if (c.file !== 'spctl') { need(c.status === 0, 'assembly command status'); continue; }
    const build = before.builds.find(b => canonical(c.args) === canonical(['--assess','--type','execute','--verbose=4',b.app]));
    need(build && !policies.has(build.target), 'assembly spctl app'); policies.add(build.target);
    const g = manifest.targets?.find(t => t.target === build.target)?.gatekeeper;
    need(g && ((c.status === 3 && g.result === 'rejected') || (c.status === 0 && g.result === 'accepted')) && g.status === c.status && digest(g.outputSha256), 'assembly gatekeeper');
  }
  need(policies.size === 2, 'assembly gatekeeper targets');
}
function checkAcceptance(receipt, { identity, assets, readiness, materials }) {
  need(object(receipt) && receipt.format === 'towermd-release-acceptance-v1' && receipt.recordedBy === 'process:towermd-release' && Number.isFinite(Date.parse(receipt.recordedAt)), 'acceptance format/recorder');
  equal(receipt.identity, identity, 'acceptance identity');
  need(Array.isArray(receipt.assets), 'acceptance assets'); assertAssetSet(receipt.assets.map(a=>a?.name),identity.tag.slice(1));
  equal([...receipt.assets].sort((a,b)=>a.name.localeCompare(b.name)), [...assets].sort((a,b)=>a.name.localeCompare(b.name)), 'acceptance assets');
  need(object(receipt.materials) && materials instanceof Map, 'acceptance materials');
  const use = (id, field='material') => {
    need(nonempty(id) && Object.hasOwn(receipt.materials, id) && materials.has(id), `acceptance material role: ${field}`);
    need(digest(receipt.materials[id]?.sha256) && hash(materials.get(id)) === receipt.materials[id].sha256, 'acceptance material hash');
    return materials.get(id);
  };
  for (const id of Object.keys(receipt.materials)) use(id);
  for (const k of ['path','blob','sha256']) equal(receipt.readiness?.[k], readiness[k], `acceptance readiness ${k}`);
  use(receipt.readiness.review,'readiness.review');
  const refs = item => { need(Array.isArray(item?.refs) && item.refs.length > 0 && new Set(item.refs).size === item.refs.length, 'acceptance observation refs'); item.refs.forEach(use); };
  const p7 = receipt.p7; need(object(p7), 'acceptance p7'); use(p7.input,'p7.input');
  const reuse = () => { use(p7.reuse,'p7.reuse'); };
  if (p7.reuse !== undefined) reuse();
  need(Array.isArray(p7.tasks) && p7.tasks.length === 10, 'acceptance ten cards');
  const classes = ['claudeCode','codex','claudeCode','codex','claudeCode','codex','claudeCode','codex','claudeCode','codex'];
  const cards = new Set(), tasks = new Set(), runs = new Set(), evidence = new Set(), trees = new Set(), groups = new Map(), sourceRefs = new Map();
  const unique = (set, value, field) => { need(nonempty(value) && !set.has(value), field); set.add(value); };
  for (const card of p7.tasks) {
    const index = Array.from({length:10}, (_,i) => `T${String(i+1).padStart(2,'0')}`).indexOf(card.card);
    need(index !== -1 && card.harnessClass === classes[index], 'acceptance card harness class');
    unique(cards, card.card, 'acceptance duplicate card'); unique(tasks, card.taskId, 'acceptance duplicate taskId');
    for (const role of ['task','record','session','git','evidence','proof','trailers','mainMessage','mainTests']) use(card[role],`p7.${card.card}.${role}`);
    const r = json(use(card.record), 'acceptance record JSON');
    need(r.task === card.card && r.result === 'pass' && Number.isInteger(r.attempt) && r.attempt > 0 && Array.isArray(r.missing) && r.missing.length === 0, 'acceptance record task/result');
    const s = r.session, b = r.binding, v = r.verification, m = r.main;
    need(object(s) && s.kind === 'human_opened' && ['harness','version','id','transcript_ref'].every(k => nonempty(s[k])) && s.skill_loaded === 'self_report' &&
      (s.cost_usd === null || (Number.isFinite(s.cost_usd) && s.cost_usd >= 0)), 'acceptance session fields');
    need(object(b) && ['project','run','host','worktree','branch','module'].every(k => nonempty(b[k])) && Array.isArray(b.fences) && b.fences.length > 0, 'acceptance run binding');
    unique(runs, b.run, 'acceptance duplicate run'); trees.add(b.worktree);
    const group = index === 9 ? 1 : index % 3, groupValue = {worktree:b.worktree,module:b.module};
    if (groups.has(group)) equal(groups.get(group),groupValue,'acceptance worktree group'); else groups.set(group,groupValue);
    need(object(r.source) && oid(r.source.sha) && (r.source.tag === null || nonempty(r.source.tag)) && digest(r.source.cli_sha256) && digest(r.source.app_sha256) &&
      object(r.source.skill_hashes) && Object.keys(r.source.skill_hashes).length > 0 && Object.values(r.source.skill_hashes).every(digest), 'acceptance source');
    if (r.source.sha !== identity.commit) reuse();
    need(object(v) && oid(v.commit) && oid(v.tree) && nonempty(v.captured_source_ref) && (v.object_ref === null || nonempty(v.object_ref)) &&
      (v.object_ref === null || digest(v.object_sha256)) && digest(v.commands_hash) && digest(v.log_sha256) && nonempty(v.log_ref) && nonempty(v.result) &&
      Number.isFinite(Date.parse(v.entered_at)) && Number.isFinite(Date.parse(v.deadline)), 'acceptance verification fields');
    unique(evidence, v.evidence_id, 'acceptance duplicate evidence');
    need(object(m) && nonempty(m.authority_ref) && oid(m.commit) && oid(m.tree) && object(m.trailers) && Object.keys(m.trailers).length > 0 &&
      nonempty(m.proof_ref) && nonempty(m.merge_done_eid) && m.task_state === 'merged' && m.run_state === 'ended' && Number.isInteger(m.test_count) && m.test_count > 0 && m.test_exit === 0 && nonempty(m.test_log_ref), 'acceptance main fields');
    const fixed = { capturedSource: [v.captured_source_ref,card.evidence], proof: [m.proof_ref,card.proof], mainTests: [m.test_log_ref,card.mainTests], transcript: [s.transcript_ref], verificationLog: [v.log_ref] };
    const bindings = card.bindings;
    need(object(bindings), 'acceptance sourceRef bindings');
    equal(Object.keys(bindings).sort(), [...Object.keys(fixed),'human','checkpoints','failures'].sort(), 'acceptance fixed binding set');
    const binding = (item, ref, role) => { need(object(item) && item.sourceRef === ref && (role === undefined || item.material === role), 'acceptance sourceRef/role'); use(item.material);
      if (sourceRefs.has(ref)) need(sourceRefs.get(ref) === item.material, 'acceptance contradictory sourceRef'); else sourceRefs.set(ref,item.material);
    };
    for (const [key,[ref,role]] of Object.entries(fixed)) binding(bindings[key], ref, role);
    need(hash(use(bindings.verificationLog.material)) === v.log_sha256, 'acceptance verification log hash');
    for (const [key,field] of [['human','human_refs'],['checkpoints','checkpoint_refs'],['failures','failure_refs']]) {
      need(Array.isArray(r[field]) && Array.isArray(bindings[key]) && r[field].length === bindings[key].length, 'acceptance reference list');
      r[field].forEach((ref,i) => { need(nonempty(ref), 'acceptance original sourceRef'); binding(bindings[key][i], ref); });
    }
  }
  need(trees.size === 3, 'acceptance three worktrees');
  need(Array.isArray(receipt.architectures) && receipt.architectures.length === 2, 'acceptance architectures');
  equal(receipt.architectures.map(a => a.target).sort(), [...targets].sort(), 'acceptance architecture targets');
  for (const a of receipt.architectures) { need(['static','rosetta','device'].includes(a.level) && (a.target !== targets[0] || a.level === 'device'), 'acceptance architecture level'); refs(a); }
  const checks = receipt.checks, keys = ['installation','finderLaunch','minimalEnvironment','runtimeClosure','daemonLifetime','reopen','archiveIntegrity','humanRecovery','humanBaselineApproval'];
  equal(Object.keys(checks ?? {}).sort(), keys.sort(), 'acceptance checks'); keys.forEach(k => refs(checks[k]));
  need(checks.installation.sourceAsset === `tower.md-${identity.tag.slice(1)}-aarch64.dmg` && digest(checks.installation.installedTreeSha256), 'acceptance installation');
  const daemon = checks.daemonLifetime;
  need(Number.isSafeInteger(daemon.before?.pid) && daemon.before.pid > 0 && nonempty(daemon.before.instance), 'acceptance daemon identity'); equal(daemon.before, daemon.after, 'acceptance daemon lifetime');
  need(checks.humanRecovery.eventId !== checks.humanBaselineApproval.eventId, 'acceptance distinct human events');
  for (const key of ['humanRecovery','humanBaselineApproval']) {
    const h = checks[key]; need(nonempty(h.eventId) && /^human:[^\s:]+$/.test(h.actor) && oid(h.implementationTree), `acceptance ${key} identity`);
    for (const role of ['event','binding','implementation']) use(h[role]);
    if (h.implementationTree !== identity.tree) { reuse(); need(h.reuse === p7.reuse, `acceptance ${key} reuse`); use(h.reuse); }
  }
  // This validates only already specified record fields and captured byte roles.
  // Raw P7 input/task/session/git/Evidence/proof and App event/binding identity
  // mappings do not yet exist. publishFrozen must refuse until those are wired.
}

function readLocal({ tag, output }, git) {
  need(typeof tag === 'string' && /^v\d+\.\d+\.\d+$/.test(tag) && tag.trim() === tag, 'tag');
  need(isAbsolute(output ?? '') && output === resolve(output), 'absolute output'); directory(output, 'asset directory');
  const logs = output + '.logs'; directory(logs, 'provenance directory');
  assertAssetSet(readdirSync(output), tag.slice(1));
  const snapshot = {}, capture = (path, field) => { const bytes = regular(path, field); snapshot[path] = { size: bytes.length, sha256: hash(bytes) }; return bytes; };
  const files = new Map(expectedAssets(tag.slice(1)).map(name => [name,capture(join(output,name),'asset file')]));
  verifySums(decode(files.get('SHA256SUMS')), new Map([...files].filter(([n]) => n !== 'SHA256SUMS')));
  const assets = [...files].map(([name,bytes]) => ({name,size:bytes.length,sha256:hash(bytes)}));
  const value = args => decode(Buffer.from(git(args))).trim();
  need(value(['cat-file','-t',`refs/tags/${tag}`]) === 'tag', 'annotated tag required');
  const identity = { tag, tagObject:value(['rev-parse','--verify',`refs/tags/${tag}`]), commit:value(['rev-parse','--verify',`refs/tags/${tag}^{commit}`]), tree:value(['rev-parse','--verify',`refs/tags/${tag}^{tree}`]) };
  need([identity.tagObject,identity.commit,identity.tree].every(oid), 'local tag identity');
  need(value(['rev-parse','--verify','refs/heads/main']) === identity.commit, 'local main differs from R; scope review required');
  const manifest = json(files.get('BUILD-MANIFEST.json'),'manifest JSON');
  for (const k of ['tag','tagObject','commit','tree']) equal(manifest[k],identity[k],`manifest ${k}`);
  need(manifest.version === tag.slice(1), 'manifest version');
  need(Array.isArray(manifest.targets) && manifest.targets.length === 2, 'manifest targets');
  equal(manifest.targets.map(t=>t.target).sort(), [...targets].sort(), 'manifest targets');
  manifest.targets.forEach(t => need(t.sourceTree === identity.tree, 'manifest sourceTree'));
  const binaries = assets.filter(a => /(?:\.zip|\.dmg|\.tar\.gz)$/.test(a.name));
  need(Array.isArray(manifest.artifacts) && manifest.artifacts.length === 5, 'manifest artifacts');
  equal(manifest.artifacts.map(a=>a.name).sort(),binaries.map(a=>a.name).sort(),'manifest artifacts');
  for (const a of manifest.artifacts) {
    const b = binaries.find(b=>b.name===a.name); equal({name:a.name,size:a.size,sha256:a.sha256},b,'manifest artifact bytes');
    equal(a.target,a.name.includes('skills') ? null : a.name.includes('aarch64') ? targets[0] : targets[1],'manifest artifact target');
  }
  const sourceFiles = new Map(), sources = decode(Buffer.from(git(['ls-tree','-r','-z','--full-tree',identity.commit]))).split('\0').filter(Boolean).map(row => {
    const m = /^(100644|100755|120000) blob ([a-f0-9]{40})\t([\s\S]+)$/.exec(row);
    need(m && !/(?:^|\/)\.env(?:\.|$)|private[-_]?key|updater[-_]?key/i.test(m[3]),'frozen source entry');
    const [,mode,blob,path] = m, bytes = Buffer.from(git(['cat-file','blob',blob]));
    need(createHash('sha1').update(`blob ${bytes.length}\0`).update(bytes).digest('hex') === blob, 'frozen blob bytes');
    need(!sourceFiles.has(path), 'frozen duplicate source'); sourceFiles.set(path,bytes); return {path,mode,blob,sha256:hash(bytes)};
  });
  const read = path => { need(sourceFiles.has(path), `frozen ${path}`); return sourceFiles.get(path); };
  const readinessSource = sources.find(s=>s.path===readinessPath); need(readinessSource && readinessSource.mode !== '120000', 'frozen readiness');
  const readiness = {path:readinessPath,blob:readinessSource.blob,sha256:hash(read(readinessPath))};
  const en = decode(read('CHANGELOG.md')), zh = decode(read('CHANGELOG.zh-CN.md')); checkChangelogs(en,zh,'frozen',tag.slice(1));
  for (const [lang,log] of [['en',en],['zh-CN',zh]]) {
    const prefix = `# Tower.md ${tag.slice(1)}\n\n${parseChangelog(log,lang).versions[0].body.trim()}\n\n`, notes=decode(files.get(`RELEASE-NOTES.${lang}.md`));
    need(notes.startsWith(prefix) && notes.slice(prefix.length).trim().length > 0, `frozen notes ${lang}`);
  }
  const before=json(capture(join(logs,'inputs-before.json'),'provenance inputs-before'),'provenance inputs-before JSON');
  const after=json(capture(join(logs,'inputs-after.json'),'provenance inputs-after'),'provenance inputs-after JSON');
  const commandBytes=capture(join(logs,'commands.jsonl'),'provenance commands');
  const commands=decode(commandBytes).trimEnd().split('\n').map(row=>json(row,'provenance command JSON'));
  for (const name of ['stdout.log','stderr.log']) capture(join(logs,name),'provenance aggregate log');
  checkProvenance(before,after,commands,manifest,identity,sources);
  const receipt=json(capture(join(logs,'acceptance.json'),'acceptance.json'),'acceptance JSON');
  need(object(receipt.materials),'acceptance materials'); const materials=new Map();
  for (const [id,item] of Object.entries(receipt.materials)) {
    need(nonempty(id) && object(item) && nonempty(item.path) && !item.path.includes('\\'), 'acceptance material path');
    const parts=item.path.split('/');
    need(parts.length >= 2 && parts[0] === 'acceptance-evidence' && parts.every(p=>p && p!=='.' && p!=='..') && !isAbsolute(item.path),'acceptance material path');
    const bytes=capture(join(logs,...parts),`acceptance material file: ${id}`); need(digest(item.sha256) && hash(bytes)===item.sha256,`acceptance material hash: ${id}`); materials.set(id,bytes);
  }
  checkAcceptance(receipt,{identity,assets,readiness,materials});
  return {identity,assets,manifest,readiness,snapshot,expectedBody:decode(files.get('RELEASE-NOTES.en.md')).trimEnd()+'\n\n---\n\n'+decode(files.get('RELEASE-NOTES.zh-CN.md')).trimEnd()+'\n'};
}

export async function publishFrozen({ repo, tag, output, download }, run = runCommand) {
  need(typeof tag === 'string' && /^v\d+\.\d+\.\d+$/.test(tag) && tag.trim() === tag, 'tag');
  for (const [key,value] of [['output',output],['download',download]]) need(nonempty(value) && isAbsolute(value) && value === resolve(value), `absolute ${key}`);
  if (repo !== undefined) need(typeof repo === 'string' && /^[A-Za-z0-9_-]+\/[A-Za-z0-9_.-]+$/.test(repo) && !repo.endsWith('/.') && !repo.endsWith('/..'), 'repository');
  const git = args => { const result = run('git',args,{cwd:process.cwd(),shell:false}); need(!result?.error && result?.status === 0, 'git command'); return result.stdout ?? ''; };
  readLocal({tag,output},git);
  // There is no production mapping for these captured raw roles yet. Do not
  // permit any remote reads or mutations on the basis of a self-consistent JSON.
  fail('unwired P7 input/task/session/git/Evidence/proof and human event/binding/implementation identity mappings');
}

// Private until P7 raw role identities are wired at the public entry point.
// Tests exercise this state machine with a derived module and a fake runner.
async function publishRemote({ repo, tag, output, download, local }, run) {
  const root = realpathSync(process.cwd()), version = tag.slice(1), logs = download + '.logs';
  need(typeof repo === 'string' && /^[A-Za-z0-9_-]+\/[A-Za-z0-9_.-]+$/.test(repo) && !repo.endsWith('/.') && !repo.endsWith('/..'), 'repository');
  need(isAbsolute(download ?? '') && download === resolve(download) && realpathSync(dirname(download)) === dirname(download), 'new absolute download');
  const inside = (a,b) => { const r=relative(a,b); return r==='' || (!isAbsolute(r) && r!=='..' && !r.startsWith(`..${sep}`)); };
  need(![root,output,output+'.logs'].some(p=>inside(p,download)||inside(download,p)||inside(p,logs)||inside(logs,p)) && !existsSync(download) && !existsSync(logs), 'new separate download');
  mkdirSync(logs,{mode:0o700});
  const observations = [], anonymous = [];
  let publicObserved = false;
  const call = (file,args,options={}) => {
    const started=new Date().toISOString(); let result;
    try { result=run(file,args,{cwd:root,shell:false,...options}); } catch { result={status:null,error:true}; }
    const record={file,args:[...args],cwd:options.cwd??root,started,finished:new Date().toISOString(),status:result?.status??null,failed:!!result?.error,
      stdoutSha256:hash(result?.stdout??''),stderrSha256:hash(result?.stderr??'')};
    appendFileSync(join(logs,'commands.jsonl'),JSON.stringify(record)+'\n',{mode:0o600});return result;
  };
  const checked = (file,args,options) => { const result=call(file,args,options);need(!result?.error&&result?.status===0,`${basename(file)} command exit=${result?.status??'unknown'}`);return result.stdout??''; };
  const gitEnv=anonymousEnv(); if(process.env.SSH_AUTH_SOCK!==undefined) gitEnv.SSH_AUTH_SOCK=process.env.SSH_AUTH_SOCK;
  const git=args=>checked('git',args,{env:gitEnv});
  const freshLocal=()=>{ const now=readLocal({tag,output},git);equal(now,local,'local frozen inputs drift'); };
  const origin=()=>{
    const one=args=>{const raw=decode(Buffer.from(git(args)));const rows=raw.replace(/\n$/,'').split('\n');need(rows.length===1,'origin unique fetch/push');return parseOrigin(rows[0]);};
    const fetch=one(['remote','get-url','--all','origin']),push=one(['remote','get-url','--push','--all','origin']);
    need(fetch.toLowerCase()===push.toLowerCase()&&fetch.toLowerCase()===repo.toLowerCase(),'origin repository mismatch');
  };
  const remoteMain=()=>parseRefs(decode(Buffer.from(git(['ls-remote','origin','refs/heads/main']))),['refs/heads/main']).get('refs/heads/main');
  const remoteTag=()=>{
    const ref=`refs/tags/${tag}`, refs=parseRefs(decode(Buffer.from(git(['ls-remote','origin',ref,ref+'^{}']))),[ref,ref+'^{}']);
    if(refs.size===0)return false;
    need(refs.size===2&&refs.get(ref)===local.identity.tagObject&&refs.get(ref+'^{}')===local.identity.commit,'remote tag identity');return true;
  };
  const refs=()=>{origin();need(remoteMain()===local.identity.commit,'remote main drift');need(remoteTag(),'remote tag missing');};
  const guard=()=>{freshLocal();refs();};
  const same=(a,b)=>{
    for(const key of ['id','tagName','url','body'])equal(a[key],b[key],`remote release ${key} drift`);
    equal(a.assets,b.assets,'remote asset id/size/set drift');
  };
  const observe=()=>{
    const view=call('gh',['release','view',tag,'--repo',repo,'--json','tagName,isDraft,assets,url,body']);
    const api=call('gh',['api','--hostname','github.com','--method','GET','--include',`repos/${repo}/releases/tags/${tag}`]);
    need(!api?.error,'release API transport');
    const raw=decode(Buffer.from(api.stdout??'')), status=/^HTTP\/\d(?:\.\d)? ([0-9]{3})(?: [^\r\n]*)?\r?\n/.exec(raw);
    need(status,'release API HTTP status');
    if(status[1]==='404') {need(api.status===1&&!view?.error&&view?.status===1,'release API inconsistent 404');return null;}
    need(status[1]==='200'&&api.status===0,'release API HTTP error');
    need(!view?.error&&view?.status===0,'release view error');
    const boundary=/\r?\n\r?\n/.exec(raw);need(boundary,'release API HTTP body');
    const data=json(raw.slice(boundary.index+boundary[0].length),'release API JSON'),v=json(view.stdout??'','release view JSON');
    need(Number.isSafeInteger(data?.id)&&data.id>0&&typeof data.draft==='boolean'&&typeof data.body==='string','release API fields');
    need(data.tag_name===tag&&data.html_url===`https://github.com/${repo}/releases/tag/${tag}`,'release API tag/URL');
    equal({tagName:v.tagName,isDraft:v.isDraft,url:v.url,body:v.body},{tagName:data.tag_name,isDraft:data.draft,url:data.html_url,body:data.body},'release view/API identity');
    const pages=json(checked('gh',['api','--hostname','github.com','--method','GET','--paginate','--slurp',`repos/${repo}/releases/${data.id}/assets?per_page=100`]),'release assets JSON');
    need(Array.isArray(pages)&&pages.length>0&&pages.every(Array.isArray),'release asset pages');
    const ids=new Set(),names=new Set();
    const assets=pages.flat().map(a=>{
      need(object(a)&&expectedAssets(version).includes(a.name)&&!names.has(a.name),'remote asset set');names.add(a.name);
      need(Number.isSafeInteger(a.id)&&a.id>0&&!ids.has(a.id),'remote asset id');ids.add(a.id);
      need(Number.isSafeInteger(a.size)&&a.size>=0&&a.state==='uploaded','remote asset size/state');
      return {name:a.name,id:a.id,size:a.size,url:checkUrl(a.browser_download_url,repo,tag,a.name)};
    }).sort((a,b)=>a.name.localeCompare(b.name));
    const result={id:data.id,tagName:data.tag_name,isDraft:data.draft,url:data.html_url,body:data.body,assets};
    need(result.body===local.expectedBody,'release body mismatch');
    if(!result.isDraft) { publicObserved=true;assertRemoteReady(result,tag,local.expectedBody); }
    observations.push({...result,body:undefined,bodySha256:hash(result.body)});
    appendFileSync(join(logs,'observations.jsonl'),JSON.stringify(observations.at(-1))+'\n',{mode:0o600});return result;
  };
  const authenticate=release=>{
    for(const asset of release.assets) {
      const dir=mkdtempSync(join(logs,'authenticated-'));
      checked('gh',['release','download',tag,'--repo',repo,'--pattern',asset.name,'--dir',dir]);
      equal(readdirSync(dir),[asset.name],'authenticated asset set');const bytes=regular(join(dir,asset.name),'authenticated asset file');
      need(bytes.length===asset.size,'authenticated asset size');
      need(uploadAction(local.assets.find(a=>a.name===asset.name),{name:asset.name,size:bytes.length,sha256:hash(bytes)})==='reuse','authenticated asset bytes');
    }
  };
  try {
    freshLocal();origin();const main=remoteMain(),hasTag=remoteTag();
    if(main!==local.identity.commit) {
      if(main) {need(decode(Buffer.from(git(['cat-file','-t',main]))).trim()==='commit','remote main object unavailable');checked('git',['merge-base','--is-ancestor',main,local.identity.commit],{env:gitEnv});}
      freshLocal();origin();need(remoteMain()===main,'remote main drift');remoteTag();
      checked('git',['push','--no-follow-tags','origin','main'],{env:gitEnv});need(remoteMain()===local.identity.commit,'remote main push readback');
    }
    if(!hasTag) {freshLocal();origin();need(remoteMain()===local.identity.commit&&!remoteTag(),'remote tag changed before push');checked('git',['push','--no-follow-tags','origin',`refs/tags/${tag}`],{env:gitEnv});need(remoteTag(),'remote tag push readback');}
    refs();let current=observe();
    if(current===null) {
      guard();need(observe()===null,'release appeared before create');
      const notes=join(logs,'release-body.md');writeFileSync(notes,local.expectedBody,{flag:'wx',mode:0o600});
      checked('gh',['release','create',tag,'--repo',repo,'--verify-tag','--draft','--title',`Tower.md ${version}`,'--notes-file',notes]);
      current=observe();need(current?.isDraft,'draft creation readback');
    }
    authenticate(current);
    if(!current.isDraft) assertRemoteReady(current,tag,local.expectedBody);
    for(const name of expectedAssets(version)) if(!current.assets.some(a=>a.name===name)) {
      guard();const before=observe();need(before?.isDraft,'release became public before upload');same(current,before);
      checked('gh',['release','upload',tag,join(output,name),'--repo',repo]);
      const after=observe();need(after?.isDraft,'release became public during upload');
      same({...after,assets:after.assets.filter(a=>a.name!==name)},before);
      need(after.assets.length===before.assets.length+1&&after.assets.some(a=>a.name===name),'upload readback');current=after;
    }
    refs();const s1=observe();need(s1,'release missing S1');assertRemoteReady(s1,tag,local.expectedBody);authenticate(s1);
    guard();const s2=observe();need(s2,'release missing S2');assertRemoteReady(s2,tag,local.expectedBody);same(s1,s2);
    // A public observation never transitions back to editing, even if an external
    // actor subsequently makes the release a draft again.
    need(!publicObserved||!s2.isDraft,'public release became draft');
    if(s2.isDraft) {guard();checked('gh',['release','edit',tag,'--repo',repo,'--draft=false']);publicObserved=true;}
    refs();const s3=observe();need(s3&&!s3.isDraft,'release publish readback');assertRemoteReady(s3,tag,local.expectedBody);same(s2,s3);
    freshLocal();mkdirSync(download,{mode:0o700});const env=anonymousEnv();
    for(const asset of s3.assets) {
      checkUrl(asset.url,repo,tag,asset.name);const started=new Date().toISOString();
      const raw=checked('curl',['--disable','--fail','--location','--proto','=https','--proto-redir','=https','--silent','--show-error','--output',join(download,asset.name),'--write-out','%{http_code}\n%{url_effective}\n',asset.url],{cwd:download,env});
      const lines=decode(Buffer.from(raw)).split('\n');need(lines.length===3&&lines[2]===''&&lines[0]==='200','anonymous HTTP result');
      let effective;try {effective=new URL(lines[1]);}catch {fail('anonymous redirect URL');}
      need(effective.protocol==='https:'&&!effective.username&&!effective.password,'anonymous HTTPS redirect');
      const bytes=regular(join(download,asset.name),'anonymous asset file'),record={name:asset.name,size:bytes.length,sha256:hash(bytes)};
      equal(record,local.assets.find(a=>a.name===asset.name),'anonymous asset bytes');
      const item={...record,url:asset.url,effectiveUrl:effective.origin+effective.pathname,http:200,started,finished:new Date().toISOString()};anonymous.push(item);
      appendFileSync(join(logs,'anonymous.jsonl'),JSON.stringify(item)+'\n',{mode:0o600});
    }
    assertAssetSet(readdirSync(download),version);
    verifySums(decode(regular(join(download,'SHA256SUMS'))),new Map(expectedAssets(version).filter(n=>n!=='SHA256SUMS').map(n=>[n,regular(join(download,n))])));
    checked('shasum',['-a','256','-c','SHA256SUMS'],{cwd:download,env});
    guard();const final=observe();need(final&&!final.isDraft,'final public release');assertRemoteReady(final,tag,local.expectedBody);same(s3,final);
    for (const asset of local.assets) { const bytes=regular(join(download,asset.name),'anonymous final file'); equal({name:asset.name,size:bytes.length,sha256:hash(bytes)},asset,'anonymous asset bytes'); }
    const receipt={repo,identity:local.identity,url:final.url,assets:local.assets,readiness:local.readiness,inputs:local.snapshot,observations,anonymous};
    writeFileSync(join(logs,'receipt.json'),JSON.stringify(receipt,null,2)+'\n',{flag:'wx',mode:0o600});return {url:final.url,receipt};
  } catch(error) {
    const message=publicObserved?'已公开，匿名回读未完成': '发布未完成';
    writeFileSync(join(logs,'failure.json'),JSON.stringify({state:message,field:error.message,observations:observations.length,anonymous:anonymous.length})+'\n',{flag:'wx',mode:0o600});
    throw new Error(`${message}: ${error.message}`);
  }
}


export async function main(args, run) {
  const values = {}, allowed=['--repo','--tag','--output','--download'];
  for (let i=0;i<args.length;i+=2) {
    const key=args[i], value=args[i+1];
    need(allowed.includes(key) && !Object.hasOwn(values,key) && typeof value==='string' && value.length>0 && !value.startsWith('--'), 'argument'); values[key]=value;
  }
  need(['--tag','--output','--download'].every(k=>values[k]), 'missing argument');
  return publishFrozen({repo:values['--repo'],tag:values['--tag'],output:values['--output'],download:values['--download']},run);
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { await main(process.argv.slice(2)); }
  catch(error) { console.error(error.message); process.exitCode=1; }
}
