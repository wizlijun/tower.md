import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import {
  parseChangelog, checkChangelogs, assertVersions, assertUnsigned, unsignedEnv,
  expectedAssets, assertAssetSet, verifySums, uploadAction,
} from './core.mjs';

const en = '# Changelog\n\n## Unreleased\n\n- Open projects.\n';
const zh = '# 更新日志\n\n## 未发布\n\n- 打开项目。\n';
const history = '\n## v0.0.1 — 2026-10-16\n\n- Previous change.\n';
const frozenEn = '# Changelog\n\n## Unreleased\n\n## v0.1.0 — 2026-10-17\n\n- Open projects.\n' + history;
const frozenZh = '# 更新日志\n\n## 未发布\n\n## v0.1.0 — 2026-10-17\n\n* 打开项目。\n' + history;
const config = () => ({ bundle: { createUpdaterArtifacts: false,
  externalBin: ['binaries/towermd'], macOS: { signingIdentity: '-' } }, plugins: {} });
const digest = bytes => createHash('sha256').update(bytes).digest('hex');
const files = new Map([['a.dmg', Buffer.from('app')], ['notes.md', Buffer.from('notes')]]);
const sums = [...files].map(([name, bytes]) => `${digest(bytes)}  ${name}`).join('\n') + '\n';

for (const [lang, input] of [['en', en], ['zh', zh], ['zh-CN', zh]]) {
  test(`parseChangelog preserves section bodies for ${lang}`, () => {
    assert.deepEqual(parseChangelog(input + history, lang), {
      unreleased: input.slice(input.indexOf('\n\n- ') + 1) + '\n',
      versions: [{ version: '0.0.1', date: '2026-10-16', body: '\n- Previous change.\n' }],
    });
  });
}
for (const [name, input] of [
  ['missing unreleased', '# Changelog\n' + history],
  ['duplicate unreleased', en + '\n## Unreleased\n- Duplicate\n'],
  ['late unreleased', '# Changelog\n' + history + '\n## Unreleased\n- Late\n'],
  ['another section before unreleased', '# Changelog\n## Other\n\n## Unreleased\n- A\n'],
  ['duplicate version', en + history + history],
  ['missing v prefix', en + '\n## 0.0.1 — 2026-10-16\n- A\n'],
  ['incorrect separator', en + '\n## v0.0.1 - 2026-10-16\n- A\n'],
  ['incorrect date format', en + '\n## v0.0.1 — 2026-1-16\n- A\n'],
  ['prerelease title', en + '\n## v0.1.0-beta — 2026-10-16\n- A\n'],
]) {
  test(`parseChangelog rejects ${name}`, () => assert.throws(() => parseChangelog(input, 'en')));
}
test('parseChangelog ignores headings inside HTML comments', () => {
  assert.equal(parseChangelog(en + '<!--\n## Unreleased\n-->\n', 'en').versions.length, 0);
});
test('parseChangelog rejects an unsupported language', () => assert.throws(() => parseChangelog(en, 'fr')));

test('prepare accepts bilingual entries and identical history', () => {
  assert.equal(checkChangelogs(en + history, zh + history, 'prepare', '0.1.0'), undefined);
});
for (const [name, english, chinese] of [
  ['empty English', '# Changelog\n## Unreleased\n', zh],
  ['empty Chinese', en, '# 更新日志\n## 未发布\n'],
  ['only comments', '# Changelog\n## Unreleased\n<!-- - Hidden. -->\n', zh],
  ['only category headings', '# Changelog\n## Unreleased\n### Added\n', zh],
  ['empty bullet', '# Changelog\n## Unreleased\n- <!-- Hidden. -->\n', zh],
  ['different version sequences', en + history, zh],
  ['different history dates', en + history, zh + history.replace('2026-10-16', '2026-10-15')],
  ['different history order', en + history + '\n## v0.0.0 — 2026-10-15\n- Old\n', zh + '\n## v0.0.0 — 2026-10-15\n- Old\n' + history],
  ['target already exists', en + '\n## v0.1.0 — 2026-10-17\n- A\n', zh + '\n## v0.1.0 — 2026-10-17\n- 甲\n'],
]) {
  test(`prepare rejects ${name}`, () => assert.throws(() => checkChangelogs(english, chinese, 'prepare', '0.1.0')));
}
test('frozen accepts empty unreleased and bilingual target entries', () => {
  assert.equal(checkChangelogs(frozenEn, frozenZh, 'frozen', '0.1.0'), undefined);
});
test('rotated empty unreleased cannot prepare the next version', () => {
  assert.throws(() => checkChangelogs(frozenEn, frozenZh, 'prepare', '0.1.1'));
});
for (const [name, english, chinese, version] of [
  ['no released version', en, zh, '0.1.0'],
  ['target is not the top release', frozenEn, frozenZh, '0.0.1'],
  ['empty target English', frozenEn.replace('- Open projects.', '### Added'), frozenZh, '0.1.0'],
  ['empty target Chinese', frozenEn, frozenZh.replace('* 打开项目。', '<!-- - Hidden. -->'), '0.1.0'],
  ['historical date mismatch', frozenEn, frozenZh.replace('2026-10-16', '2026-10-15'), '0.1.0'],
]) {
  test(`frozen rejects ${name}`, () => assert.throws(() => checkChangelogs(english, chinese, 'frozen', version)));
}
test('changelog checks reject an unknown phase', () => assert.throws(() => checkChangelogs(en, zh, 'other', '0.1.0')));
test('changelog checks reject an invalid target version', () => assert.throws(() => checkChangelogs(en, zh, 'prepare', 'v0.1.0')));

