import test, { after } from 'node:test';
import assert from 'node:assert/strict';
import { assertRemoteReady, expectedAssets } from './core.mjs';
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
  .replace('if (process.argv[1] &&', 'if (false &&');
const privateDir = realpathSync(mkdtempSync(join(tmpdir(), 'towermd-publish-private-test-')));
after(() => rmSync(privateDir, { recursive: true, force: true }));
const privateModule = join(privateDir, 'publish-private.mjs');
writeFileSync(privateModule, source + '\nexport {parseOrigin, parseRefs, anonymousEnv, checkUrl, checkProvenance, checkAcceptance, readLocal};');
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
  const manifest = { ...identity, lockfiles, tools: Object.fromEntries(builds.map(b => [b.target, b.tools])), targets: targets.map(target => ({ target, sourceTree: identity.tree, gatekeeper: { status: 3, result: 'rejected', outputSha256: sum('f') } })) };
  const before = { ...identity, sources, builds }, after = clone(before);
  const commands = builds.map(b => command('spctl', ['--assess','--type','execute','--verbose=4', b.app], 3));
  return { before, after, commands, manifest, sources };
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
]) test(`provenance rejects ${name}`, () => { const f = provenanceFixture(); change(f); assert.throws(() => checkProvenance(f), /release/); });

