import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { appendFileSync, copyFileSync, existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, readlinkSync, realpathSync, unlinkSync, writeFileSync } from 'node:fs';
import { basename, dirname, isAbsolute, join, posix, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { assertCliSigningEquivalent } from './macho-cli.mjs';
import { assertAssetSet, assertVersions, checkChangelogs, expectedAssets, parseChangelog, unsignedEnv, verifySums } from './core.mjs';

const targets = ['aarch64-apple-darwin', 'x86_64-apple-darwin'];
const architectures = ['arm64', 'x86_64'];
const locks = ['Cargo.lock', 'src-tauri/Cargo.lock', 'pnpm-lock.yaml'];
const skills = ['init', 'baseline', 'claim', 'ask', 'propose', 'finish'];
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const text = bytes => new TextDecoder('utf-8', { fatal: true }).decode(bytes);
const inside = (root, path) => { const r = relative(root, path); return r === '' || (!isAbsolute(r) && r !== '..' && !r.startsWith(`..${sep}`)); };
const runCommand = (file, args, options) => spawnSync(file, args, { ...options, maxBuffer: 128 * 1024 * 1024 });
const fail = field => { throw new Error(`invalid ${field}`); };
function checked(run, file, args, options = {}) {
  const r = run(file, args, { shell: false, ...options });
  if (r?.error || r?.status !== 0) throw new Error(`${basename(file)} ${args[0]} exit=${r?.status ?? 'unknown'}`);
  return r;
}
function regular(path, executable = false) {
  let stat;
  try { stat = lstatSync(path); } catch { fail(`file: ${basename(path)}`); }
  if (!isAbsolute(path) || !stat.isFile() || realpathSync(path) !== path || (executable && !(stat.mode & 0o111))) fail(`file: ${basename(path)}`);
  return stat;
}
function directory(path) {
  if (!isAbsolute(path) || !lstatSync(path).isDirectory() || realpathSync(path) !== path) fail('directory');
}
function json(bytes, field) {
  try { return JSON.parse(String(bytes)); } catch { fail(field); }
}

// lstat every entry, preserve links as links, and never read a link outside the bundle.
export function bundleTree(app) {
  directory(app);
  const result = [{ path: '.', mode: lstatSync(app).mode & 0o7777, type: 'directory' }];
  const visit = (directoryPath, prefix) => {
    for (const name of readdirSync(directoryPath).sort()) {
      if (/[\x00-\x1f\x7f]/.test(name)) fail('bundle name');
      const path = join(directoryPath, name), rel = prefix + name, s = lstatSync(path);
      const item = { path: rel, mode: s.mode & 0o7777 };
      if (s.isSymbolicLink()) {
        const target = readlinkSync(path);
        let final;
        try { final = realpathSync(path); } catch { fail('bundle link'); }
        if (isAbsolute(target) || !inside(app, final)) fail('bundle link escape');
        result.push({ ...item, type: 'link', target });
      } else if (s.isDirectory()) {
        result.push({ ...item, type: 'directory' }); visit(path, `${rel}/`);
      } else if (s.isFile()) result.push({ ...item, type: 'file', size: s.size, sha256: hash(readFileSync(path)) });
      else fail('bundle entry');
    }
  };
  visit(app, ''); return result;
}

function libraries(binary, app, run) {
  const output = String(checked(run, 'otool', ['-L', binary]).stdout ?? '');
  const lines = output.trimEnd().split('\n');
  if (lines.shift() !== `${binary}:` || !lines.length) fail('otool dependencies');
  const system = path => posix.isAbsolute(path) && posix.normalize(path) === path &&
    (path.startsWith('/usr/lib/') || path.startsWith('/System/Library/'));
  const local = path => {
    let full;
    if (path.startsWith('@executable_path/')) full = resolve(dirname(binary), path.slice(17));
    else if (path.startsWith('@loader_path/')) full = resolve(dirname(binary), path.slice(13));
    else fail('dylib path');
    if (!inside(app, full)) fail('dylib escape');
    return full;
  };
  const load = String(checked(run, 'otool', ['-l', binary]).stdout ?? '');
  const rpaths = [];
  for (const block of load.split(/^Load command \d+\n/m).slice(1)) {
    if (!/^\s*cmd LC_RPATH\s*$/m.test(block)) continue;
    const match = /^\s*path (.+) \(offset \d+\)\s*$/m.exec(block);
    if (!match) fail('LC_RPATH');
    rpaths.push(local(match[1]));
  }
  const dependencies = lines.map(line => {
    const m = /^\s+(.+) \(compatibility version [^,]+, current version [^)]+\)$/.exec(line);
    if (!m) fail('otool dependency');
    const path = m[1];
    if (system(path)) return path;
    const candidates = path.startsWith('@rpath/')
      ? rpaths.map(base => resolve(base, path.slice(7))) : [local(path)];
    if (!candidates.some(candidate => {
      try { return inside(app, realpathSync(candidate)) && lstatSync(realpathSync(candidate)).isFile(); } catch { return false; }
    })) fail('missing package dylib');
    return path;
  });
  return { dependencies, rpaths };
}

