import test, { after } from 'node:test';
import assert from 'node:assert/strict';
import { assertRemoteReady, expectedAssets, verifySums } from './core.mjs';
const tag = 'v0.1.0', body = 'Frozen English.\n\n---\n\n冻结中文。\n';
const remote = () => ({ tagName: tag, body, assets: expectedAssets('0.1.0').map(name => ({ name })) });
test('remote gate accepts exactly the frozen tag, nine assets and complete notes', () => {
  assert.doesNotThrow(() => assertRemoteReady(remote(), tag, body));
});
for (const [name, change, pattern] of [
  ['additional updater', r => r.assets.push({ name: 'latest.json' }), /asset set/],
  ['additional signature', r => r.assets.push({ name: 'old-updater.sig' }), /asset set/],
  ['duplicate', r => r.assets.push(r.assets[0]), /asset set/],
  ['missing architecture', r => r.assets.pop(), /asset set/],
  ['wrong tag', r => r.tagName = 'v0.2.0', /remote tag/],
  ['incomplete notes', r => r.body = 'Frozen English.\n', /release body/],
]) test(`remote gate rejects ${name}`, () => {
  const r = remote(); change(r); assert.throws(() => assertRemoteReady(r, tag, body), pattern);
});

import { readFileSync, writeFileSync, mkdtempSync, mkdirSync, rmSync, realpathSync, symlinkSync, readdirSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { tmpdir } from 'node:os';
import { createHash } from 'node:crypto';
// Expose private units only in this derived test module. Production has no alternate
// entry point or approval/skip switch. No fixture claims to be P7 acceptance.
const source = readFileSync(new URL('./publish.mjs', import.meta.url), 'utf8')
  .replace("'./core.mjs'", JSON.stringify(new URL('./core.mjs', import.meta.url).href))
  .replace("'./acceptance.mjs'", JSON.stringify(new URL('./acceptance.mjs', import.meta.url).href))
  .replace('if (process.argv[1] &&', 'if (false &&');
const privateDir = realpathSync(mkdtempSync(join(tmpdir(), 'towermd-publish-private-test-')));
after(() => rmSync(privateDir, { recursive: true, force: true }));
const privateModule = join(privateDir, 'publish-private.mjs');
writeFileSync(privateModule, source + '\nexport {parseOrigin, parseRefs, anonymousEnv, checkUrl, checkProvenance, checkAcceptance, checkRawAcceptance, readLocal, publishRemote};');
const units = await import(privateModule);
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const sha = n => String(n).repeat(40), sum = n => String(n).repeat(64);
const identity = { tag, tagObject: sha('a'), commit: sha('b'), tree: sha('c') };
const repo = 'fixture/tower', targets = ['aarch64-apple-darwin', 'x86_64-apple-darwin'];
for (const origin of ['git@github.com:fixture/tower.git', 'git@github.com:fixture/tower', 'ssh://git@github.com/fixture/tower.git'])
  test(`origin parses fixed GitHub SSH ${origin}`, () => assert.equal(units.parseOrigin(origin), repo));
for (const origin of ['https://github.com/fixture/tower', 'git@alias:fixture/tower', 'ssh://user@github.com/fixture/tower',
  'ssh://git:secret@github.com/fixture/tower', 'ssh://git@github.com:22/fixture/tower', 'git@github.com:fixture/tower.git\n',
  'git@github.com:fixture/../tower', 'git@github.com:fixture/%74ower', 'git@github.com:fixture/tower?secret',
  'git@github.com:fixture/tower\ngit@github.com:other/tower'])
  test('origin rejects unsafe or ambiguous SSH without echoing source', () => {
    assert.throws(() => units.parseOrigin(origin), e => /origin/.test(e.message) && !e.message.includes('secret'));
  });
test('refs parse exact allowed OIDs and reject duplicate/unknown/malformed rows', () => {
  const ref = `refs/tags/${tag}`;
  assert.deepEqual([...units.parseRefs(`${sha('a')}\t${ref}\n${sha('b')}\t${ref}^{}\n`, [ref, ref + '^{}'])], [[ref, sha('a')], [ref + '^{}', sha('b')]]);
  for (const raw of [`${sha('a')}\t${ref}\n${sha('a')}\t${ref}\n`, `${sha('a')}\trefs/tags/latest\n`, `bad\t${ref}\n`])
    assert.throws(() => units.parseRefs(raw, [ref]), /remote refs/);
});
test('anonymous env never reads auth/proxy/CA getters', () => {
  const env = { HOME: '/tmp/fixture-home', TMPDIR: '/tmp', LANG: 'C', LC_ALL: 'C' };
  for (const key of ['GH_TOKEN', 'GITHUB_TOKEN', 'HTTP_PROXY', 'CURL_CA_BUNDLE', 'PATH'])
    Object.defineProperty(env, key, { get() { throw new Error('secret getter'); } });
  assert.deepEqual(units.anonymousEnv(env), { PATH: '/usr/bin:/bin:/usr/sbin:/sbin', ...env });
});
test('anonymous asset URL is exact repository/tag/name HTTPS with no credentials or query', () => {
  const url = `https://github.com/${repo}/releases/download/${tag}/SHA256SUMS`;
  assert.equal(units.checkUrl(url, repo, tag, 'SHA256SUMS'), url);
  for (const bad of [url + '?secret=x', url + '#x', url.replace('github.com', 'evil.example'), url.replace(tag, 'latest'),
    url.replace('/fixture/', '/other/'), url.replace('SHA256SUMS', '%53HA256SUMS'), url.replace('https://', 'https://secret@'), url.replace('/download/', '/download/../download/')])
    assert.throws(() => units.checkUrl(bad, repo, tag, 'SHA256SUMS'), /asset URL/);
});

const clone = value => structuredClone(value);
function command(file, args, status = 0) {
  return { file, args, status, failed: false, started: '2026-10-17T00:00:00.000Z', finished: '2026-10-17T00:00:00.001Z', stdoutSha256: sum('d'), stderrSha256: sum('e') };
}
function provenanceFixture() {
  const paths = ['Cargo.lock', 'src-tauri/Cargo.lock', 'pnpm-lock.yaml', 'scripts/project-pnpm.mjs', 'scripts/build-app.mjs', 'scripts/stage-cli.mjs', 'scripts/release/build.mjs', 'src-tauri/tauri.conf.json', 'src-tauri/tauri.unsigned.conf.json', 'src-tauri/src/lib.rs', 'index.html', 'vite.config.ts'];
  const sources = paths.map(path => ({ path, mode: '100644', blob: sha('d'), sha256: sum('e') }));
  const lockfiles = Object.fromEntries(paths.slice(0, 3).map(p => [p, sum('e')]));
  const tools = Object.fromEntries(['node','pnpm','rustc','cargo','tauri','sdk','xcode'].map(k => [k, 'synthetic 1']));
  const builds = targets.map((target, n) => {
    const root = `/synthetic/build-${n}`, script = `${root}/scripts/project-pnpm.mjs`;
    return { ...identity, target, app: `/synthetic/out-${n}/target/${target}/release/bundle/macos/Tower.md.app`, dmg: `/synthetic/out-${n}/target/${target}/release/bundle/dmg/Tower.md.dmg`, sources, tools, lockfiles,
      commands: [command('/synthetic/cargo', ['build','--locked','-p','towermd','--release','--target',target]),
        command('/synthetic/cargo', ['metadata','--locked','--no-deps','--format-version','1']),
        command('lipo', ['-archs', `${root}/src-tauri/binaries/towermd-${target}`]),
        command(process.execPath, [script,'build']),
        command(process.execPath, [script,'tauri','build','--target',target,'--bundles','app,dmg','--config','src-tauri/tauri.unsigned.conf.json','--','--locked'])] };
  });
  const manifest = { ...identity, lockfiles, tools: Object.fromEntries(builds.map(b => [b.target, b.tools])), targets: targets.map(target => ({ target, sourceTree: identity.tree, cliSha256:hash('synthetic cliBinary bytes'), gatekeeper: { status: 3, result: 'rejected', outputSha256: sum('f') } })) };
  const before = { ...identity, sources, builds }, after = clone(before);
  const commands = builds.map(b => command('spctl', ['--assess','--type','execute','--verbose=4', b.app], 3));
  return { before:clone(before), after, commands, manifest:clone(manifest), sources:clone(sources) };
}
function checkProvenance(f) { return units.checkProvenance(f.before, f.after, f.commands, f.manifest, identity, f.sources); }
test('actual provenance fields accept expected spctl rejection without inventing output boundaries', () => assert.doesNotThrow(() => checkProvenance(provenanceFixture())));
for (const [name, change] of [
  ['after drift', f => f.after.tree = sha('d')],
  ['wrong source', f => f.before.sources.pop()],
  ['target duplicate', f => f.before.builds[1].target = targets[0]],
  ['missing lock', f => delete f.before.builds[0].lockfiles['src-tauri/Cargo.lock']],
  ['tool missing', f => f.before.builds[0].tools.node = ''],
  ['failed build', f => f.before.builds[0].commands[0].status = 3],
  ['metadata after Tauri', f => { const a = f.before.builds[0].commands; a.push(a.splice(1, 1)[0]); }],
  ['missing frontend command', f => f.before.builds[0].commands.splice(3, 1)],
  ['invalid command digest', f => f.commands[0].stdoutSha256 = 'bad'],
  ['invalid time', f => f.commands[0].finished = 'bad'],
  ['command threw', f => f.commands[0].failed = true],
  ['unrelated nonzero', f => f.commands.push(command('git', ['show','HEAD'], 3))],
  ['spctl wrong app', f => f.commands[0].args[4] = '/other.app'],
  ['spctl gatekeeper mismatch', f => f.manifest.targets[0].gatekeeper.result = 'accepted'],
  ['spctl unexpected status', f => { f.commands[0].status = 4; f.manifest.targets[0].gatekeeper.status = 4; }],
]) test(`provenance rejects ${name}`, () => { const f = provenanceFixture(); change(f); if (name !== 'after drift') f.after=clone(f.before); assert.throws(() => checkProvenance(f), /release/); });

function acceptanceFixture(acceptedIdentity = identity, acceptedSources) {
  // Fully synthetic captured product DTOs exercise machine identity checks only;
  // they are never represented as real Tasks, harness sessions or human actions.
  const materials = new Map(), listing = {};
  const material = (id, value = `synthetic ${id}`) => { const bytes = Buffer.isBuffer(value) ? value : Buffer.from(typeof value === 'string' ? value : JSON.stringify(value)); listing[id] = { path: `acceptance-evidence/${id}.txt`, sha256: hash(bytes) }; materials.set(id, bytes); return id; };
  const assets = expectedAssets('0.1.0').map(name => ({ name, size: 1, sha256: sum('d') }));
  const receipt = { format: 'towermd-release-acceptance-v1', recordedAt: '2026-10-17T00:00:00.000Z', recordedBy: 'process:towermd-release', identity:acceptedIdentity, assets: clone(assets),
    readiness: { path: 'docs/verification/2026-10-17-release-readiness.md', blob: sha('d'), sha256: sum('e'), review: material('review') }, materials: listing,
    p7: { input: 'input', tasks: [] },
    architectures: targets.map((target, i) => ({ target, level: i ? 'static' : 'device', refs: [material(`arch${i}`)] })),
    checks: Object.fromEntries(['installation','finderLaunch','minimalEnvironment','runtimeClosure','daemonLifetime','reopen','archiveIntegrity','humanRecovery','humanBaselineApproval'].map(k => [k, { refs: [material(k)] }])) };
  receipt.checks.installation.sourceAsset = 'tower.md-0.1.0-aarch64.dmg'; receipt.checks.installation.installedTreeSha256 = sum('f');
  receipt.checks.daemonLifetime.before = { pid: 42, instance: 'instance' }; receipt.checks.daemonLifetime.after = { pid: 42, instance: 'instance' };
  const canonicalHash = value => hash(JSON.stringify(value, function (_, v) { return v && typeof v === 'object' && !Array.isArray(v) ? Object.fromEntries(Object.keys(v).sort().map(k => [k,v[k]])) : v; }));
  const skillRoles = ['skillInit','skillBaseline','skillClaim','skillAsk','skillPropose','skillFinish'];
  const shared = {}, skillHashes = {};
  for (const role of ['cliBinary','appBinary','cliVersion',...skillRoles]) { const value = role === 'cliVersion' ? 'towermd 0.1.0\n' : `synthetic ${role} bytes`; shared[role] = material(`global-${role}`, value); if (skillRoles.includes(role)) skillHashes[role] = hash(materials.get(shared[role])); }
  const implementation = { commit: acceptedIdentity.commit, tree: acceptedIdentity.tree, sources: acceptedSources ?? skillRoles.map(role => ({ path:`skills/skills/towermd-${role.slice(5).toLowerCase()}/SKILL.md`, sha256:skillHashes[role] })), commands:[{file:'cargo',status:0}] };
  shared.prerequisite = material('prerequisite', implementation);
  const input = { sha:acceptedIdentity.commit, tag:acceptedIdentity.tag ?? null, cli_sha256:hash(materials.get(shared.cliBinary)), app_sha256:hash(materials.get(shared.appBinary)), cli_version:'towermd 0.1.0', skill_hashes:skillHashes, prerequisite_receipt_ref:'prerequisite' };
  shared.input = material('input',input);
  const baseBinding = { project_id:'p', object_id:'draft-B0', content_hash:sum('a'), target_baseline:'towermd/B0', draft_revision:sha('d') };
  const b0 = { seq:1,eid:'humanBaselineApproval',type:'baseline.approved',actor:'human:fixture',host:'h',t:'1970-01-01T00:00:01.000Z',layer:'L1',obj:'draft-B0',guards:[],epoch:null,stale:false,cost_usd:null,
    data:{approval:{approval_eid:'humanBaselineApproval',binding:baseBinding,actor:'human:fixture',verdict:'approved',comment:'reviewed in synthetic mechanism fixture'},request_body:{project_id:'p',actor:'human:fixture'},p3_request:{request_id:'baseline-request'}} };
  b0.data.p3_receipt={result_ref:b0.data.approval};
  const published = (event, target) => ({seq:event.seq+1,eid:`published-${target}`,type:'baseline.created',actor:'process:towermd-baseline',host:'h',t:'1970-01-01T00:00:02.000Z',layer:'L1',obj:target,guards:[],epoch:null,stale:false,cost_usd:null,data:{approval_eid:event.eid,content_hash:event.data.approval.binding.content_hash,target,tag:target,commit:sha('f'),request_body:{project_id:'p'}}});
  const b0Published = published(b0,'towermd/B0');
  const recoveryRequest = {status:'awaiting_app_confirmation',request_id:'request-rebuild',project_id:'p',source_commit:sha('d'),requested_at:1000,expires_at:301000};
  const recovery = {seq:100,eid:'humanRecovery',type:'hub.rebuild.approved',actor:'human:fixture',host:'h',t:'1970-01-01T00:00:02.000Z',layer:'L3',obj:'main',guards:[],epoch:null,stale:false,cost_usd:null,data:{request_id:recoveryRequest.request_id,source_commit:recoveryRequest.source_commit}};
  for (const [key,event,binding] of [ ['humanRecovery',recovery,{request:recoveryRequest,human_actor:'human:fixture',can_confirm:true}], ['humanBaselineApproval',b0,{request_id:'baseline-request',binding:baseBinding,verdict:'approved',comment:b0.data.approval.comment,human_actor:'human:fixture'}] ]) Object.assign(receipt.checks[key],{eventId:event.eid,actor:event.actor,implementationTree:acceptedIdentity.tree,event:material(`${key}-event`,event),binding:material(`${key}-binding`,binding),implementation:material(`${key}-implementation`,implementation)});
  const classes = ['claudeCode','codex','claudeCode','codex','claudeCode','codex','claudeCode','codex','claudeCode','codex'];
  for (let i=0;i<10;i++) {
    const card=`T${String(i+1).padStart(2,'0')}`, group=i===9?1:i%3, module=['ledger','reports','filters'][group], worktree=`/original/w${group}`, taskId=`product-task-${i}`, runId=`run-${i}`, evidenceId=`evidence-${i}`, base=i*100;
    const manifests={},captured={}, captures=new Map();
    const put=(role,value,sourceRef=`/original/${card}/${role}`,format='json',id=`${card}-${role}`)=>{material(id,value);const bytes=materials.get(id);manifests[role]={path:role,sourceRef,format,sha256:hash(bytes)};captured[role]=id;captures.set(role,bytes);return id;};
    for(const [role,id]of Object.entries(shared)) put(role,materials.get(id),role,role==='input'||role==='prerequisite'?'json':role.endsWith('Binary')?'bytes':'text',id);
    const touches=i===9?['ledger','filters']:[],fences=[module,...touches].sort().map((m,n)=>({name:`module:${m}`,holder:runId,epoch:7+n})),high=[0,3,6,9].includes(i);
    const task={type:'Task',id:taskId,title:`synthetic ${card}`,module,touches,allowed_paths:[`${module}/**`],state:'running'};
    const session={kind:'human_opened',harness:classes[i]==='codex'?'actual-codex':'actual-claude',version:`version-${i}`,id:`session-${i}`,skill_loaded:'self_report',transcript_ref:`/absolute/original/transcript-${i}`,cost_usd:null};
    const run={type:'Run',id:runId,task:taskId,host:'h',worktree,branch:`branch-${i}`,state:'running',leases:fences,cost_usd:null,self_reported:{harness:session.harness,version:session.version}};
    const commands=[{cwd:'.',argv:['python3','-m','unittest','discover','-s','.','-p','test_*.py']}];
    const frozen={evidence_id:evidenceId,project_id:'p',host:'h',root:worktree,common_dir:'/original/w0/.git',task_id:taskId,run_id:runId,module_id:module,commit:sha('d'),tree:sha('e'),base:sha('a'),authority:sha('a'),baseline:'towermd/B0',plan_hash:high?sum('a'):null,commands,commands_hash:canonicalHash(commands),fences,entered_verifying_at:(base+40)*1000,merge_deadline:(base+40)*1000+3600000};
    const event=(type,n,data,actor='process:towermd-verifier',guards=fences)=>({seq:base+n,eid:`${card}-${type}-${n}`,type,actor,host:'h',t:new Date((base+n)*1000).toISOString(),layer:'L4',obj:taskId,guards,epoch:guards[0]?.epoch??null,stale:false,cost_usd:null,data});
    const claim=event('task.claimed',10,{task,run},'process:towermd-cli');claim.layer='L5';
    const start=event('finish.started',40,{task,run,input:frozen,review_context:null});
    const verificationLog=`synthetic log${i}\nRan 6 tests in 0.001s\n\nOK\n`;
    put('verificationLog',verificationLog,`/original/log-${i}`,'text',`${card}-log`);put('verificationStdout','',`/original/stdout-${i}`,'text');
    const report={commit:frozen.commit,tree:frozen.tree,executions:[{command:commands[0],result:'passed',exit_code:0,timed_out:false,stdout_log:`/original/stdout-${i}`,stderr_log:`/original/log-${i}`,stdout_sha256:hash(''),stderr_sha256:hash(verificationLog),diagnostic:null}]};
    const evidence={id:evidenceId,kind:'verification',task:taskId,run:runId,module,...Object.fromEntries(['commit','tree','base','authority','baseline','plan_hash','commands_hash','fences'].map(key=>[key,frozen[key]])),result:'passed',generated_by:'process:towermd-verifier',log:{report_eid:`${card}-finish.completed-50`,report_sha256:canonicalHash(report)},gates:{}};
    const completed=event('finish.completed',50,{task,run,evidence,report,hold:null,review_context:null});
    const terminal=event('evidence.recorded',70,{completed_eid:completed.eid,parents:[frozen.base],evidence:{gate:'g5',result:'passed',project_id:'p',task:taskId,run:runId,baseline:frozen.baseline,commands_hash:frozen.commands_hash,fences}},'process:towermd-gates');
    terminal.layer='L5';
    const reviewContexts=[],human=[];
    const observation=(gate,actual)=>{put(`human${gate}`,actual);put(`observation${gate}`,{performed_by:'human',event_id:actual.eid,actor:actual.actor,source_sha:acceptedIdentity.commit,record_ref:session.transcript_ref});return{gate,event:`human${gate}`,observation:`observation${gate}`};};
    human.push({...observation('B0',b0),publication:'publishedB0'});put('publishedB0',b0Published);
    for(const gate of high?['G1','G4']:[]) {const review=event('hold.reviewed',gate==='G1'?30:60,{},'human:fixture');review.layer='L5';review.data={fact:{actor:review.actor,answer_eid:review.eid,verdict:'approved',binding:{run:runId,task:taskId,baseline:frozen.baseline,fences,subject:gate==='G1'?{kind:'plan',sha256:frozen.plan_hash}:{kind:'tree',tree:frozen.tree}}},request_body:{project_id:'p',actor:review.actor}};reviewContexts.push({gate:gate.toLowerCase(),reviewed:review});human.push(observation(gate,review));}
    if(i===7){const b1=clone(b0);b1.eid='B1-approval';b1.data.approval.approval_eid=b1.eid;b1.data.approval.binding.target_baseline='towermd/B1';human.push({...observation('B1',b1),publication:'publishedB1'});put('publishedB1',published(b1,'towermd/B1'));}
    if(i===8){const answer=event('hold.answered',35,{},'human:fixture',[]);answer.layer='L5';answer.data={hold:{body:{kind:'ask'},reply:{id:answer.eid,actor:answer.actor,text:'OR empty is False'},recipients:[{run:runId,task:taskId}]},request_body:{project_id:'p',actor:answer.actor}};human.push(observation('answer',answer));}
    const merged=event('merge.done',80,{task:{...task,state:'merged'},run:{...run,state:'ended',verification_evidence:evidenceId},evidence,commit:sha('f'),tree:sha('e'),proof:{schema:1,claim,started:start,completed,integration_gate:terminal,review_contexts:reviewContexts}},'process:towermd-integrator',[]);
    const trailers={'Towermd-Task':taskId,'Towermd-Run':runId,'Towermd-Module':module,'Towermd-Evidence':evidenceId,'Towermd-Epoch':String(fences.find(f=>f.name===`module:${module}`).epoch)},message=Object.entries(trailers).map(([k,v])=>`${k}: ${v}`).join('\n');
    put('task',task);put('session',session);put('harnessVersion',session.version+'\n',`/original/${card}/harnessVersion`,'text');put('transcript',`synthetic transcript-${i}`,session.transcript_ref,'text');
    put('finish',{input:frozen,evidence,report,actual:completed,job_state:'completed',result:'passed'},`/original/evidence-${i}`,'cli-json',`${card}-evidence`);put('proof',merged,`/original/proof-${i}`);put('trailers',message,`/original/${card}/trailers`,'text');
    put('mainStdout','',`/original/${card}/mainStdout`,'text');put('mainStderr','Ran 6 tests in 0.001s\n\nOK\n',`/original/${card}/mainStderr`,'text');put('mainTests',{commit:sha('f'),tree:sha('e'),cwd:worktree,argv:commands[0].argv,exit_code:0,stdout_ref:manifests.mainStdout.sourceRef,stderr_ref:manifests.mainStderr.sourceRef},`/original/main-tests-${i}`);put('checkpoint',{captured:'synthetic checkpoint output'});
    const manifest={format:'towermd-p7-captured-v1',origin:'harness_capture',card,attempt:1,authorityRef:'refs/heads/main',materials:manifests,human,checkpoints:['checkpoint'],failures:[]};
    const git={worktree,mainCommit:sha('f'),mainTree:sha('e'),verifiedTree:sha('e'),authorityContains:true,mainMessage:message,baselines:{B0:sha('f'),...(i===7?{B1:sha('f')}:{})}};
    const record={task:card,attempt:1,result:'pass',source:{sha:input.sha,tag:input.tag,cli_sha256:input.cli_sha256,app_sha256:input.app_sha256,skill_hashes:skillHashes},session,binding:{project:'p',run:runId,host:'h',worktree,branch:run.branch,module,fences},verification:{commit:frozen.commit,tree:frozen.tree,evidence_id:evidenceId,captured_source_ref:manifests.finish.sourceRef,object_ref:null,object_sha256:'',commands_hash:frozen.commands_hash,log_ref:manifests.verificationLog.sourceRef,log_sha256:hash(verificationLog),result:'passed',entered_at:new Date(frozen.entered_verifying_at).toISOString(),deadline:new Date(frozen.merge_deadline).toISOString()},main:{authority_ref:manifest.authorityRef,commit:git.mainCommit,tree:git.mainTree,trailers,proof_ref:manifests.proof.sourceRef,merge_done_eid:merged.eid,task_state:'merged',run_state:'ended',test_count:6,test_exit:0,test_log_ref:manifests.mainTests.sourceRef},human_refs:human.flatMap(row=>[manifests[row.event].sourceRef,manifests[row.observation].sourceRef,...(row.publication?[manifests[row.publication].sourceRef]:[])]),checkpoint_refs:[manifests.checkpoint.sourceRef],failure_refs:[],missing:[]};
    const bindings={};for(const [key,role]of Object.entries({capturedSource:'finish',proof:'proof',mainTests:'mainTests',transcript:'transcript',verificationLog:'verificationLog'}))bindings[key]={sourceRef:manifests[role].sourceRef,material:captured[role]};
    for(const [key,field]of [['human','human_refs'],['checkpoints','checkpoint_refs'],['failures','failure_refs']])bindings[key]=record[field].map(sourceRef=>({sourceRef,material:captured[Object.keys(manifests).find(role=>manifests[role].sourceRef===sourceRef)]}));
    receipt.p7.tasks.push({card,taskId,harnessClass:classes[i],record:material(`${card}-record`,record),task:captured.task,session:captured.session,git:material(`${card}-git`,git),evidence:captured.finish,proof:captured.proof,trailers:captured.trailers,mainMessage:material(`${card}-mainMessage`,message),mainTests:captured.mainTests,captureManifest:material(`${card}-captureManifest`,manifest),captured,bindings});
  }
  return { receipt, materials, assets, rawSources:implementation.sources, rawBuilds:[implementation], readiness: { path: receipt.readiness.path, blob: sha('d'), sha256: sum('e') } };
}
function checkAcceptance(f) { return units.checkAcceptance(f.receipt, { identity, assets: f.assets, readiness: f.readiness, materials: f.materials }); }
function editRecord(f, i, change) {
  const id = f.receipt.p7.tasks[i].record, value = JSON.parse(f.materials.get(id)); change(value); const bytes = Buffer.from(JSON.stringify(value)); f.materials.set(id, bytes); f.receipt.materials[id].sha256 = hash(bytes);
}
test('fixed record/sourceRef parser accepts per-card versions, unknown costs, optional empty lists and null Git object path', () => assert.doesNotThrow(() => checkAcceptance(acceptanceFixture())));
for (const [name, change] of [
  ['wrong identity', f => f.receipt.identity = { ...identity, tagObject: sha('f') }],
  ['sums own bytes mismatch', f => f.receipt.assets.find(a => a.name === 'SHA256SUMS').sha256 = sum('f')],
  ['readiness wrong blob', f => f.receipt.readiness.blob = sha('e')],
  ['missing review', f => f.receipt.readiness.review = 'missing'],
  ['missing material role', f => delete f.receipt.p7.tasks[0].proof],
  ['missing raw material', f => f.materials.delete(f.receipt.p7.tasks[0].evidence)],
  ['material corrupt', f => f.materials.set(f.receipt.p7.tasks[0].task, Buffer.from('corrupt'))],
  ['nine cards', f => f.receipt.p7.tasks.pop()],
  ['duplicate task id', f => f.receipt.p7.tasks[1].taskId = f.receipt.p7.tasks[0].taskId],
  ['duplicate run', f => editRecord(f, 1, r => r.binding.run = 'run-0')],
  ['duplicate evidence', f => editRecord(f, 1, r => r.verification.evidence_id = 'evidence-0')],
  ['two worktrees', f => f.receipt.p7.tasks.forEach((_, i) => editRecord(f, i, r => r.binding.worktree = `/original/w${i % 2}`))],
  ['wrong harness class for card', f => f.receipt.p7.tasks[0].harnessClass = 'codex'],
  ['empty version', f => editRecord(f, 0, r => r.session.version = '')],
  ['wrong task label', f => editRecord(f, 0, r => r.task = 'T02')],
  ['no canonical event', f => editRecord(f, 0, r => r.main.merge_done_eid = '')],
  ['not merged', f => editRecord(f, 0, r => r.main.task_state = 'running')],
  ['not ended', f => editRecord(f, 0, r => r.main.run_state = 'running')],
  ['no main tests', f => editRecord(f, 0, r => r.main.test_count = 0)],
  ['main tests failed', f => editRecord(f, 0, r => r.main.test_exit = 1)],
  ['captured source mismatch', f => f.receipt.p7.tasks[0].bindings.capturedSource.sourceRef += 'x'],
  ['proof wrong role', f => f.receipt.p7.tasks[0].bindings.proof.material = f.receipt.p7.tasks[0].evidence],
  ['extra fixed binding', f => f.receipt.p7.tasks[0].bindings.unexpected = { sourceRef: 'x', material: 'review' }],
  ['unbound original list entry', f => editRecord(f, 0, r => r.human_refs.push('/original/human'))],
  ['extra list binding', f => f.receipt.p7.tasks[0].bindings.failures.push({ sourceRef: 'unexpected', material: 'review' })],
  ['verification log bytes mismatch', f => editRecord(f, 0, r => r.verification.log_sha256 = sum('a'))],
  ['source changed without reuse', f => editRecord(f, 0, r => r.source.sha = sha('d'))],
  ['architecture duplicated', f => f.receipt.architectures[1].target = targets[0]],
  ['ARM only static', f => f.receipt.architectures[0].level = 'static'],
  ['missing check', f => delete f.receipt.checks.archiveIntegrity],
  ['empty observation refs', f => f.receipt.checks.reopen.refs = []],
  ['different daemon instance', f => f.receipt.checks.daemonLifetime.after.instance = 'other'],
  ['wrong install asset', f => f.receipt.checks.installation.sourceAsset = 'tower.md-0.1.0-aarch64.app.zip'],
  ['process actor', f => f.receipt.checks.humanRecovery.actor = 'process:towermd-cli'],
  ['same human event', f => f.receipt.checks.humanRecovery.eventId = f.receipt.checks.humanBaselineApproval.eventId],
  ['missing human binding', f => delete f.receipt.checks.humanRecovery.binding],
  ['old human tree without reuse', f => f.receipt.checks.humanRecovery.implementationTree = sha('f')],
]) test(`fixed acceptance bindings reject ${name}`, () => { const f = acceptanceFixture(); change(f); assert.throws(() => checkAcceptance(f), /release/); });

function localFixture(t) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'towermd-publish-local-'))); t.after(() => rmSync(root, { recursive: true, force: true }));
  const output = join(root, 'assets'), logs = output + '.logs'; mkdirSync(output); mkdirSync(logs);
  const f = acceptanceFixture(), p = provenanceFixture();
  const blobs = new Map(), skillPaths = ['Init','Baseline','Claim','Ask','Propose','Finish'].map(name => `skills/skills/towermd-${name.toLowerCase()}/SKILL.md`), sourcePaths = p.sources.map(s => s.path).concat(['CHANGELOG.md','CHANGELOG.zh-CN.md',f.readiness.path], skillPaths);
  const fileBytes = path => Buffer.from(skillPaths.includes(path) ? f.materials.get(`global-skill${path.split('/')[2].slice(8).replace(/^./, c => c.toUpperCase())}`) : path === 'CHANGELOG.md' ? '# Changelog\n\n## Unreleased\n\n## v0.1.0 — 2026-10-17\n\n- Frozen English.\n' : path === 'CHANGELOG.zh-CN.md' ? '# 更新日志\n\n## 未发布\n\n## v0.1.0 — 2026-10-17\n\n- 冻结中文。\n' : `synthetic frozen ${path}`);
  const sources = sourcePaths.sort().map(path => { const bytes = fileBytes(path), blob = createHash('sha1').update(`blob ${bytes.length}\0`).update(bytes).digest('hex'); blobs.set(blob, bytes); return { path, mode:'100644', blob, sha256: hash(bytes) }; });
  const locks = Object.fromEntries(['Cargo.lock','src-tauri/Cargo.lock','pnpm-lock.yaml'].map(k => [k, sources.find(s => s.path === k).sha256]));
  p.before.sources = sources; for (const b of p.before.builds) { b.sources = sources; b.lockfiles = locks; } p.after = clone(p.before); p.manifest.lockfiles = locks;
  const actualBuildBytes=Buffer.from(JSON.stringify(p.before.builds[0]));
  for(const id of ['prerequisite','humanRecovery-implementation','humanBaselineApproval-implementation']) { f.materials.set(id,actualBuildBytes);f.receipt.materials[id].sha256=hash(actualBuildBytes); }
  for(const card of f.receipt.p7.tasks) { const manifest=JSON.parse(f.materials.get(card.captureManifest));manifest.materials.prerequisite.sha256=hash(actualBuildBytes);const bytes=Buffer.from(JSON.stringify(manifest));f.materials.set(card.captureManifest,bytes);f.receipt.materials[card.captureManifest].sha256=hash(bytes); }
  f.rawBuilds=p.before.builds;
  const binary = expectedAssets('0.1.0').filter(n => /(?:\.zip|\.dmg|\.tar\.gz)$/.test(n));
  p.manifest.version = '0.1.0'; p.manifest.artifacts = binary.map(name => { const bytes = Buffer.from(`synthetic ${name}`); writeFileSync(join(output,name),bytes); return { name, size:bytes.length, sha256: hash(bytes), target: name.includes('skills') ? null : name.includes('aarch64') ? targets[0] : targets[1] }; });
  writeFileSync(join(output,'BUILD-MANIFEST.json'),JSON.stringify(p.manifest));
  writeFileSync(join(output,'RELEASE-NOTES.en.md'),'# Tower.md 0.1.0\n\n- Frozen English.\n\nSynthetic install text.\n');
  writeFileSync(join(output,'RELEASE-NOTES.zh-CN.md'),'# Tower.md 0.1.0\n\n- 冻结中文。\n\n合成安装说明。\n');
  const bytes = new Map(expectedAssets('0.1.0').filter(n => n !== 'SHA256SUMS').map(n => [n,readFileSync(join(output,n))]));
  writeFileSync(join(output,'SHA256SUMS'), [...bytes].map(([n,b]) => `${hash(b)}  ${n}`).join('\n') + '\n');
  f.receipt.assets = expectedAssets('0.1.0').map(name => { const b = readFileSync(join(output,name)); return { name, size: b.length, sha256: hash(b) }; });
  const readiness = sources.find(s => s.path === f.readiness.path); Object.assign(f.receipt.readiness, { blob:readiness.blob, sha256:readiness.sha256 });
  for (const [id,b] of f.materials) { const path = join(logs,f.receipt.materials[id].path); mkdirSync(dirname(path),{recursive:true});writeFileSync(path,b); }
  const saveReceipt = () => writeFileSync(join(logs,'acceptance.json'),JSON.stringify(f.receipt)); saveReceipt();
  for (const [name,data] of [['inputs-before.json',p.before],['inputs-after.json',p.after]]) writeFileSync(join(logs,name),JSON.stringify(data));
  writeFileSync(join(logs,'commands.jsonl'),p.commands.map(c => JSON.stringify(c)).join('\n')+'\n');
  writeFileSync(join(logs,'stdout.log'),'synthetic aggregate'); writeFileSync(join(logs,'stderr.log'),'synthetic aggregate');
  const calls = [], git = args => {
    calls.push(args);
    const key = JSON.stringify(args);
    if (key === JSON.stringify(['cat-file','-t',`refs/tags/${tag}`])) return Buffer.from('tag\n');
    for (const [ref,value] of [[`refs/tags/${tag}`,identity.tagObject],[`refs/tags/${tag}^{commit}`,identity.commit],[`refs/tags/${tag}^{tree}`,identity.tree],['refs/heads/main',identity.commit]])
      if (key === JSON.stringify(['rev-parse','--verify',ref])) return Buffer.from(value+'\n');
    if (key === JSON.stringify(['ls-tree','-r','-z','--full-tree',identity.commit])) return Buffer.from(sources.map(s => `${s.mode} blob ${s.blob}\t${s.path}\0`).join(''));
    if (args[0] === 'cat-file' && args[1] === 'blob' && blobs.has(args[2])) return blobs.get(args[2]);
    throw new Error(`unexpected fixture git ${key}`);
  };
  return { ...f, root, output, logs, git, calls, sources, p, saveReceipt };
}
test('local reader binds nine bytes, frozen readiness and provenance without raw-reference filesystem reads', t => {
  const f = localFixture(t); const result = units.readLocal({ tag, output:f.output },f.git);
  assert.ok(result, 'local frozen input snapshot required'); assert.deepEqual(result.identity, identity); assert.equal(result.assets.length,9);
  assert.equal(result.assets.find(a => a.name === 'SHA256SUMS').sha256,hash(readFileSync(join(f.output,'SHA256SUMS'))));
});
for (const [name, change, pattern] of [
  ['missing receipt', f => rmSync(join(f.logs,'acceptance.json')), /acceptance/],
  ['extra asset', f => writeFileSync(join(f.output,'latest.json'),'x'), /asset set/],
  ['binary changed', f => writeFileSync(join(f.output,'tower.md-0.1.0-aarch64.dmg'),'x'), /checksum/],
  ['SUMS own byte changed', f => writeFileSync(join(f.output,'SHA256SUMS'),readFileSync(join(f.output,'SHA256SUMS'),'utf8')+'\n'), /acceptance assets/],
  ['asset symlink', f => { const p=join(f.output,'SHA256SUMS');rmSync(p);symlinkSync('/does/not/exist',p); }, /asset/],
  ['material symlink', f => { const p=join(f.logs,f.receipt.materials.review.path);rmSync(p);symlinkSync('/etc/passwd',p); }, /material/],
  ['material directory symlink', f => { const p=join(f.logs,'acceptance-evidence');rmSync(p,{recursive:true});symlinkSync('/tmp',p); }, /material/],
  ['material escape', f => { f.receipt.materials.review.path='acceptance-evidence/../outside';f.saveReceipt(); }, /material path/],
  ['material absolute original', f => { f.receipt.materials.review.path='/original/secret';f.saveReceipt(); }, /material path/],
  ['material control path', f => { f.receipt.materials.review.path='acceptance-evidence/a\nb';f.saveReceipt(); }, /material path/],
  ['material hash changed', f => writeFileSync(join(f.logs,f.receipt.materials.review.path),'x'), /material hash/],
  ['provenance missing', f => rmSync(join(f.logs,'inputs-after.json')), /provenance/],
  ['provenance source drift', f => {f.p.after.sources.pop();writeFileSync(join(f.logs,'inputs-after.json'),JSON.stringify(f.p.after));}, /provenance/],
]) test(`local reader rejects ${name}`, t => { const f=localFixture(t);change(f);assert.throws(()=>units.readLocal({tag,output:f.output},f.git),pattern); });