test('all version sources must match the requested version', () => {
  assert.equal(assertVersions({ core: '0.1.0', app: '0.1.0', frontend: '0.1.0' }, '0.1.0'), undefined);
  assert.throws(() => assertVersions({ core: '0.1.0', app: '0.0.0' }, '0.1.0'), /app/);
});
test('version validation rejects empty sources', () => assert.throws(() => assertVersions({}, '0.1.0')));
for (const version of ['v0.1.0', '0.1', '0.1.0-beta', '', '0.1.0\n']) {
  test(`version validation rejects ${JSON.stringify(version)}`, () => assert.throws(() => assertVersions({ app: version }, version)));
}

test('unsigned configuration accepts ad-hoc and the fixed sidecar', () => assert.equal(assertUnsigned(config()), undefined));
for (const [name, change] of [
  ['updater artifacts', c => { c.bundle.createUpdaterArtifacts = true; }],
  ['missing updater setting', c => { delete c.bundle.createUpdaterArtifacts; }],
  ['updater plugin', c => { c.plugins.updater = {}; }],
  ['publisher identity', c => { c.bundle.macOS.signingIdentity = 'Developer ID'; }],
  ['missing ad-hoc', c => { delete c.bundle.macOS.signingIdentity; }],
  ['notarization provider', c => { c.bundle.macOS.providerShortName = 'provider'; }],
  ['missing sidecar', c => { c.bundle.externalBin = []; }],
  ['another sidecar', c => { c.bundle.externalBin.push('binaries/other'); }],
  ['missing bundle', c => { delete c.bundle; }],
]) {
  test(`unsigned configuration rejects ${name}`, () => {
    const c = config(); change(c); assert.throws(() => assertUnsigned(c));
  });
}
test('unsignedEnv copies only allowed inputs and prepends explicit tool paths', () => {
  const allowed = Object.fromEntries(['HOME', 'TMPDIR', 'LANG', 'LC_ALL', 'SDKROOT', 'DEVELOPER_DIR',
    'MACOSX_DEPLOYMENT_TARGET', 'CARGO_HOME', 'RUSTUP_HOME'].map(key => [key, `/test/${key}`]));
  const input = { ...allowed, PATH: '/usr/bin', APPLE_PASSWORD: 'synthetic-sentinel',
    TAURI_SIGNING_PRIVATE_KEY: 'synthetic-sentinel', TAURI_CONFIG: 'bad',
    GITHUB_TOKEN: 'synthetic-sentinel', CARGO_ENCODED_RUSTFLAGS: 'bad' };
  const before = { ...input };
  assert.deepEqual(unsignedEnv(input, '/test/pnpm/bin'), {
    ...allowed, PATH: '/test/pnpm/bin:/test/HOME/.cargo/bin:/usr/bin',
  });
  assert.deepEqual(input, before);
});
test('unsignedEnv supplies the defined default system PATH', () => {
  assert.deepEqual(unsignedEnv({ HOME: '/test/home' }, '/test/pnpm'), {
    HOME: '/test/home', PATH: '/test/pnpm:/test/home/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin',
  });
});
test('unsignedEnv never reads disallowed secret properties', () => {
  const input = { HOME: '/test/home' };
  Object.defineProperty(input, 'APPLE_PASSWORD', { enumerable: true, get() { throw new Error('secret read'); } });
  assert.doesNotThrow(() => unsignedEnv(input, '/test/pnpm'));
});