export function inspectApp({ app, target, version }, run = runCommand) {
  assertVersions({ version }, version);
  if (!targets.includes(target)) fail('target');
  directory(app);
  const before = bundleTree(app), plistPath = join(app, 'Contents/Info.plist');
  regular(plistPath);
  const plist = json(checked(run, 'plutil', ['-convert', 'json', '-o', '-', plistPath]).stdout, 'plist JSON');
  if (plist.CFBundleShortVersionString !== version || plist.CFBundleVersion !== version || plist.CFBundleExecutable !== 'towermd-app') fail('plist version/executable');
  const main = join(app, 'Contents/MacOS/towermd-app'), cli = join(app, 'Contents/MacOS/towermd');
  const binaries = [];
  for (const binary of [main, cli]) {
    regular(binary, true);
    const architecture = String(checked(run, 'lipo', ['-archs', binary]).stdout ?? '').trim();
    if (architecture !== architectures[targets.indexOf(target)]) fail('architecture');
    const dylibs = libraries(binary, app, run);
    const sign = checked(run, 'codesign', ['-dv', '--verbose=4', binary === main ? app : cli]);
    const signature = String(sign.stdout ?? '') + String(sign.stderr ?? '');
    if (!/^Signature=adhoc$/m.test(signature) || !/^TeamIdentifier=not set$/m.test(signature) || /^Authority=/m.test(signature)) fail('ad-hoc signature');
    binaries.push({ name: basename(binary), architecture, sha256: hash(readFileSync(binary)), ...dylibs });
  }
  checked(run, 'codesign', ['--verify', '--deep', '--strict', app]);
  const policy = run('spctl', ['--assess', '--type', 'execute', '--verbose=4', app], { shell: false });
  const assessment = String(policy?.stderr ?? '') + String(policy?.stdout ?? '');
  // spctl(8): 3 is a policy denial; 1/2/4 and tool errors are not an expected denial.
  const firstLine = assessment.split('\n')[0];
  if (policy?.error || !((policy?.status === 3 && firstLine === `${app}: rejected`) || (policy?.status === 0 && firstLine === `${app}: accepted`))) fail('spctl assessment');
  let cliVersion = { scope: 'static-only; CLI not executed on this host' };
  const host = process.arch === 'arm64' ? targets[0] : process.arch === 'x64' ? targets[1] : null;
  if (target === host) {
    if (String(checked(run, cli, ['--version']).stdout ?? '').trim() !== `towermd ${version}`) fail('CLI version');
    cliVersion = { scope: 'host CLI --version', version };
  }
  if (JSON.stringify(bundleTree(app)) !== JSON.stringify(before)) fail('App changed during inspection');
  return { target, scope: 'static bundle checks; no App launch', binaries, cliVersion,
    frontend: 'embedded bytes not independently inspected; assemble checks frozen build source chain',
    gatekeeper: { status: policy.status, result: policy.status === 3 ? 'rejected' : 'accepted', outputSha256: hash(assessment) }, tree: before };
}

export function inspectDmg({ dmg, app }, run = runCommand) {
  regular(dmg); const before = bundleTree(app);
  checked(run, 'hdiutil', ['verify', dmg]);
  const owned = []; let error, result;
  try {
    const attached = run('hdiutil', ['attach', '-readonly', '-nobrowse', '-plist', dmg], { shell: false });
    const plist = json(checked(run, 'plutil', ['-convert', 'json', '-o', '-', '-'], { input: attached?.stdout ?? '' }).stdout, 'attach plist');
    const entities = plist?.['system-entities'];
    if (!Array.isArray(entities) || !entities.length) fail('attach entities');
    for (const entity of entities) {
      const m = /^\/dev\/(disk\d+)(?:s\d+)*$/.exec(entity['dev-entry'] ?? '');
      if (m && !owned.includes(`/dev/${m[1]}`)) owned.push(`/dev/${m[1]}`);
    }
    if (attached?.error || attached?.status !== 0) throw new Error(`hdiutil attach exit=${attached?.status ?? 'unknown'}`);
    if (owned.length !== 1 || entities.some(e => !/^\/dev\/disk\d+(?:s\d+)*$/.test(e['dev-entry'] ?? ''))) fail('attach device ownership');
    const mounted = entities.filter(e => e['mount-point'] !== undefined);
    if (mounted.length !== 1) fail('attach mount count');
    const mount = mounted[0]['mount-point']; directory(mount);
    const apps = readdirSync(mount).filter(name => name.endsWith('.app'));
    if (apps.length !== 1 || apps[0] !== basename(app)) fail('DMG App count/name');
    if (JSON.stringify(bundleTree(join(mount, apps[0]))) !== JSON.stringify(before)) fail('DMG bundle differs');
    result = { verified: true, treeSha256: hash(JSON.stringify(before)) };
  } catch (caught) { error = caught; }
  finally {
    const cleanup = [];
    for (const device of owned) {
      try { checked(run, 'hdiutil', ['detach', device]); } catch (caught) { cleanup.push(caught.message); }
    }
    if (cleanup.length) error = new Error(`${error?.message ?? 'DMG checked'}; cleanup failed: ${cleanup.join(', ')}`);
  }
  if (error) throw error;
  return result;
}