test('public entry refuses missing actual P7 raw roles before any network command or mutation', async t => {
  const f = localFixture(t), calls=[]; delete f.receipt.p7.tasks[0].captureManifest; f.saveReceipt();
  const run=(file,args,options)=>{calls.push([file,args]);assert.equal(file,'git');return {status:0,stdout:f.git(args),stderr:''};};
  await assert.rejects(units.publishFrozen({repo,tag,output:f.output,download:join(f.root,'public')},run), /acceptance raw controlled material identity/);
  assert(!calls.some(([file,args])=>file==='gh'||file==='curl'||args[0]==='push'));
});

test('fixed records enforce the prescribed three worktree groups including T10 in reports tree', () => {
  const f=acceptanceFixture();editRecord(f,9,r=>r.binding.worktree='/original/w0');
  assert.throws(()=>checkAcceptance(f),/worktree group/);
});
test('one original sourceRef cannot bind contradictory captured materials', () => {
  const f=acceptanceFixture(), c=f.receipt.p7.tasks[0];editRecord(f,0,r=>r.session.transcript_ref=r.verification.log_ref);
  c.bindings.transcript.sourceRef=c.bindings.verificationLog.sourceRef;
  assert.throws(()=>checkAcceptance(f),/contradictory sourceRef/);
});

