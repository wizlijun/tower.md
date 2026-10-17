import { createHash } from 'node:crypto';

const withoutComments = text => text.replace(/<!--[\s\S]*?(?:-->|$)/g,
  comment => comment.replace(/[^\n]/g, ' '));
const hasEntries = body => /^[ \t]*[-*] +\S/m.test(withoutComments(body));
const higherVersion = (left, right) => {
  const a = left.split('.').map(BigInt), b = right.split('.').map(BigInt);
  const index = a.findIndex((part, i) => part !== b[i]);
  return index !== -1 && a[index] > b[index];
};

export function parseChangelog(text, lang) {
  const title = { en: 'Unreleased', zh: '未发布', 'zh-CN': '未发布' }[lang];
  if (!title) throw new Error('invalid changelog language');
  const headings = [...withoutComments(text).matchAll(/^## ([^\n]*)(?:\n|$)/gm)];
  if (headings[0]?.[1] !== title) throw new Error('unreleased must be first');
  const versions = [], seen = new Set();
  let unreleased;
  for (const [index, heading] of headings.entries()) {
    const body = text.slice(heading.index + heading[0].length,
      headings[index + 1]?.index ?? text.length);
    if (heading[1] === title) {
      if (index !== 0) throw new Error('duplicate unreleased');
      unreleased = body;
      continue;
    }
    const match = /^v(\d+\.\d+\.\d+) — (\d{4}-\d{2}-\d{2})$/.exec(heading[1]);
    if (!match) throw new Error('invalid changelog version heading');
    const [, version, date] = match;
    if (seen.has(version)) throw new Error(`duplicate changelog version: ${version}`);
    seen.add(version);
    versions.push({ version, date, body });
  }
  return { unreleased, versions };
}

export function checkChangelogs(en, zh, phase, version) {
  assertVersions({ version }, version);
  if (!['prepare', 'frozen'].includes(phase)) throw new Error('invalid changelog phase');
  const english = parseChangelog(en, 'en'), chinese = parseChangelog(zh, 'zh');
  const sequence = log => log.versions.map(({ version, date }) => ({ version, date }));
  if (JSON.stringify(sequence(english)) !== JSON.stringify(sequence(chinese)))
    throw new Error('changelog history mismatch');
  if (phase === 'prepare') {
    if (!hasEntries(english.unreleased) || !hasEntries(chinese.unreleased))
      throw new Error('unreleased entries required');
    if (english.versions.some(release => release.version === version))
      throw new Error('target version already released');
  } else {
    for (const log of [english, chinese]) {
      if (log.versions[0]?.version !== version || !hasEntries(log.versions[0].body) ||
          log.versions.some(release => higherVersion(release.version, version)))
        throw new Error('frozen target entries required');
    }
  }
}

export function assertVersions(values, expected) {
  if (typeof expected !== 'string' || expected.trim() !== expected ||
      !/^\d+\.\d+\.\d+$/.test(expected) || !Object.keys(values).length)
    throw new Error('invalid version input');
  for (const [name, value] of Object.entries(values))
    if (value !== expected) throw new Error(`version mismatch: ${name}`);
}

export function assertUnsigned(c) {
  if (c.bundle?.createUpdaterArtifacts !== false || c.plugins?.updater !== undefined)
    throw new Error('updater forbidden');
  if (c.bundle?.macOS?.signingIdentity !== '-') throw new Error('ad-hoc required');
  if (JSON.stringify(c.bundle?.externalBin) !== JSON.stringify(['binaries/towermd']))
    throw new Error('fixed CLI missing');
  if (c.bundle.macOS.providerShortName != null) throw new Error('notarization forbidden');
}

export function unsignedEnv(env, pnpmBin) {
  const out = {};
  for (const key of ['HOME', 'TMPDIR', 'LANG', 'LC_ALL', 'SDKROOT', 'DEVELOPER_DIR',
    'MACOSX_DEPLOYMENT_TARGET', 'CARGO_HOME', 'RUSTUP_HOME'])
    if (env[key] !== undefined) out[key] = env[key];
  out.PATH = `${pnpmBin}:${env.HOME}/.cargo/bin:${env.PATH ?? '/usr/bin:/bin:/usr/sbin:/sbin'}`;
  return out;
}

export function expectedAssets(v) {
  assertVersions({ v }, v);
  return ['aarch64', 'x86_64'].flatMap(a => [
    `tower.md-${v}-${a}.app.zip`, `tower.md-${v}-${a}.dmg`,
  ]).concat([`towermd-skills-${v}.tar.gz`, 'BUILD-MANIFEST.json',
    'RELEASE-NOTES.en.md', 'RELEASE-NOTES.zh-CN.md', 'SHA256SUMS']).sort();
}

export function assertAssetSet(names, v) {
  if (new Set(names).size !== names.length ||
      JSON.stringify([...names].sort()) !== JSON.stringify(expectedAssets(v)))
    throw new Error('asset set mismatch');
}

export function verifySums(text, files) {
  const rows = text.trim().split('\n'), seen = new Set();
  for (const row of rows) {
    const m = /^([a-f0-9]{64})  ([A-Za-z0-9_.-]+)$/.exec(row);
    if (!m || m[2] === 'SHA256SUMS' || seen.has(m[2]) || !files.has(m[2]))
      throw new Error('invalid checksum set');
    seen.add(m[2]);
    if (createHash('sha256').update(files.get(m[2])).digest('hex') !== m[1])
      throw new Error(`checksum mismatch: ${m[2]}`);
  }
  if (seen.size !== files.size) throw new Error('incomplete checksum set');
}

export function uploadAction(local, remote) {
  if (!remote) return 'upload';
  if (local.size === remote.size && local.sha256 === remote.sha256) return 'reuse';
  throw new Error(`remote asset conflict: ${local.name}`);
}