const archivePath = name => {
  if (typeof name !== 'string' || !name || name.startsWith('/') || /[\\\x00-\x1f\x7f]/.test(name) || name.split('/').some(p => p === '..' || p === '.')) fail('archive path');
  return name;
};
function linksStayInside(entries, prefix) {
  const map = new Map(entries.map(e => [e.path.replace(/\/$/, ''), e]));
  const follow = (path, seen = new Set()) => {
    const parts = path.split('/');
    for (let i = 1; i <= parts.length; i++) {
      const parent = parts.slice(0, i).join('/'), entry = map.get(parent);
      if (entry?.type === 'link') {
        if (seen.has(parent)) fail('archive link cycle');
        const next = new Set(seen); next.add(parent);
        if (entry.target.startsWith('/') || /[\\\x00-\x1f\x7f]/.test(entry.target)) fail('archive link');
        const target = posix.normalize(posix.join(posix.dirname(parent), entry.target, ...parts.slice(i)));
        if (!target.startsWith(prefix) || target === prefix.slice(0, -1)) fail('archive link escape');
        return follow(target, next);
      }
      if (i < parts.length && entry && entry.type !== 'directory') fail('archive parent');
    }
    return path;
  };
  for (const entry of entries) {
    for (let parent = posix.dirname(entry.path.replace(/\/$/, '')); parent !== '.'; parent = posix.dirname(parent))
      if (map.get(parent)?.type === 'link') fail('archive symlink parent');
    const final = follow(entry.path.replace(/\/$/, ''));
    if (entry.type === 'link' && !map.has(final)) fail('archive dangling link');
  }
}

// Only the USTAR and PAX records emitted by git archive; never extract unknown formats.
export function readArchive(bytes, prefix) {
  archivePath(prefix); if (!prefix.endsWith('/')) fail('archive prefix');
  const entries = [], seen = new Set(); let offset = 0, pending = {}, ended = false;
  const number = b => { const s = b.toString('ascii').replace(/\0.*$/, '').trim(); if (!/^[0-7]+$/.test(s)) fail('tar number'); return Number.parseInt(s, 8); };
  const string = b => { const zero = b.indexOf(0); return text(zero === -1 ? b : b.subarray(0, zero)); };
  const pax = data => {
    const fields = {}; let index = 0;
    while (index < data.length) {
      const space = data.indexOf(32, index); if (space < 0) fail('PAX length');
      const len = text(data.subarray(index, space)); if (!/^[1-9]\d*$/.test(len)) fail('PAX length');
      const end = index + Number(len); if (!Number.isSafeInteger(end) || end > data.length || data[end - 1] !== 10) fail('PAX record');
      const row = text(data.subarray(space + 1, end - 1)), equals = row.indexOf('=');
      if (equals < 1) fail('PAX field');
      const key = row.slice(0, equals); if (Object.hasOwn(fields, key)) fail('duplicate PAX');
      if (!['path', 'linkpath', 'comment', 'mtime', 'atime', 'ctime'].includes(key)) fail('unsupported PAX field');
      fields[key] = row.slice(equals + 1); index = end;
    }
    return fields;
  };
  while (offset + 512 <= bytes.length) {
    const header = bytes.subarray(offset, offset + 512); offset += 512;
    if (header.every(b => b === 0)) {
      if (bytes.length - offset < 512 || !bytes.subarray(offset).every(b => b === 0) || Object.keys(pending).length) fail('tar ending');
      ended = true; break;
    }
    const sum = [...header].reduce((s, b, i) => s + (i >= 148 && i < 156 ? 32 : b), 0);
    if (sum !== number(header.subarray(148, 156)) || string(header.subarray(257, 263)) !== 'ustar') fail('tar header');
    const size = number(header.subarray(124, 136)), mode = number(header.subarray(100, 108));
    if (!Number.isSafeInteger(size) || offset + Math.ceil(size / 512) * 512 > bytes.length) fail('tar size');
    const data = bytes.subarray(offset, offset + size); offset += Math.ceil(size / 512) * 512;
    const kind = String.fromCharCode(header[156] || 48);
    if (kind === 'x' || kind === 'g') {
      const fields = pax(data);
      if (kind === 'g') { if (Object.keys(fields).some(k => k !== 'comment')) fail('global PAX'); }
      else { if (Object.keys(pending).length) fail('consecutive PAX'); pending = fields; }
      continue;
    }
    if (!['0', '2', '5'].includes(kind)) fail('archive entry type');
    const base = string(header.subarray(345, 500)), name = pending.path ?? ((base ? `${base}/` : '') + string(header.subarray(0, 100)));
    const target = pending.linkpath ?? string(header.subarray(157, 257)); pending = {};
    if (kind !== '2' && target) fail('archive link type');
    archivePath(name);
    if (!(name === prefix || name.startsWith(`${prefix}skills/`)) || (kind !== '5' && name.endsWith('/'))) fail('archive prefix/path');
    const key = name.replace(/\/$/, ''); if (seen.has(key)) fail('duplicate archive entry'); seen.add(key);
    if ((kind !== '0' && size !== 0) || (mode & ~0o777)) fail('archive mode/data');
    entries.push({ path: name, type: { 0: 'file', 2: 'link', 5: 'directory' }[kind], mode, ...(kind === '2' ? { target } : kind === '0' ? { bytes: data } : {}) });
  }
  if (!ended || !entries.length) fail('truncated archive');
  linksStayInside(entries, prefix); return entries;
}