test('human actor preserves ordinary ids and rejects whitespace', () => {
  const f=acceptanceFixture();f.receipt.checks.humanRecovery.actor='human:sam';assert.doesNotThrow(()=>checkAcceptance(f));
  f.receipt.checks.humanRecovery.actor='human:two words';assert.throws(()=>checkAcceptance(f),/humanRecovery identity/);
});

function remoteFixture(t, { count=9, draft=true, missing=false }={}) {
  const f=localFixture(t), local=units.readLocal({tag,output:f.output},f.git), download=join(f.root,'public');
  const all=new Map(expectedAssets('0.1.0').map(n=>[n,readFileSync(join(f.output,n))]));
  const state={main:identity.commit,tagObject:identity.tagObject,tagCommit:identity.commit,release:!missing,draft,body:local.expectedBody,id:42,reads:0,curls:0,downloads:0,
    runnerErrors:[],assets:expectedAssets('0.1.0').slice(0,count).map((name,i)=>({id:i+100,name,size:all.get(name).length,state:'uploaded',browser_download_url:`https://github.com/${repo}/releases/download/${tag}/${name}`})),bytes:new Map(all),calls:[]};
  const ok=stdout=>({status:0,stdout,stderr:''}), encoded=value=>ok(JSON.stringify(value));
  const dispatch=(file,args,options={})=>{
    state.calls.push({file,args:[...args],cwd:options.cwd,env:options.env}); assert.equal(options.shell,false);
    if(state.hook){const override=state.hook(file,args,options);if(override)return override;}
    if(file==='git') {
      const key=JSON.stringify(args);
      if(key===JSON.stringify(['remote','get-url','origin'])||key===JSON.stringify(['remote','get-url','--all','origin'])||key===JSON.stringify(['remote','get-url','--push','--all','origin'])) return ok('git@github.com:fixture/tower.git\n');
      if(key===JSON.stringify(['ls-remote','origin','refs/heads/main'])) return ok(state.main?`${state.main}\trefs/heads/main\n`:'');
      if(key===JSON.stringify(['ls-remote','origin',`refs/tags/${tag}`,`refs/tags/${tag}^{}`])) return ok(state.tagObject?`${state.tagObject}\trefs/tags/${tag}\n${state.tagCommit}\trefs/tags/${tag}^{}\n`:'');
      if(args[0]==='push'){assert.deepEqual(args.slice(0,3),['push','--no-follow-tags','origin']);if(args[3]==='main')state.main=identity.commit;else {assert.equal(args[3],`refs/tags/${tag}`);state.tagObject=identity.tagObject;state.tagCommit=identity.commit;}return ok('');}
      if(args[0]==='merge-base'){assert.deepEqual(args,['merge-base','--is-ancestor',state.main,identity.commit]);return ok('');}
      if(args[0]==='cat-file'&&args[1]==='-t'&&args[2]===state.main) return ok('commit\n');
      return ok(f.git(args));
    }
    if(file==='gh') {
      if(args[0]==='release'&&args[1]==='view') return state.release?encoded({tagName:tag,isDraft:state.draft,url:`https://github.com/${repo}/releases/tag/${tag}`,body:state.body,assets:state.assets.map(a=>({name:a.name}))}):{status:1,stderr:'not found'};
      if(args[0]==='api'&&args.includes('--include')) {
        state.reads++;if(state.onRead)state.onRead(state.reads);
        return state.release?ok('HTTP/2.0 200 OK\r\nContent-Type: application/json\r\n\r\n'+JSON.stringify({id:state.id,tag_name:tag,draft:state.draft,html_url:`https://github.com/${repo}/releases/tag/${tag}`,body:state.body})):{status:1,stdout:'HTTP/2.0 404 Not Found\r\nContent-Type: application/json\r\n\r\n{"message":"Not Found"}',stderr:'not found'};
      }
      if(args[0]==='api'&&args.includes('--paginate')) {assert.deepEqual(args,['api','--hostname','github.com','--method','GET','--paginate','--slurp',`repos/${repo}/releases/${state.id}/assets?per_page=100`]);return encoded(state.pages??[state.assets]);}
      if(args[0]==='release'&&args[1]==='create') {assert.equal(readFileSync(args.at(-1),'utf8'),local.expectedBody);state.release=true;state.draft=true;return ok('');}
      if(args[0]==='release'&&args[1]==='upload') {
        const name=args[3].split('/').at(-1);state.assets.push({id:state.assets.length+100,name,size:all.get(name).length,state:'uploaded',browser_download_url:`https://github.com/${repo}/releases/download/${tag}/${name}`});state.bytes.set(name,readFileSync(args[3]));
        if(state.failUploadOnce){state.failUploadOnce=false;return {status:1,stderr:'synthetic interrupted after server accepted'};}return ok('');
      }
      if(args[0]==='release'&&args[1]==='download') {state.downloads++;const name=args[args.indexOf('--pattern')+1],dir=args[args.indexOf('--dir')+1];writeFileSync(join(dir,name),state.bytes.get(name));return ok('');}
      if(args[0]==='release'&&args[1]==='edit') {assert.equal(args.at(-1),'--draft=false');state.draft=false;return ok('');}
    }
    if(file==='curl') {
      state.curls++;assert.equal(args[0],'--disable');assert.deepEqual(Object.keys(options.env).sort(),Object.keys(units.anonymousEnv()).sort());
      assert.equal(options.env.PATH,'/usr/bin:/bin:/usr/sbin:/sbin');const output=args[args.indexOf('--output')+1],name=output.split('/').at(-1);writeFileSync(output,state.bytes.get(name));return ok('200\nhttps://release-assets.githubusercontent.com/synthetic/file?signature=DO_NOT_SAVE\n');
    }
    if(file==='shasum') {assert.deepEqual(args,['-a','256','-c','SHA256SUMS']);verifySums(readFileSync(join(options.cwd,'SHA256SUMS'),'utf8'),new Map(expectedAssets('0.1.0').filter(n=>n!=='SHA256SUMS').map(n=>[n,readFileSync(join(options.cwd,n))])));return ok('synthetic checksums OK\n');}
    throw new Error(`unexpected fake command ${file} ${JSON.stringify(args)}`);
  };
  const run=(...args)=>{try{return dispatch(...args);}catch(error){state.runnerErrors.push(error.message);throw error;}};
  t.after(()=>assert.deepEqual(state.runnerErrors,[], 'fake runner must recognize every command and satisfy its argv assertions'));
  const invoke=()=>units.publishRemote({repo,tag,output:f.output,download,local},run);
  const mutation=()=>state.calls.filter(c=>c.file==='git'&&c.args[0]==='push'||c.file==='gh'&&c.args[0]==='release'&&['create','upload','edit'].includes(c.args[1]));
  return {...f,local,download,state,run,invoke,mutation};
}
test('private remote state machine authenticates nine assets, publishes once, anonymously compares nine bytes and sanitizes output', async t=>{
  const f=remoteFixture(t);const result=await f.invoke();assert.ok(result,'remote receipt required');assert.equal(result.url,`https://github.com/${repo}/releases/tag/${tag}`);
  assert.equal(f.state.curls,9);assert.equal(f.mutation().filter(c=>c.args[1]==='edit').length,1);
  assert.deepEqual(readdirSync(f.download).sort(),expectedAssets('0.1.0'));
  for(const name of readdirSync(f.download+'.logs'))if(name.endsWith('.json')||name.endsWith('.jsonl'))assert(!readFileSync(join(f.download+'.logs',name),'utf8').includes('DO_NOT_SAVE'));
});
test('private remote missing draft uses anchored API 404 and only uploads missing files',async t=>{
  const f=remoteFixture(t,{missing:true,count:0});await f.invoke();assert.equal(f.mutation().filter(c=>c.args[1]==='create').length,1);assert.equal(f.mutation().filter(c=>c.args[1]==='upload').length,9);
});
test('private remote reuses byte-identical subset and existing public has no mutation',async t=>{
  const f=remoteFixture(t,{count:4});await f.invoke();assert.equal(f.mutation().filter(c=>c.args[1]==='upload').length,5);
  const p=remoteFixture(t,{draft:false});await p.invoke();assert.equal(p.mutation().length,0);assert.equal(p.state.curls,9);
});
for(const [name,change,pattern] of [
  ['remote annotated object changed', f=>f.state.tagObject=sha('d'),/remote tag/],
  ['remote peeled commit changed', f=>f.state.tagCommit=sha('d'),/remote tag/],
  ['remote body conflict',f=>f.state.body='other',/release body/],
  ['extra asset',f=>f.state.assets.push({...f.state.assets[0],id:999,name:'latest.json'}),/asset set/],
  ['duplicate asset id',f=>f.state.assets[1].id=f.state.assets[0].id,/asset id/],
  ['same name conflicting bytes',f=>f.state.bytes.set(f.state.assets[0].name,Buffer.from('changed')),/asset.*(bytes|size|conflict)/],
  ['additional page asset',f=>f.state.pages=[f.state.assets,[{...f.state.assets[0],id:999,name:'latest.json'}]],/asset set/],
  ['cross-page duplicate',f=>f.state.pages=[f.state.assets,[f.state.assets[0]]],/asset (set|id)/],
  ['unsafe API download URL',f=>f.state.assets[0].browser_download_url+='?secret=x',/asset URL/],
])test(`private remote rejects ${name} without mutation`,async t=>{const f=remoteFixture(t);change(f);await assert.rejects(f.invoke(),pattern);assert.equal(f.mutation().length,0);});