function acceptanceFixture() {
  const materials = new Map(), listing = {};
  const material = (id, bytes = Buffer.from(`synthetic ${id}`)) => { listing[id] = { path: `acceptance-evidence/${id}.txt`, sha256: hash(bytes) }; materials.set(id, bytes); return id; };
  const assets = expectedAssets('0.1.0').map(name => ({ name, size: 1, sha256: sum('d') }));
  const receipt = { format: 'towermd-release-acceptance-v1', recordedAt: '2026-10-17T00:00:00.000Z', recordedBy: 'process:towermd-release', identity, assets: clone(assets),
    readiness: { path: 'docs/verification/2026-10-17-release-readiness.md', blob: sha('d'), sha256: sum('e'), review: material('review') }, materials: listing,
    p7: { input: material('input'), tasks: [] },
    architectures: targets.map((target, i) => ({ target, level: i ? 'static' : 'device', refs: [material(`arch${i}`)] })),
    checks: Object.fromEntries(['installation','finderLaunch','minimalEnvironment','runtimeClosure','daemonLifetime','reopen','archiveIntegrity','humanRecovery','humanBaselineApproval'].map(k => [k, { refs: [material(k)] }])) };
  receipt.checks.installation.sourceAsset = 'tower.md-0.1.0-aarch64.dmg'; receipt.checks.installation.installedTreeSha256 = sum('f');
  receipt.checks.daemonLifetime.before = { pid: 42, instance: 'instance' }; receipt.checks.daemonLifetime.after = { pid: 42, instance: 'instance' };
  for (const k of ['humanRecovery','humanBaselineApproval']) Object.assign(receipt.checks[k], { eventId: k, actor: 'human:fixture', implementationTree: identity.tree, event: material(k + '-event'), binding: material(k + '-binding'), implementation: material(k + '-implementation') });
  const classes = ['claudeCode','codex','claudeCode','codex','claudeCode','codex','claudeCode','codex','claudeCode','codex'];
  for (let i = 0; i < 10; i++) {
    const card = `T${String(i + 1).padStart(2, '0')}`, harness = classes[i] === 'codex' ? 'actual-codex' : 'actual-claude';
    const record = { task: card, attempt: 1, result: 'pass', source: { sha: identity.commit, tag, cli_sha256: sum('d'), app_sha256: sum('e'), skill_hashes: { claim: sum('f') } },
      session: { kind: 'human_opened', harness, version: `version-${i}`, id: `session-${i}`, skill_loaded: 'self_report', transcript_ref: `/absolute/original/transcript-${i}`, cost_usd: null },
      binding: { project: 'p', run: `run-${i}`, host: 'h', worktree: `/original/w${i === 9 ? 1 : i % 3}`, branch: `branch-${i}`, module: ['ledger','reports','filters'][i === 9 ? 1 : i % 3], fences: [{ opaque: 'preserved' }] },
      verification: { commit: sha('d'), tree: sha('e'), evidence_id: `evidence-${i}`, captured_source_ref: `/original/evidence-${i}`, object_ref: null, object_sha256: '', commands_hash: sum('d'), log_ref: `/original/log-${i}`, log_sha256: hash(Buffer.from(`synthetic log${i}`)), result: 'pass', entered_at: '2026-10-17T00:00:00Z', deadline: '2026-10-17T00:01:00Z' },
      main: { authority_ref: 'refs/heads/main', commit: sha('f'), tree: sha('e'), trailers: { opaque: 'CLI original' }, proof_ref: `/original/proof-${i}`, merge_done_eid: `merge-${i}`, task_state: 'merged', run_state: 'ended', test_count: 1, test_exit: 0, test_log_ref: `/original/main-tests-${i}` },
      human_refs: [], checkpoint_refs: [], failure_refs: [], missing: [] };
    const roles = Object.fromEntries(['task','session','git','evidence','proof','trailers','mainMessage','mainTests'].map(role => [role, material(`${card}-${role}`)]));
    const bindings = {};
    for (const [k, sourceRef, id] of [ ['capturedSource',record.verification.captured_source_ref,roles.evidence], ['proof',record.main.proof_ref,roles.proof], ['mainTests',record.main.test_log_ref,roles.mainTests], ['transcript',record.session.transcript_ref,material(`${card}-transcript`)], ['verificationLog',record.verification.log_ref,material(`${card}-log`, Buffer.from(`synthetic log${i}`))] ]) bindings[k] = { sourceRef, material: id };
    for (const k of ['human','checkpoints','failures']) bindings[k] = [];
    const recordId = material(`${card}-record`, Buffer.from(JSON.stringify(record)));
    receipt.p7.tasks.push({ card, taskId: `product-task-${i}`, harnessClass: classes[i], record: recordId, ...roles, bindings });
  }
  return { receipt, materials, assets, readiness: { path: receipt.readiness.path, blob: sha('d'), sha256: sum('e') } };
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
  const blobs = new Map(), sourcePaths = p.sources.map(s => s.path).concat(['CHANGELOG.md','CHANGELOG.zh-CN.md',f.readiness.path]);
  const fileBytes = path => Buffer.from(path === 'CHANGELOG.md' ? '# Changelog\n\n## Unreleased\n\n## v0.1.0 — 2026-10-17\n\n- Frozen English.\n' : path === 'CHANGELOG.zh-CN.md' ? '# 更新日志\n\n## 未发布\n\n## v0.1.0 — 2026-10-17\n\n- 冻结中文。\n' : `synthetic frozen ${path}`);
  const sources = sourcePaths.sort().map(path => { const bytes = fileBytes(path), blob = createHash('sha1').update(`blob ${bytes.length}\0`).update(bytes).digest('hex'); blobs.set(blob, bytes); return { path, mode:'100644', blob, sha256: hash(bytes) }; });
  const locks = Object.fromEntries(['Cargo.lock','src-tauri/Cargo.lock','pnpm-lock.yaml'].map(k => [k, sources.find(s => s.path === k).sha256]));
  p.before.sources = sources; for (const b of p.before.builds) { b.sources = sources; b.lockfiles = locks; } p.after = clone(p.before); p.manifest.lockfiles = locks;
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

test('public entry refuses unimplemented P7 raw mappings before any network command or mutation', async t => {
  const f = localFixture(t), calls=[];
  const run=(file,args,options)=>{calls.push([file,args]);assert.equal(file,'git');return {status:0,stdout:f.git(args),stderr:''};};
  await assert.rejects(units.publishFrozen({repo,tag,output:f.output,download:join(f.root,'public')},run), /unwired.*P7.*input.*task.*session.*Evidence.*proof.*human/);
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