function frozenFiles(root, commit, run) {
  const output = checked(run, 'git', ['ls-tree', '-r', '-z', '--full-tree', commit], { cwd: root }).stdout;
  const entries = String(output).split('\0').filter(Boolean).map(row => {
    const match = /^(100644|100755|120000) blob ([a-f0-9]{40})\t([\s\S]+)$/.exec(row);
    if (!match) fail('frozen tree entry');
    const [, mode, blob, path] = match; archivePath(path);
    if (/(?:^|\/)\.env(?:\.|$)|private[-_]?key|updater[-_]?key/i.test(path)) fail('forbidden source path');
    const bytes = Buffer.from(checked(run, 'git', ['cat-file', 'blob', blob], { cwd: root }).stdout);
    return { path, mode, blob, sha256: hash(bytes), bytes };
  });
  if (!entries.length) fail('empty frozen tree'); return entries;
}
function checkSkills(files, version) {
  const map = new Map(files.filter(f => f.path.startsWith('skills/')).map(f => [f.path, f]));
  const read = path => { const entry = map.get(path); if (!entry || entry.mode === '120000') fail(`missing native skill input: ${path}`); return text(entry.bytes); };
  const manifest = json(read('skills/.claude-plugin/plugin.json'), 'skill manifest');
  if (manifest.name !== 'towermd' || manifest.version !== version || typeof manifest.description !== 'string' || !manifest.description.trim() || Object.keys(manifest).some(k => !['name', 'version', 'description'].includes(k))) fail('skill manifest');
  for (const path of ['skills/INSTALL.md', 'skills/references/session-contract.md', 'skills/references/cli-contracts.json', 'skills/scripts/check-contracts.mjs']) if (!read(path).trim()) fail('empty skill resource');
  const actual = [...map.keys()].filter(p => /\/SKILL\.md$/.test(p)).sort();
  const expected = skills.map(name => `skills/skills/towermd-${name}/SKILL.md`).sort();
  if (JSON.stringify(actual) !== JSON.stringify(expected)) fail('six native skills');
  if ([...map.values()].some(f => f.mode === '100755' && /(?:^|\/)(?:towermd-)?seed(?:[./_-]|$)/i.test(f.path))) fail('executable seed entry');
  for (const path of expected) {
    const body = read(path), name = path.split('/').at(-2);
    if (!body.startsWith('---\n') || !new RegExp(`^name: ${name}$`, 'm').test(body) || !/^description: \S.+$/m.test(body) || !body.includes('../../references/session-contract.md') || /尚未接入|not implemented|placeholder|\bstub\b/i.test(body)) fail('native skill body');
  }
  // Validate all local Markdown resources against the frozen member graph, not the worktree.
  const resolveLink = (path, seen = new Set()) => {
    if (!path.startsWith('skills/') || seen.has(path)) fail('skill reference escape/cycle');
    const found = map.get(path); if (!found) fail('missing skill reference');
    if (found.mode !== '120000') return found;
    seen.add(path); const target = text(found.bytes);
    if (posix.isAbsolute(target)) fail('skill symlink escape');
    return resolveLink(posix.normalize(posix.join(posix.dirname(path), target)), seen);
  };
  for (const f of map.values()) {
    resolveLink(f.path);
    if (f.path.endsWith('.md') && f.mode !== '120000') for (const m of text(f.bytes).matchAll(/\[[^\]]*\]\(([^\s)]+)\)/g)) {
      if (/^https:\/\//.test(m[1]) || m[1].startsWith('#')) continue;
      if (posix.isAbsolute(m[1]) || /^[a-z]+:/i.test(m[1])) fail('skill reference path');
      resolveLink(posix.normalize(posix.join(posix.dirname(f.path), m[1].split('#')[0])));
    }
  }
  return [...map.values()];
}