function onView(state,n,mutate) {let views=0;state.hook=(file,args)=>{if(file==='gh'&&args[0]==='release'&&args[1]==='view'&&++views===n)mutate();};}
for(const [name,change,pattern] of [
  ['S2 asset id changes with same size',f=>onView(f.state,3,()=>f.state.assets[0].id=999),/asset id\/size\/set drift/],
  ['S2 asset size changes',f=>onView(f.state,3,()=>f.state.assets[0].size++),/asset id\/size\/set drift/],
  ['S2 additional signature',f=>onView(f.state,3,()=>f.state.assets.push({...f.state.assets[0],id:999,name:'old.sig'})),/asset set/],
  ['S2 body changes',f=>onView(f.state,3,()=>f.state.body+='x'),/release body/],
  ['S2 tag object changes',f=>{let n=0;f.state.hook=(file,args)=>{if(file==='git'&&args[0]==='ls-remote'&&args[2]===`refs/tags/${tag}`&&++n===3)f.state.tagObject=sha('d');};},/remote tag/],
  ['local receipt drift after S1 authentication',f=>{let n=0;f.state.hook=(file,args)=>{if(file==='gh'&&args[0]==='release'&&args[1]==='download'&&++n===18)writeFileSync(join(f.logs,'acceptance.json'),readFileSync(join(f.logs,'acceptance.json'),'utf8')+'\n');};},/local frozen inputs drift/],
  ['local material drift after S1 authentication',f=>{let n=0;f.state.hook=(file,args)=>{if(file==='gh'&&args[0]==='release'&&args[1]==='download'&&++n===18)writeFileSync(join(f.logs,f.receipt.materials.review.path),'changed');};},/material hash/],
  ['S3 id swap after publish',f=>onView(f.state,4,()=>f.state.assets[0].id=999),/已公开.*asset id\/size\/set drift/],
  ['final body drift during anonymous transfer',f=>onView(f.state,5,()=>f.state.body+='changed'),/已公开.*release body/],
])test(`private state detects ${name}`,async t=>{const f=remoteFixture(t);change(f);await assert.rejects(f.invoke(),pattern);assert(!existsSync(join(f.download+'.logs','receipt.json')));if(name.startsWith('S2')||name.startsWith('local'))assert(!f.mutation().some(c=>c.args[1]==='edit'));});