const assets = [
  'BUILD-MANIFEST.json', 'RELEASE-NOTES.en.md', 'RELEASE-NOTES.zh-CN.md', 'SHA256SUMS',
  'tower.md-0.1.0-aarch64.app.zip', 'tower.md-0.1.0-aarch64.dmg',
  'tower.md-0.1.0-x86_64.app.zip', 'tower.md-0.1.0-x86_64.dmg', 'towermd-skills-0.1.0.tar.gz',
];
test('expectedAssets returns exactly nine sorted release basenames', () => assert.deepEqual(expectedAssets('0.1.0'), assets));
test('expectedAssets rejects an invalid version', () => assert.throws(() => expectedAssets('../0.1.0')));
test('assertAssetSet accepts the exact set in any order', () => assert.equal(assertAssetSet([...assets].reverse(), '0.1.0'), undefined));
for (const [name, names] of [
  ['missing asset', assets.slice(1)], ['duplicate asset', [...assets, assets[0]]],
  ['updater metadata', [...assets, 'latest.json']], ['signature', [...assets, 'app.sig']],
  ['wrong version', assets.map(name => name.replace('0.1.0', '0.0.0'))],
]) {
  test(`assertAssetSet rejects ${name}`, () => assert.throws(() => assertAssetSet(names, '0.1.0')));
}

test('verifySums accepts exact lowercase checksums and buffers', () => assert.equal(verifySums(sums, files), undefined));
test('verifySums rejects one changed byte', () => {
  assert.throws(() => verifySums(sums, new Map([...files].map(([name, bytes]) => [name, name === 'a.dmg' ? Buffer.from('App') : bytes]))), /checksum mismatch: a.dmg/);
});
for (const [name, text, buffers] of [
  ['missing row', sums.split('\n')[0] + '\n', files],
  ['duplicate row', sums + sums.split('\n')[0] + '\n', files],
  ['parent traversal', sums.replace('a.dmg', '../a.dmg'), files],
  ['forward separator', sums.replace('a.dmg', 'dir/a.dmg'), files],
  ['backslash separator', sums.replace('a.dmg', 'dir\\a.dmg'), files],
  ['extra row', sums + `${digest(Buffer.from('extra'))}  extra.dmg\n`, files],
  ['extra file', sums, new Map([...files, ['extra.dmg', Buffer.from('extra')]])],
  ['uppercase hash', sums.replace(digest(files.get('a.dmg')), digest(files.get('a.dmg')).toUpperCase()), files],
  ['one-space delimiter', sums.replace('  a.dmg', ' a.dmg'), files],
  ['self-referencing checksum', `${digest(Buffer.from('sum'))}  SHA256SUMS\n`, new Map([['SHA256SUMS', Buffer.from('sum')]])],
  ['empty checksum set', '', new Map()],
]) {
  test(`verifySums rejects ${name}`, () => assert.throws(() => verifySums(text, buffers)));
}

test('uploadAction uploads a missing remote asset', () => {
  assert.equal(uploadAction({ name: 'a.dmg', size: 3, sha256: digest(Buffer.from('app')) }, null), 'upload');
});
test('uploadAction reuses byte-identical remote content', () => {
  const local = { name: 'a.dmg', size: 3, sha256: digest(Buffer.from('app')) };
  assert.equal(uploadAction(local, { ...local }), 'reuse');
});
for (const [name, remote] of [
  ['hash conflict', { name: 'a.dmg', size: 3, sha256: digest(Buffer.from('App')) }],
  ['size conflict', { name: 'a.dmg', size: 4, sha256: digest(Buffer.from('app')) }],
]) {
  test(`uploadAction refuses ${name}`, () => {
    assert.throws(() => uploadAction({ name: 'a.dmg', size: 3, sha256: digest(Buffer.from('app')) }, remote), /remote asset conflict: a.dmg/);
  });
}

test('frozen rejects a historical version higher than the target', () => {
  assert.throws(() => checkChangelogs(
    frozenEn.replace('v0.0.1', 'v0.2.0'), frozenZh.replace('v0.0.1', 'v0.2.0'), 'frozen', '0.1.0'));
});
test('frozen compares the highest version numerically', () => {
  assert.doesNotThrow(() => checkChangelogs(
    frozenEn.replace('v0.1.0', 'v0.10.0').replace('v0.0.1', 'v0.9.0'),
    frozenZh.replace('v0.1.0', 'v0.10.0').replace('v0.0.1', 'v0.9.0'), 'frozen', '0.10.0'));
});