export function archiveSkills({ root, commit, version, workspace, destination }, run = runCommand) {
  const expected = checkSkills(frozenFiles(root, commit, run), version), prefix = `towermd-skills-${version}/`, tar = join(workspace, 'skills.tar');
  checked(run, 'git', ['-c', 'tar.umask=0022', 'archive', '--format=tar', `--prefix=${prefix}`, '--output', tar, commit, 'skills'], { cwd: root });
  const entries = readArchive(readFileSync(tar), prefix), byPath = new Map(entries.filter(e => e.type !== 'directory').map(e => [e.path.slice(prefix.length), e]));
  const dirs = new Set([prefix.slice(0, -1)]);
  for (const f of expected) {
    for (let parent = posix.dirname(f.path); parent !== '.'; parent = posix.dirname(parent)) dirs.add(prefix + parent);
    const e = byPath.get(f.path), mode = f.mode === '100755' ? 0o755 : f.mode === '120000' ? 0o777 : 0o644;
    if (!e || e.mode !== mode || (f.mode === '120000' ? e.type !== 'link' || e.target !== text(f.bytes) : e.type !== 'file' || !e.bytes.equals(f.bytes))) fail('archive differs from frozen blob/mode');
  }
  if (byPath.size !== expected.length || entries.some(e => e.type === 'directory' && (e.mode !== 0o755 || !dirs.has(e.path.replace(/\/$/, ''))))) fail('archive member set');
  const extracted = join(workspace, 'extracted'); mkdirSync(extracted);
  checked(run, 'tar', ['-xf', tar, '-C', extracted]);
  const actual = bundleTree(join(extracted, prefix, 'skills')).filter(e => e.type !== 'directory');
  if (actual.length !== expected.length) fail('extracted skill set');
  for (const f of expected) {
    const item = actual.find(e => `skills/${e.path}` === f.path);
    if (!item || (f.mode === '120000' ? item.type !== 'link' || item.target !== text(f.bytes) : item.type !== 'file' || item.sha256 !== f.sha256 || item.mode !== (f.mode === '100755' ? 0o755 : 0o644))) fail('extracted skill blob/mode');
  }
  writeFileSync(destination, checked(run, 'gzip', ['-n', '-c', tar]).stdout, { flag: 'wx' });
  return { files: expected.length, scope: 'frozen native package structure/blobs; no harness loading or human acceptance asserted' };
}