test('S2 consistent public race proceeds only to anonymous verification',async t=>{const f=remoteFixture(t);onView(f.state,3,()=>f.state.draft=false);await f.invoke();assert.equal(f.mutation().length,0);assert.equal(f.state.curls,9);});
test('an observed public release made draft again must never be edited',async t=>{const f=remoteFixture(t,{draft:false});onView(f.state,2,()=>f.state.draft=true);await assert.rejects(f.invoke(),/public release became draft/);assert.equal(f.mutation().length,0);});
test('release id replacement during upload is rejected',async t=>{const f=remoteFixture(t,{count:8});f.state.hook=(file,args)=>{if(file==='gh'&&args[1]==='upload')f.state.id=99;};await assert.rejects(f.invoke(),/release id drift/);assert(!f.mutation().some(c=>c.args[1]==='edit'));});

test('interrupted upload is retried from actual remote bytes in a new download directory',async t=>{
  const f=remoteFixture(t,{count:4});f.state.failUploadOnce=true;await assert.rejects(f.invoke(),/gh command/);assert.equal(f.state.assets.length,5);assert(f.state.draft);
  const second=join(f.root,'public-retry');await units.publishRemote({repo,tag,output:f.output,download:second,local:f.local},f.run);
  assert.equal(f.mutation().filter(c=>c.args[1]==='upload').length,5);assert.equal(f.mutation().filter(c=>c.args[1]==='edit').length,1);
  assert(!f.state.calls.some(c=>c.args.includes('--clobber')||c.args.includes('--force')||c.args.includes('--tags')||c.args.includes('--all')&&c.args[0]==='push'));
});
for(const [label,result] of [
  ['401',{status:1,stdout:'HTTP/2.0 401 Unauthorized\r\n\r\n{}',stderr:'404'}],
  ['403',{status:1,stdout:'HTTP/2.0 403 Forbidden\r\n\r\n{}',stderr:'404'}],
  ['transport',{status:1,stdout:'',stderr:'404 not found'}],
  ['unanchored',{status:1,stdout:'sentinel\nHTTP/2.0 404 Not Found\n\n{}',stderr:''}],
  ['bad JSON',{status:0,stdout:'HTTP/2.0 200 OK\r\n\r\nnot JSON'}],
  ['500',{status:1,stdout:'HTTP/2.0 500 Server Error\n\n{}'}],
])test(`API ${label} cannot create or publish`,async t=>{const f=remoteFixture(t);f.state.hook=(file,args)=>file==='gh'&&args[0]==='api'&&args.includes('--include')?result:null;await assert.rejects(f.invoke(),/release API/);assert.equal(f.mutation().length,0);});
for(const payload of ['{}','[{}]','not json'])test('incomplete/malformed pagination cannot publish',async t=>{const f=remoteFixture(t);f.state.hook=(file,args)=>file==='gh'&&args[0]==='api'&&args.includes('--paginate')?{status:0,stdout:payload}:null;await assert.rejects(f.invoke(),/asset/);assert.equal(f.mutation().length,0);});
test('page transport failure is rejected and arbitrary stderr is never stored',async t=>{const f=remoteFixture(t);f.state.hook=(file,args)=>file==='gh'&&args.includes('--paginate')?{status:1,stdout:'[]',stderr:'AUTH_SECRET_DO_NOT_SAVE'}:null;await assert.rejects(f.invoke(),/gh command/);assert.equal(f.mutation().length,0);for(const name of readdirSync(f.download+'.logs'))if(name.endsWith('.json')||name.endsWith('.jsonl'))assert(!readFileSync(join(f.download+'.logs',name),'utf8').includes('AUTH_SECRET_DO_NOT_SAVE'));});

for(const [name,override,pattern] of [
  ['curl HTTP404',()=>({status:22,stdout:'404\nhttps://github.com/x\n',stderr:'synthetic'}),/curl command/],
  ['HTML', (args)=>{writeFileSync(args[args.indexOf('--output')+1],'<html>login</html>');return {status:0,stdout:'200\nhttps://cdn.example/file?secret=x\n'};},/anonymous asset bytes/],
  ['truncated', (args)=>{writeFileSync(args[args.indexOf('--output')+1],'');return {status:0,stdout:'200\nhttps://cdn.example/file\n'};},/anonymous asset bytes/],
  ['HTTP result not 200',()=>({status:0,stdout:'404\nhttps://cdn.example/file\n'}),/anonymous HTTP/],
  ['HTTP downgrade',()=>({status:0,stdout:'200\nhttp://cdn.example/file\n'}),/anonymous HTTPS/],
  ['redirect credentials',()=>({status:0,stdout:'200\nhttps://secret@cdn.example/file\n'}),/anonymous HTTPS/],
])test(`anonymous ${name} reports public but incomplete`,async t=>{const f=remoteFixture(t);f.state.hook=(file,args)=>file==='curl'?override(args):null;await assert.rejects(f.invoke(),e=>e.message.includes('已公开，匿名回读未完成')&&pattern.test(e.message));assert(!existsSync(join(f.download+'.logs','receipt.json')));assert.equal(f.mutation().filter(c=>c.args[1]==='edit').length,1);});
test('anonymous SUMS itself cannot change even if the other eight hashes still validate',async t=>{const f=remoteFixture(t);f.state.hook=(file,args)=>{if(file==='curl'&&args.at(-1).endsWith('/SHA256SUMS')){writeFileSync(join(f.download,'SHA256SUMS'),readFileSync(join(f.output,'SHA256SUMS'),'utf8')+'\n');return {status:0,stdout:'200\nhttps://cdn.example/file\n'};}};await assert.rejects(f.invoke(),/已公开.*anonymous asset bytes/);});
test('shasum exit failure cannot yield a complete receipt',async t=>{const f=remoteFixture(t);f.state.hook=(file)=>file==='shasum'?{status:1,stdout:'',stderr:'bad checksum'}:null;await assert.rejects(f.invoke(),/已公开.*shasum command/);assert(!existsSync(join(f.download+'.logs','receipt.json')));});

for(const [name,override,pattern] of [
  ['multi fetch URL',(args)=>args[0]==='remote'&&!args.includes('--push')?{status:0,stdout:'git@github.com:fixture/tower.git\ngit@github.com:other/tower.git\n'}:null,/origin unique/],
  ['push URL differs',(args)=>args[0]==='remote'&&args.includes('--push')?{status:0,stdout:'git@github.com:other/tower.git\n'}:null,/origin repository/],
  ['local main advanced',(args)=>JSON.stringify(args)===JSON.stringify(['rev-parse','--verify','refs/heads/main'])?{status:0,stdout:sha('d')+'\n'}:null,/main differs/],
  ['lightweight remote tag',(args)=>args[0]==='ls-remote'&&args[2]===`refs/tags/${tag}`?{status:0,stdout:`${identity.commit}\trefs/tags/${tag}\n`}:null,/remote tag/],
  ['remote ref network failure',(args)=>args[0]==='ls-remote'?{status:1,stdout:'',stderr:'synthetic'}:null,/git command/],
])test(`fixed Git boundaries reject ${name}`,async t=>{const f=remoteFixture(t);f.state.hook=(file,args)=>file==='git'?override(args):null;await assert.rejects(f.invoke(),pattern);assert.equal(f.mutation().length,0);});
test('only exact main and annotated tag are pushed with followTags disabled',async t=>{const f=remoteFixture(t);f.state.main=sha('d');f.state.tagObject=null;await f.invoke();assert.deepEqual(f.mutation().filter(c=>c.file==='git').map(c=>c.args),[['push','--no-follow-tags','origin','main'],['push','--no-follow-tags','origin',`refs/tags/${tag}`]]);});
test('unknown remote main cannot be treated as an ancestor',async t=>{const f=remoteFixture(t);f.state.main=sha('d');f.state.hook=(file,args)=>file==='git'&&args[0]==='cat-file'&&args[2]===sha('d')?{status:1,stderr:'unknown object'}:null;await assert.rejects(f.invoke(),/git command/);assert.equal(f.mutation().length,0);});

test('anonymous directory must still match all frozen bytes after shasum and final read',async t=>{
  const f=remoteFixture(t);f.state.hook=(file)=>{if(file==='shasum'){writeFileSync(join(f.download,'SHA256SUMS'),readFileSync(join(f.output,'SHA256SUMS'),'utf8')+'\n');return {status:0,stdout:'synthetic OK'};}};
  await assert.rejects(f.invoke(),/anonymous asset bytes/);assert(!existsSync(join(f.download+'.logs','receipt.json')));
});

for(const args of [[],['--skip','yes'],['--tag',tag,'--tag',tag,'--output','/tmp/o','--download','/tmp/d'],['--tag'],
  ['--tag','latest','--output','/tmp/o','--download','/tmp/d'],['--tag',tag,'--output','relative','--download','/tmp/d'],
  ['--tag',tag,'--output','/tmp/o','--download','../d'],['--tag',tag,'--output','/tmp/o','--download','/tmp/d','--repo','-malicious']])
  test('CLI rejects invalid/unknown/duplicate/missing arguments without runner calls',async()=>{let calls=0;await assert.rejects(units.main(args,()=>{calls++;throw new Error('must not execute');}),/argument|tag|output|download|repository/);assert.equal(calls,0);});
test('CLI accepted four-parameter contract refuses missing captured roles before a release',async t=>{
  const f=localFixture(t),calls=[];delete f.receipt.p7.tasks[0].captureManifest;f.saveReceipt();await assert.rejects(units.main(['--tag',tag,'--output',f.output,'--download',join(f.root,'public'),'--repo',repo],(file,args)=>{calls.push([file,args]);assert.equal(file,'git');return{status:0,stdout:f.git(args)};}),/acceptance raw controlled material identity/);
  assert(calls.length>0);assert(!calls.some(([file])=>file==='gh'||file==='curl'));
});

test('spctl accepted status is valid but arbitrary nonzero status never inherits that exemption',()=>{
  const f=provenanceFixture();f.commands[0].status=0;f.manifest.targets[0].gatekeeper={status:0,result:'accepted',outputSha256:sum('f')};assert.doesNotThrow(()=>checkProvenance(f));
  f.commands.push(command('codesign',['--verify','synthetic.app'],3));assert.throws(()=>checkProvenance(f),/assembly command status/);
});

test('each raw role and observation required by the fixed receipt rejects a missing material',()=>{
  for(const role of ['task','record','session','git','evidence','proof','trailers','mainMessage','mainTests']){const f=acceptanceFixture();delete f.receipt.p7.tasks[0][role];assert.throws(()=>checkAcceptance(f),/material role/);}
  for(const key of ['installation','finderLaunch','minimalEnvironment','runtimeClosure','daemonLifetime','reopen','archiveIntegrity','humanRecovery','humanBaselineApproval']){const f=acceptanceFixture();delete f.receipt.checks[key];assert.throws(()=>checkAcceptance(f),/acceptance checks/);}
});
test('source difference requires captured reuse but does not decode or endorse its business semantics',()=>{const f=acceptanceFixture();editRecord(f,0,r=>r.source.sha=sha('d'));f.receipt.p7.reuse='review';assert.doesNotThrow(()=>checkAcceptance(f));});
test('optional original Git sourceRef is only a preserved string, never a file to open',()=>{const f=acceptanceFixture();editRecord(f,0,r=>{r.verification.object_ref='/never/read/original/evidence.yaml';r.verification.object_sha256=sum('f');});assert.doesNotThrow(()=>checkAcceptance(f));});
test('public release missing one architecture cannot be silently patched',async t=>{const f=remoteFixture(t,{count:8,draft:false});await assert.rejects(f.invoke(),/asset set/);assert.equal(f.mutation().length,0);});
test('authenticated same-size corrupted bytes and SUMS are refused',async t=>{for(const name of ['BUILD-MANIFEST.json','SHA256SUMS']){const f=remoteFixture(t);const b=Buffer.from(f.state.bytes.get(name));b[0]^=1;f.state.bytes.set(name,b);await assert.rejects(f.invoke(),/remote asset conflict/);assert.equal(f.mutation().length,0);}});
test('anonymous alternative self-consistent bytes cannot replace local frozen release',async t=>{const f=remoteFixture(t);const alternative=new Map(f.state.bytes);alternative.set('RELEASE-NOTES.en.md',Buffer.from('different notes'));alternative.set('SHA256SUMS',Buffer.from([...alternative].filter(([n])=>n!=='SHA256SUMS').map(([n,b])=>`${hash(b)}  ${n}`).sort().join('\n')+'\n'));f.state.hook=(file,args)=>{if(file==='curl'){const path=args[args.indexOf('--output')+1],name=path.split('/').at(-1);writeFileSync(path,alternative.get(name));return{status:0,stdout:'200\nhttps://cdn.example/file\n'};}};await assert.rejects(f.invoke(),/anonymous asset bytes/);});
test('new download directory is required and existing assets/receipt are not overwritten',async t=>{const f=remoteFixture(t);mkdirSync(f.download);writeFileSync(join(f.download,'old'),'keep');await assert.rejects(f.invoke(),/new separate download/);assert.equal(readFileSync(join(f.download,'old'),'utf8'),'keep');assert.equal(f.mutation().length,0);});