function checkReceipt(build, files, identity, run) {
  if (!build || !targets.includes(build.target) || ['tag', 'commit', 'tree', 'tagObject'].some(k => build[k] !== identity[k])) fail('build receipt identity');
  if (typeof build.app !== 'string' || typeof build.dmg !== 'string') fail('build receipt paths');
  const output = resolve(build.app, '../../../../../..');
  const expectedApp = join(output, 'target', build.target, 'release/bundle/macos/Tower.md.app');
  if (build.app !== expectedApp || dirname(build.dmg) !== join(output, 'target', build.target, 'release/bundle/dmg') || !build.dmg.endsWith('.dmg')) fail('build receipt layout');
  for (const [path, suffix] of [[build.app, '.app'], [build.dmg, '.dmg']]) {
    directory(dirname(path));
    const names = readdirSync(dirname(path)).filter(name => name.endsWith(suffix));
    if (names.length !== 1 || names[0] !== basename(path)) fail('build bundle count');
  }
  directory(output); regular(join(output, 'build-receipt.json'));
  if (JSON.stringify(json(readFileSync(join(output, 'build-receipt.json')), 'build receipt JSON')) !== JSON.stringify(build)) fail('build receipt differs from disk');
  const sources = files.map(({ bytes, ...source }) => source);
  if (JSON.stringify(build.sources) !== JSON.stringify(sources)) fail('build frozen sources');
  const lockfiles = Object.fromEntries(locks.map(path => [path, files.find(f => f.path === path)?.sha256]));
  if (Object.values(lockfiles).some(v => !v) || JSON.stringify(build.lockfiles) !== JSON.stringify(lockfiles)) fail('build lockfiles');
  if (['node', 'pnpm', 'cargo', 'rustc', 'tauri', 'sdk', 'xcode'].some(k => typeof build.tools?.[k] !== 'string' || !build.tools[k].trim())) fail('build tools');
  const commands = build.commands;
  if (!Array.isArray(commands) || commands.some(c => c.status !== 0 || c.failed !== false || !Array.isArray(c.args) || !/^[a-f0-9]{64}$/.test(c.stdoutSha256) || !/^[a-f0-9]{64}$/.test(c.stderrSha256) || !Number.isFinite(Date.parse(c.started)) || !Number.isFinite(Date.parse(c.finished)))) fail('successful build commands');
  const lipo = commands.filter(c => c.file === 'lipo' && c.args.length === 2 && c.args[0] === '-archs');
  if (lipo.length !== 1 || !isAbsolute(lipo[0].args[1])) fail('staged CLI command');
  const stage = lipo[0].args[1], buildRoot = resolve(stage, '../../..'); directory(buildRoot);
  if (stage !== join(buildRoot, 'src-tauri/binaries', `towermd-${build.target}`)) fail('staged CLI path');
  if (String(checked(run, 'git', ['rev-parse', '--show-toplevel'], { cwd: buildRoot }).stdout).trim() !== buildRoot ||
      String(checked(run, 'git', ['rev-parse', '--verify', 'HEAD'], { cwd: buildRoot }).stdout).trim() !== identity.commit) fail('build source root');
  for (const source of files) {
    const path = join(buildRoot, source.path); let bytes;
    try {
      if (realpathSync(dirname(path)) !== dirname(path)) fail('build source parent');
      const stat = lstatSync(path);
      if (source.mode === '120000') {
        if (!stat.isSymbolicLink()) fail('build source link');
        bytes = readlinkSync(path, { encoding: 'buffer' });
      } else {
        if (!stat.isFile() || !!(stat.mode & 0o111) !== (source.mode === '100755')) fail('build source mode');
        bytes = readFileSync(path);
      }
    } catch { fail('build source file'); }
    if (hash(bytes) !== source.sha256) fail('build source bytes');
  }
  const locate = (file, args) => commands.findIndex(c => c.file === file && JSON.stringify(c.args) === JSON.stringify(args));
  const cargo = commands.find(c => basename(c.file ?? '') === 'cargo' && JSON.stringify(c.args) === JSON.stringify(['build', '--locked', '-p', 'towermd', '--release', '--target', build.target]));
  const metadata = cargo ? locate(cargo.file, ['metadata', '--locked', '--no-deps', '--format-version', '1']) : -1;
  const script = join(buildRoot, 'scripts/project-pnpm.mjs');
  const front = commands.findIndex(c => c.file === process.execPath && JSON.stringify(c.args) === JSON.stringify([script, 'build']));
  const tauri = commands.findIndex(c => c.file === process.execPath && JSON.stringify(c.args) === JSON.stringify([script, 'tauri', 'build', '--target', build.target, '--bundles', 'app,dmg', '--config', 'src-tauri/tauri.unsigned.conf.json', '--', '--locked']));
  if (!cargo || commands.indexOf(cargo) >= metadata || metadata >= commands.indexOf(lipo[0]) || commands.indexOf(lipo[0]) >= front || front >= tauri || tauri < 0) fail('build command source chain');
  for (const required of ['src-tauri/tauri.conf.json', 'src-tauri/tauri.unsigned.conf.json', 'src-tauri/src/lib.rs', 'index.html', 'vite.config.ts',
    'scripts/project-pnpm.mjs', 'scripts/build-app.mjs', 'scripts/stage-cli.mjs', 'scripts/release/build.mjs'])
    if (!files.some(f => f.path === required)) fail('frontend source chain');
  const targetCli = join(output, 'target', build.target, 'release/towermd');
  for (const path of [targetCli, stage, join(build.app, 'Contents/MacOS/towermd')]) regular(path, true);
  const staged = readFileSync(stage), bundled = readFileSync(join(build.app, 'Contents/MacOS/towermd'));
  if (hash(readFileSync(targetCli)) !== hash(staged)) fail('target/staged CLI hash');
  assertCliSigningEquivalent(staged, bundled, build.target);
  return { buildRoot, cliHash: hash(bundled), lockfiles };
}