import { spawnSync } from 'node:child_process';
test('real owned Git C/R blob comparison allows only readiness reuse; CLI refuses absent origin',t=>{
  const f=localFixture(t), sourceRoot=join(f.root,'source');mkdirSync(sourceRoot);
  for(const source of f.sources){const p=join(sourceRoot,source.path);mkdirSync(dirname(p),{recursive:true});writeFileSync(p,f.git(['cat-file','blob',source.blob]));}
  writeFileSync(join(sourceRoot,'support.bin'),Buffer.from([0xff,0x00,0x80,0x42]));
  const fixtureCommands=[],git=(args)=>{
    const argv=['-c','user.name=codex/gpt-6','-c','user.email=codex@towermd.invalid','-c','commit.gpgsign=false','-c','core.hooksPath=/dev/null',...args];
    const started=new Date().toISOString(),r=spawnSync('/usr/bin/git',argv,{cwd:sourceRoot,env:units.anonymousEnv(),encoding:null});
    fixtureCommands.push({file:'/usr/bin/git',args:argv,cwd:sourceRoot,started,finished:new Date().toISOString(),status:r.status,stdoutSha256:hash(r.stdout??''),stderrSha256:hash(r.stderr??''),stdoutBase64:Buffer.from(r.stdout??'').toString('base64'),stderrBase64:Buffer.from(r.stderr??'').toString('base64')});
    assert.equal(r.status,0,String(r.stderr));return r.stdout;
  };
  git(['init','--initial-branch=main']);git(['add','--',...f.sources.map(s=>s.path),'support.bin']);git(['commit','-m','synthetic acceptance source C']);
  const candidate={tag:null,commit:String(git(['rev-parse','HEAD'])).trim(),tree:String(git(['rev-parse','HEAD^{tree}'])).trim()};
  const candidateSources=String(git(['ls-tree','-r','-z','--full-tree',candidate.commit])).split('\0').filter(Boolean).map(row=>{const m=/^(\d+) blob ([a-f0-9]{40})\t(.+)$/.exec(row);return{mode:m[1],blob:m[2],path:m[3],sha256:hash(git(['cat-file','blob',m[2]]))};});
  writeFileSync(join(sourceRoot,f.readiness.path),readFileSync(join(sourceRoot,f.readiness.path),'utf8')+'\nSynthetic C to R readiness-only difference.\n');
  git(['add','--',f.readiness.path]);git(['commit','-m','synthetic frozen readiness R']);git(['tag','-a',tag,'-m','synthetic frozen tag']);
  const actual={tag,commit:String(git(['rev-parse','HEAD'])).trim(),tree:String(git(['rev-parse','HEAD^{tree}'])).trim(),tagObject:String(git(['rev-parse',`refs/tags/${tag}`])).trim()};
  const sources=String(git(['ls-tree','-r','-z','--full-tree',actual.commit])).split('\0').filter(Boolean).map(row=>{const m=/^(\d+) blob ([a-f0-9]{40})\t(.+)$/.exec(row);return{mode:m[1],blob:m[2],path:m[3],sha256:hash(git(['cat-file','blob',m[2]]))};});
  const locks=Object.fromEntries(['Cargo.lock','src-tauri/Cargo.lock','pnpm-lock.yaml'].map(k=>[k,sources.find(s=>s.path===k).sha256]));
  for(const p of [f.p.before,f.p.after]){Object.assign(p,actual,{sources});for(const b of p.builds)Object.assign(b,actual,{sources,lockfiles:locks});}
  Object.assign(f.p.manifest,actual,{lockfiles:locks});f.p.manifest.targets.forEach(t=>t.sourceTree=actual.tree);
  writeFileSync(join(f.output,'BUILD-MANIFEST.json'),JSON.stringify(f.p.manifest));
  for(const [n,v]of[['inputs-before.json',f.p.before],['inputs-after.json',f.p.after]])writeFileSync(join(f.logs,n),JSON.stringify(v));
  const names=expectedAssets('0.1.0');writeFileSync(join(f.output,'SHA256SUMS'),names.filter(n=>n!=='SHA256SUMS').map(n=>`${hash(readFileSync(join(f.output,n)))}  ${n}`).join('\n')+'\n');
  const r=f.receipt;r.identity=actual;r.assets=names.map(name=>{const b=readFileSync(join(f.output,name));return{name,size:b.length,sha256:hash(b)};});
  const ready=sources.find(s=>s.path===r.readiness.path);r.readiness.blob=ready.blob;r.readiness.sha256=ready.sha256;
  const captured=acceptanceFixture(candidate,candidateSources);captured.receipt.identity=actual;captured.receipt.assets=r.assets;captured.receipt.readiness=r.readiness;
  attachReuse(captured,{releaseIdentity:actual,releaseSources:sources,builds:f.p.before.builds,changedPaths:[f.readiness.path]});
  for(const [id,bytes]of captured.materials){const path=join(f.logs,captured.receipt.materials[id].path);mkdirSync(dirname(path),{recursive:true});writeFileSync(path,bytes);}
  writeFileSync(join(f.logs,'acceptance.json'),JSON.stringify(captured.receipt));
  const observed=units.readLocal({tag,output:f.output},git);assert.deepEqual(observed.identity,actual);
  const cliArgs=[new URL('./publish.mjs',import.meta.url).pathname,'--tag',tag,'--output',f.output,'--download',join(f.root,'public')], cliStarted=new Date().toISOString();
  const cli=spawnSync(process.execPath,cliArgs,{cwd:sourceRoot,env:units.anonymousEnv(),encoding:'utf8'});
  const cliFinished=new Date().toISOString();
  assert.equal(cli.status,1);assert.match(cli.stderr,/release git command/);assert.equal(cli.stdout,'');
  assert(!existsSync(join(f.root,'public')));assert(!existsSync(join(f.root,'public.logs')));
  t.diagnostic('owned Git commands and actual CLI: '+JSON.stringify({commands:fixtureCommands,cli:{file:process.execPath,args:cliArgs,cwd:sourceRoot,started:cliStarted,finished:cliFinished,status:cli.status,stdout:cli.stdout,stderr:cli.stderr}}));
});

test('nested verification result keeps the actual producer value instead of inventing a new enum',()=>{const f=acceptanceFixture();editRecord(f,0,r=>r.verification.result='passed');assert.doesNotThrow(()=>checkAcceptance(f));});
test('missing captured role identifies the controlled card field',()=>{const f=acceptanceFixture();delete f.receipt.p7.tasks[0].proof;assert.throws(()=>checkAcceptance(f),/p7\.T01\.proof/);});

test('acceptance assets are an exact set and do not require an undocumented row order',()=>{const f=acceptanceFixture();f.receipt.assets.reverse();assert.doesNotThrow(()=>checkAcceptance(f));f.receipt.assets[0]=clone(f.receipt.assets[1]);assert.throws(()=>checkAcceptance(f),/asset set/);});

test('API 404 cannot authorize create when release view failed with an unrelated exit',async t=>{const f=remoteFixture(t,{missing:true,count:0});f.state.hook=(file,args)=>file==='gh'&&args[0]==='release'&&args[1]==='view'?{status:2,stdout:'',stderr:'bad arguments'}:null;await assert.rejects(f.invoke(),/404|view error/);assert.equal(f.mutation().length,0);});

// Q1: original independent SPEC probes, using the same captured fixture helpers.
test('control: unchanged complete synthetic fixed receipt is accepted',()=>{
 assert.doesNotThrow(()=>checkAcceptance(acceptanceFixture()));
});

test('valid cross-card relative log refs preserve each card literal and distinct original bytes',()=>{
 const f=acceptanceFixture();
 for(const i of [0,1]) {
  editRecord(f,i,r=>r.verification.log_ref='verification.log');
  f.receipt.p7.tasks[i].bindings.verificationLog.sourceRef='verification.log';
 }
 assert.notEqual(f.receipt.p7.tasks[0].bindings.verificationLog.material,f.receipt.p7.tasks[1].bindings.verificationLog.material);
 assert.doesNotThrow(()=>checkAcceptance(f));
});

test('same shared transcript bytes may have per-card material ids without contradictory content',()=>{
 const f=acceptanceFixture();
 const first=f.receipt.p7.tasks[0].bindings.transcript;
 const second=f.receipt.p7.tasks[1].bindings.transcript;
 const bytes=Buffer.from(f.materials.get(first.material));
 f.materials.set(second.material,bytes);
 f.receipt.materials[second.material].sha256=f.receipt.materials[first.material].sha256;
 editRecord(f,1,r=>r.session.transcript_ref=first.sourceRef);
 second.sourceRef=first.sourceRef;
 assert.notEqual(first.material,second.material);
 assert.deepEqual(f.materials.get(first.material),f.materials.get(second.material));
 assert.doesNotThrow(()=>checkAcceptance(f));
});

test('control: same-card conflicting bytes for one original ref are rejected',()=>{
 const f=acceptanceFixture(); const c=f.receipt.p7.tasks[0];
 editRecord(f,0,r=>r.session.transcript_ref=r.verification.log_ref);
 c.bindings.transcript.sourceRef=c.bindings.verificationLog.sourceRef;
 assert.throws(()=>checkAcceptance(f),/contradictory sourceRef/);
});

test('control: nonempty ordered optional list is accepted and original order mismatch rejected',()=>{
 const f=acceptanceFixture(),c=f.receipt.p7.tasks[0];
 editRecord(f,0,r=>r.checkpoint_refs=['first','second']);
 c.bindings.checkpoints=[{sourceRef:'first',material:c.task},{sourceRef:'second',material:c.session}];
 assert.doesNotThrow(()=>checkAcceptance(f));
 c.bindings.checkpoints.reverse();
 assert.throws(()=>checkAcceptance(f),/sourceRef/);
});

test('same-card same sourceRef may use distinct material ids with byte-identical copies',()=>{
  const f=acceptanceFixture(),c=f.receipt.p7.tasks[0],log=c.bindings.verificationLog,transcript=c.bindings.transcript;
  const bytes=Buffer.from(f.materials.get(log.material));
  f.materials.set(transcript.material,bytes);f.receipt.materials[transcript.material].sha256=hash(bytes);
  editRecord(f,0,r=>r.session.transcript_ref=r.verification.log_ref);transcript.sourceRef=log.sourceRef;
  assert.notEqual(transcript.material,log.material);assert.deepEqual(f.materials.get(transcript.material),f.materials.get(log.material));
  assert.doesNotThrow(()=>checkAcceptance(f));
});