export function assembleAssets({ root, tag, output, builds }, run = runCommand) {
  if (!Array.isArray(builds) || builds.length !== 2 || new Set(builds.map(b => b?.target)).size !== 2) fail('two build receipts');
  if (typeof tag !== 'string' || !/^v\d+\.\d+\.\d+$/.test(tag)) fail('tag');
  const version = tag.slice(1); assertVersions({ version }, version);
  root = realpathSync(root);
  if (typeof output !== 'string' || !isAbsolute(output)) fail('absolute output');
  output = resolve(output); const logs = `${output}.logs`;
  if (inside(root, output) || inside(output, root) || existsSync(output) || existsSync(logs) || realpathSync(dirname(output)) !== dirname(output)) fail('new output outside source');
  mkdirSync(logs, { mode: 0o700 });
  const env = unsignedEnv(process.env, join(root, '.tools/pnpm/node_modules/.bin'));
  const logged = (file, args, options = {}) => {
    const started = new Date().toISOString(); let result;
    try { result = run(file, args, { cwd: root, env, shell: false, ...options }); }
    catch { result = { status: null, error: true }; }
    const stdout = result?.stdout ?? '', stderr = result?.stderr ?? '';
    appendFileSync(join(logs, 'stdout.log'), stdout); appendFileSync(join(logs, 'stderr.log'), stderr);
    const record = { file, args, started, finished: new Date().toISOString(), status: result?.status ?? null, failed: !!result?.error, stdoutSha256: hash(stdout), stderrSha256: hash(stderr) };
    appendFileSync(join(logs, 'commands.jsonl'), JSON.stringify(record) + '\n'); return result;
  };
  const git = args => String(checked(logged, 'git', args).stdout).trim();
  const identity = () => ({ tag, commit: git(['rev-parse', '--verify', `refs/tags/${tag}^{commit}`]), tree: git(['rev-parse', '--verify', `refs/tags/${tag}^{tree}`]), tagObject: git(['rev-parse', '--verify', `refs/tags/${tag}`]) });
  const initial = identity(); if (git(['rev-parse', '--verify', 'HEAD']) !== initial.commit) fail('source HEAD/tag');
  const files = frozenFiles(root, initial.commit, logged);
  writeFileSync(join(logs, 'inputs-before.json'), JSON.stringify({ ...initial, sources: files.map(({ bytes, ...f }) => f), builds }, null, 2) + '\n');
  const provenance = builds.map(b => checkReceipt(b, files, initial, logged));
  for (const { buildRoot } of provenance) if (inside(buildRoot, output) || inside(output, buildRoot)) fail('output overlaps build source');
  const packageInputs = () => builds.map(b => {
    regular(b.dmg); return { app: bundleTree(b.app), dmg: hash(readFileSync(b.dmg)) };
  });
  const beforePackages = packageInputs();
  checkSkills(files, version);
  const read = path => { const entry = files.find(f => f.path === path); if (!entry || entry.mode === '120000') fail('frozen changelog'); return text(entry.bytes); };
  const en = read('CHANGELOG.md'), zh = read('CHANGELOG.zh-CN.md'); checkChangelogs(en, zh, 'frozen', version);
  mkdirSync(output, { mode: 0o700 });
  const inspections = [], artifacts = [];
  const record = (name, target) => { const path = join(output, name), s = regular(path); artifacts.push({ name, target, size: s.size, sha256: hash(readFileSync(path)) }); };
  try {
    for (const target of targets) {
      const build = builds.find(b => b.target === target), provenanceItem = provenance[builds.indexOf(build)], arch = target.split('-')[0];
      const inspection = inspectApp({ app: build.app, target, version }, logged);
      inspectDmg({ dmg: build.dmg, app: build.app }, logged);
      const zip = `tower.md-${version}-${arch}.app.zip`, dmg = `tower.md-${version}-${arch}.dmg`;
      checked(logged, 'ditto', ['-c', '-k', '--sequesterRsrc', '--keepParent', build.app, join(output, zip)]);
      const unpack = join(logs, `unpacked-${arch}`); mkdirSync(unpack);
      checked(logged, 'ditto', ['-x', '-k', join(output, zip), unpack]);
      if (readdirSync(unpack).some(name => name !== basename(build.app) && name !== '__MACOSX') || JSON.stringify(bundleTree(join(unpack, basename(build.app)))) !== JSON.stringify(inspection.tree)) fail('App ZIP differs');
      copyFileSync(build.dmg, join(output, dmg)); record(zip, target); record(dmg, target);
      inspections.push({ target, scope: inspection.scope, cliVersion: inspection.cliVersion, frontend: inspection.frontend, gatekeeper: inspection.gatekeeper, cliSha256: provenanceItem.cliHash, sourceTree: initial.tree });
    }
    const skillsName = `towermd-skills-${version}.tar.gz`, workspace = join(logs, 'skills'); mkdirSync(workspace);
    archiveSkills({ root, commit: initial.commit, version, workspace, destination: join(output, skillsName) }, logged); record(skillsName, null);
    for (const lang of ['en', 'zh-CN']) {
      const body = parseChangelog(lang === 'en' ? en : zh, lang).versions.find(v => v.version === version).body.trim();
      const install = lang === 'en'
        ? `Choose aarch64 for Apple Silicon or x86_64 for Intel. Extract the App ZIP or open the DMG and copy Tower.md.app. The CLI is bundled; running the App needs no Node, Rust or pnpm.\n\nThis release uses ad-hoc signing, has no Apple publisher identity or notarization, and has no automatic updates. Gatekeeper may require an explicit approval in Privacy & Security; do not disable Gatekeeper globally.\n\nThese asset checks do not launch the App. Intel has static package/source checks only here; no Intel device acceptance is asserted. Embedded frontend bytes and P6/Task8 runtime acceptance remain separate.\n\nExtract towermd-skills-${version}.tar.gz; follow towermd-skills-${version}/skills/INSTALL.md for Claude Code and Codex. Native harness loading and human acceptance are separate evidence.`
        : `Apple Silicon 选择 aarch64，Intel 选择 x86_64。解开 App ZIP 或打开 DMG 后复制 Tower.md.app。内置 CLI，运行 App 无需 Node、Rust 或 pnpm。\n\n本版为 ad-hoc 签名，无 Apple 发行者身份、未公证、无自动更新。Gatekeeper 可能要求在“隐私与安全性”显式允许打开；不要关闭全局 Gatekeeper。\n\n本工具不启动 App；这里的 Intel 证据仅为静态包与源码检查，未声称 Intel 设备验收。嵌入前端字节与 P6/Task8 运行验收另行证明。\n\n解开 towermd-skills-${version}.tar.gz，按 towermd-skills-${version}/skills/INSTALL.md 使用 Claude Code/Codex；原生加载及真人验收证据另行记录。`;
      writeFileSync(join(output, `RELEASE-NOTES.${lang}.md`), `# Tower.md ${version}\n\n${body}\n\n${install}\n`, { flag: 'wx' });
    }
    if (JSON.stringify(identity()) !== JSON.stringify(initial) || JSON.stringify(frozenFiles(root, initial.commit, logged).map(({ bytes, ...f }) => f)) !== JSON.stringify(files.map(({ bytes, ...f }) => f))) fail('frozen inputs changed');
    builds.forEach(b => checkReceipt(b, files, initial, logged));
    if (JSON.stringify(packageInputs()) !== JSON.stringify(beforePackages)) fail('bundle inputs changed');
    for (const artifact of artifacts) {
      const path = join(output, artifact.name); regular(path);
      const bytes = readFileSync(path);
      if (bytes.length !== artifact.size || hash(bytes) !== artifact.sha256) fail('binary artifact changed');
    }
    writeFileSync(join(logs, 'inputs-after.json'), JSON.stringify({ ...initial, sources: files.map(({ bytes, ...f }) => f), builds }, null, 2) + '\n');
    const manifest = { version, ...initial, targets: inspections, tools: Object.fromEntries(builds.map(b => [b.target, b.tools])), lockfiles: provenance[0].lockfiles, artifacts: artifacts.sort((a, b) => a.name.localeCompare(b.name)) };
    writeFileSync(join(output, 'BUILD-MANIFEST.json'), JSON.stringify(manifest, null, 2) + '\n', { flag: 'wx' });
    const buffers = new Map(expectedAssets(version).filter(name => name !== 'SHA256SUMS').map(name => [name, readFileSync(join(output, name))]));
    const sums = [...buffers].map(([name, bytes]) => `${hash(bytes)}  ${name}`).join('\n') + '\n';
    verifySums(sums, buffers); writeFileSync(join(output, 'SHA256SUMS'), sums, { flag: 'wx' });
    assertAssetSet(readdirSync(output), version);
    verifySums(readFileSync(join(output, 'SHA256SUMS'), 'utf8'), new Map([...buffers.keys()].map(name => [name, readFileSync(join(output, name))])));
    return { manifest, assets: expectedAssets(version).map(name => { const bytes = readFileSync(join(output, name)); return { name, size: bytes.length, sha256: hash(bytes) }; }) };
  } catch (error) {
    for (const name of ['BUILD-MANIFEST.json', 'SHA256SUMS']) if (existsSync(join(output, name))) unlinkSync(join(output, name));
    throw error;
  }
}

export function main(args, run) {
  const values = {}, allowed = ['--root', '--tag', '--output', '--arm-build', '--intel-build'];
  for (let i = 0; i < args.length; i += 2) {
    const key = args[i], value = args[i + 1];
    if (!allowed.includes(key) || values[key] !== undefined || !value || value.startsWith('--')) fail('argument'); values[key] = value;
  }
  if (allowed.some(key => !values[key])) fail('argument');
  const builds = ['--arm-build', '--intel-build'].map((key, index) => {
    if (!isAbsolute(values[key])) fail('absolute build directory');
    const path = join(values[key], 'build-receipt.json'); regular(path);
    const build = json(readFileSync(path), 'build receipt'); if (build.target !== targets[index]) fail('build target argument'); return build;
  });
  return assembleAssets({ root: values['--root'], tag: values['--tag'], output: values['--output'], builds }, run);
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(process.argv.slice(2)); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