function checkRaw(f) { return units.checkRawAcceptance(f.receipt, { identity:f.rawIdentity ?? identity, materials:f.materials, sources:f.rawSources, builds:f.rawBuilds ?? [], sourceSnapshot:f.sourceSnapshot, packageManifest:{targets:[{target:targets[0],cliSha256:hash(f.materials.get("global-cliBinary"))}]} }); }
function editMaterial(f,id,edit) {
  const original=f.materials.get(id), value=edit(original), bytes=Buffer.isBuffer(value)?value:Buffer.from(typeof value==='string'?value:JSON.stringify(value));
  f.materials.set(id,bytes);f.receipt.materials[id].sha256=hash(bytes);
}
function editCaptured(f,index,role,edit) {
  const card=f.receipt.p7.tasks[index],id=card.captured[role];
  editMaterial(f,id,bytes=>edit(JSON.parse(bytes)));
  editMaterial(f,card.captureManifest,bytes=>{const manifest=JSON.parse(bytes);manifest.materials[role].sha256=hash(f.materials.get(id));return manifest;});
}
test('raw mapper accepts complete synthetic actual DTO roles without any process or network interface',()=>{const f=acceptanceFixture();assert.doesNotThrow(()=>checkRaw(f));});
for(const [name,change,pattern]of [
  ['missing original input manifest',f=>delete f.receipt.p7.tasks[0].captureManifest,/controlled material/],
  ['extra captured role',f=>f.receipt.p7.tasks[0].captured.unlisted='review',/fixed captured role set/],
  ['product Task ID differs from card label mapping',f=>f.receipt.p7.tasks[0].taskId='different-product-id',/product taskId/],
  ['raw session differs from record',f=>editCaptured(f,0,'session',s=>({...s,id:'another-session'})),/record session/],
  ['raw Evidence is absent',f=>editCaptured(f,0,'finish',value=>{delete value.evidence;return value;}),/incomplete mappings/],
  ['done without canonical event',f=>editCaptured(f,0,'proof',()=>({done:true})),/incomplete mappings/],
  ['raw Task not merged',f=>editCaptured(f,0,'proof',value=>{value.data.task.state='running';return value;}),/incomplete mappings/],
  ['raw Run not ended',f=>editCaptured(f,0,'proof',value=>{value.data.run.state='running';return value;}),/incomplete mappings/],
  ['raw Git tree differs',f=>editMaterial(f,f.receipt.p7.tasks[0].git,bytes=>({...JSON.parse(bytes),mainTree:sha('a')})),/incomplete mappings/],
  ['raw unknown cost converted to zero',f=>editCaptured(f,0,'session',s=>({...s,cost_usd:0})),/incomplete mappings/],
  ['raw human QA misrepresented as real person',f=>editCaptured(f,0,'observationG1',s=>({...s,performed_by:'agent_qa'})),/incomplete mappings/],
  ['raw G4 other Run',f=>editCaptured(f,0,'humanG4',s=>{s.data.fact.binding.run='another-run';return s;}),/incomplete mappings/],
  ['cross-card material binding',f=>f.receipt.p7.tasks[0].bindings.transcript.material=f.receipt.p7.tasks[1].bindings.transcript.material,/sourceRef captured bytes/],
  ['actual main message differs',f=>editMaterial(f,f.receipt.p7.tasks[0].mainMessage,()=>Buffer.from('unrelated main message')),/actual main message/],
  ['human recovery uses another project',f=>editMaterial(f,f.receipt.checks.humanRecovery.binding,bytes=>{const value=JSON.parse(bytes);value.request.project_id='another-project';return value;}),/RecoveryView binding/],
  ['human recovery view was not confirmable',f=>editMaterial(f,f.receipt.checks.humanRecovery.binding,bytes=>({...JSON.parse(bytes),can_confirm:false})),/RecoveryView binding/],
  ['human baseline request changed',f=>editMaterial(f,f.receipt.checks.humanBaselineApproval.binding,bytes=>({...JSON.parse(bytes),request_id:'different-request'})),/PreparedReview identity/],
  ['human baseline binding changed',f=>editMaterial(f,f.receipt.checks.humanBaselineApproval.binding,bytes=>{const value=JSON.parse(bytes);value.binding.draft_revision=sha('a');return value;}),/PreparedReview binding/],
  ['implementation is not an actual captured build receipt',f=>editMaterial(f,f.receipt.checks.humanRecovery.implementation,bytes=>{const value=JSON.parse(bytes);value.commands[0].file='fake-build';return value;}),/implementation identity/],
  ['frozen skill source changed',f=>{f.rawSources[0].sha256=sum('0');},/frozen skill source/],
])test(`fixed raw identities reject ${name} despite result pass`,()=>{const f=acceptanceFixture();change(f);assert.throws(()=>checkRaw(f),pattern);});
test('optional original Git reference remains inert text in the raw checker',()=>{const f=acceptanceFixture();editRecord(f,0,r=>{r.verification.object_ref='/never/open/original/evidence.yaml';r.verification.object_sha256=sum('f');});assert.doesNotThrow(()=>checkRaw(f));});
test('byte-identical duplicate transcript material IDs preserve exact captured identity',()=>{const f=acceptanceFixture(),card=f.receipt.p7.tasks[0],original=card.bindings.transcript.material,id='transcript-copy';const bytes=Buffer.from(f.materials.get(original));f.materials.set(id,bytes);f.receipt.materials[id]={path:'acceptance-evidence/transcript-copy',sha256:hash(bytes)};card.bindings.transcript.material=id;assert.doesNotThrow(()=>checkRaw(f));});
test('public entry reaches existing remote machine only after all raw identities pass',async t=>{const f=remoteFixture(t);const result=await units.publishFrozen({repo,tag,output:f.output,download:f.download},f.run);assert.equal(result.url,`https://github.com/${repo}/releases/tag/${tag}`);assert.equal(f.state.curls,9);assert.equal(f.mutation().filter(c=>c.args[1]==='edit').length,1);});
test('public entry can derive the exact existing SSH origin when repo is omitted',async t=>{const f=remoteFixture(t,{draft:false});await units.publishFrozen({tag,output:f.output,download:f.download},f.run);assert.equal(f.state.curls,9);assert.equal(f.mutation().length,0);});


function attachReuse(f,{releaseIdentity=identity,releaseSources=f.rawSources,builds=[],changedPaths=[]}={}) {
  const input=JSON.parse(f.materials.get(f.receipt.p7.input));
  const original=JSON.parse(f.materials.get('prerequisite'));
  const source={commit:input.sha,tree:original.tree},release={commit:releaseIdentity.commit,tree:releaseIdentity.tree};
  const store=(id,value)=>{const bytes=Buffer.isBuffer(value)?value:Buffer.from(typeof value==='string'?value:JSON.stringify(value));f.materials.set(id,bytes);f.receipt.materials[id]={path:`acceptance-evidence/${id}.txt`,sha256:hash(bytes)};return id;};
  const build=builds[0]??{...release,sources:releaseSources,commands:[{file:'cargo',status:0}]};
  const freshInput={...input,sha:release.commit,tag:releaseIdentity.tag,prerequisite_receipt_ref:'release-build'};
  const reuse={format:'towermd-p7-source-reuse-v1',source,release,changed_paths:changedPaths,review:store('reuse-review','Synthetic mechanism review only; actual humans and P7 have not run.'),release_input:store('release-input',freshInput),release_build:store('release-build',build),release_cli:store('release-cli',f.materials.get('global-cliBinary')),release_app:store('release-app',f.materials.get('global-appBinary')),reruns:{}};
  for(const kind of ['installation','cliContracts','nativeLoading','endToEnd']) {
    const record={kind,source_sha:release.commit,cli_sha256:freshInput.cli_sha256,app_sha256:freshInput.app_sha256,refs:[store(`reuse-${kind}-trace`,`Synthetic ${kind} trace; not actual acceptance.`)]};
    if(kind==='nativeLoading')record.harnessClasses=['claudeCode','codex'];
    if(kind==='installation'){record.sourceAsset=f.receipt.checks.installation.sourceAsset;record.sourceAssetSha256=f.receipt.assets.find(asset=>asset.name===record.sourceAsset).sha256;}
    reuse.reruns[kind]=store(`reuse-${kind}`,record);
  }
  f.receipt.p7.reuse=store('reuse',reuse);for(const key of ['humanRecovery','humanBaselineApproval'])f.receipt.checks[key].reuse='reuse';
  f.sourceSnapshot={identity:source,sources:clone(f.rawSources)};f.rawSources=releaseSources;f.rawBuilds=[build];f.rawIdentity=releaseIdentity;
  return reuse;
}
test('C source SHA is preserved when controlled runtime blobs are identical and new-package checks bind R',()=>{const f=acceptanceFixture({tag:null,commit:sha('9'),tree:sha('8')});attachReuse(f);assert.doesNotThrow(()=>checkRaw(f));assert.equal(JSON.parse(f.materials.get(f.receipt.p7.tasks[0].record)).source.sha,sha('9'));});
for(const [name,change,pattern]of [
  ['arbitrary file existence',f=>editMaterial(f,'reuse',()=>({note:'review exists'})),/reuse format/],
  ['incorrect actual C tree',f=>editMaterial(f,'reuse',bytes=>{const value=JSON.parse(bytes);value.source.tree=sha('0');return value;}),/actual C identity/],
  ['unlisted Core blob difference',f=>{f.rawSources=[...f.rawSources,{path:'crates/towermd-core/src/new.rs',mode:'100644',blob:sha('4'),sha256:sum('4')}];editMaterial(f,'reuse',bytes=>{const value=JSON.parse(bytes);value.changed_paths=['crates\/towermd-core\/src\/new.rs'];return value;});},/runtime\/source difference/],
  ['unknown source path difference',f=>{f.rawSources=[...f.rawSources,{path:'unknown-input.bin',mode:'100644',blob:sha('4'),sha256:sum('4')}];editMaterial(f,'reuse',bytes=>{const value=JSON.parse(bytes);value.changed_paths=['unknown-input.bin'];return value;});},/runtime\/source difference/],
  ['missing new native loading',f=>editMaterial(f,'reuse',bytes=>{const value=JSON.parse(bytes);delete value.reruns.nativeLoading;return value;}),/required new-package checks/],
  ['new CLI bytes changed',f=>editMaterial(f,'release-cli',()=>Buffer.from('another CLI')),/new package binary bytes/],
  ['wrong new DMG',f=>editMaterial(f,'reuse-installation',bytes=>({...JSON.parse(bytes),sourceAssetSha256:sum('0')})),/exact new DMG/],
  ['new end-to-end records old source',f=>editMaterial(f,'reuse-endToEnd',bytes=>({...JSON.parse(bytes),source_sha:sha('9')})),/endToEnd actual source/],
])test(`source reuse rejects ${name}`,()=>{const f=acceptanceFixture({tag:null,commit:sha('9'),tree:sha('8')});attachReuse(f);change(f);assert.throws(()=>checkRaw(f),pattern);});


for(const path of ['crates/towermd-core/src/lib.rs','crates/towermd/src/main.rs','src-tauri/src/lib.rs','src/App.svelte','crates/towermd-core/schema/baseline.schema.json','skills/references/session-contract.md','Cargo.lock','pnpm-lock.yaml','src-tauri/Cargo.toml','scripts/build-app.mjs','AGENTS.md','README.md'])test(`C to R refuses controlled runtime/source change at ${path}`,()=>{
  const f=acceptanceFixture({tag:null,commit:sha('9'),tree:sha('8')});attachReuse(f);
  f.rawSources=[...f.rawSources.filter(source=>source.path!==path),{path,mode:'100644',blob:sha('4'),sha256:sum('4')}];
  editMaterial(f,'reuse',bytes=>({...JSON.parse(bytes),changed_paths:[path]}));
  assert.throws(()=>checkRaw(f),/runtime\/source difference/);
});
test('C build receipt must cover the entire actual C source, including unknown inputs',()=>{const f=acceptanceFixture({tag:null,commit:sha('9'),tree:sha('8')});attachReuse(f);editCaptured(f,0,'prerequisite',value=>({...value,sources:value.sources.slice(1)}));assert.throws(()=>checkRaw(f),/complete actual C build source/);});
test('same R receipt must be the actual already validated frozen build',()=>{const f=acceptanceFixture();editCaptured(f,0,'prerequisite',value=>({...value,commands:[{file:'another-build'}]}));assert.throws(()=>checkRaw(f),/actual frozen R build receipt/);});
test('runtime reuse does not normalize a pre-release CLI version',()=>{const f=acceptanceFixture();editMaterial(f,f.receipt.p7.input,bytes=>({...JSON.parse(bytes),cli_version:'towermd 0.0.0'}));assert.throws(()=>checkRaw(f),/actual CLI version for frozen runtime/);});
