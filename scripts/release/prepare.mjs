import { readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { homedir } from 'node:os';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { assertVersions, checkChangelogs } from './core.mjs';

const cargo = join(homedir(), '.cargo/bin/cargo');
const members = ['crates/towermd-core', 'crates/towermd'];
const locks = ['Cargo.lock', 'src-tauri/Cargo.lock'];
const pluginManifest = 'skills/.claude-plugin/plugin.json';
const writable = ['Cargo.toml', 'src-tauri/Cargo.toml', 'package.json',
  'src-tauri/tauri.conf.json', pluginManifest, ...locks, 'pnpm-lock.yaml', 'CHANGELOG.md', 'CHANGELOG.zh-CN.md'];
const read = (root, path) => readFileSync(join(root, path), 'utf8');
const fail = field => { throw new Error(`invalid ${field}`); };
const runCommand = (file, args, options) => spawnSync(file, args, { ...options, encoding: 'utf8' });

function versionInput(version) {
  assertVersions({ version }, version);
}
function dateInput(date) {
  if (typeof date !== 'string' || !/^\d{4}-\d{2}-\d{2}$/.test(date) ||
      !Number.isFinite(Date.parse(`${date}T00:00:00Z`)) ||
      new Date(`${date}T00:00:00Z`).toISOString().slice(0, 10) !== date) fail('date');
}
// These readers intentionally accept the project's fixed manifest layout only.
function section(text, name, file) {
  const headings = [...text.matchAll(/^\[([^\n]+)\][ \t]*(?:#.*)?$/gm)];
  const found = headings.filter(match => match[1] === name);
  if (found.length !== 1) fail(`${file} [${name}]`);
  const heading = found[0], index = headings.indexOf(heading);
  const start = heading.index + heading[0].length;
  const end = headings[index + 1]?.index ?? text.length;
  return { body: text.slice(start, end), start, end };
}
function field(body, key, file) {
  const lines = body.split('\n').filter(line => line.trimStart().startsWith(`${key} `) || line.trimStart().startsWith(`${key}=`));
  if (lines.length !== 1) fail(`${file} ${key}`);
  const match = /^[^=]+=[ \t]*(.*?)[ \t]*(?:#.*)?$/.exec(lines[0]);
  if (!match) fail(`${file} ${key}`);
  return match[1];
}
function stringField(body, key, file) {
  const value = field(body, key, file), match = /^"([^"\n]+)"$/.exec(value);
  if (!match) fail(`${file} ${key}`);
  return match[1];
}
function versionField(body, file, inherited = false) {
  const versionKeys = body.split('\n').filter(line => /^[ \t]*version(?:[. \t=])/.test(line));
  if (versionKeys.length !== 1) fail(`${file} version`);
  if (inherited) {
    if (field(body, 'version.workspace', file) !== 'true') fail(`${file} version.workspace`);
    return;
  }
  const version = stringField(body, 'version', file);
  try { versionInput(version); } catch { fail(`${file} version`); }
  return version;
}
function arrayField(body, key, file) {
  try {
    const value = JSON.parse(field(body, key, file));
    if (!Array.isArray(value) || !value.every(item => typeof item === 'string')) fail(`${file} ${key}`);
    return value;
  } catch { fail(`${file} ${key}`); }
}
function json(root, file) {
  try { return JSON.parse(read(root, file)); } catch { fail(file); }
}
function importer(text) {
  if (!/^lockfileVersion: ['"]?9\.0['"]?[ \t]*$/m.test(text)) fail('pnpm-lock.yaml lockfileVersion');
  const root = /^  \.:\n([\s\S]*?)(?=^  \S|^\S|(?![\s\S]))/gm;
  const found = [...text.matchAll(root)];
  if (found.length !== 1 || !/^importers:\s*$/m.test(text)) fail('pnpm-lock.yaml root importer');
  return found[0];
}
function pnpmVersion(text, pkg, expected) {
  const body = importer(text)[1];
  const versions = [...body.matchAll(/^    version: (.+)$/gm)];
  if (versions.length > 1) fail('pnpm-lock.yaml version');
  if (versions.length && versions[0][1].replace(/^['"]|['"]$/g, '') !== expected)
    fail('pnpm-lock.yaml version');
  for (const group of ['dependencies', 'devDependencies', 'optionalDependencies']) {
    const header = new RegExp(`^    ${group}:\\n([\\s\\S]*?)(?=^    \\S|(?![\\s\\S]))`, 'm');
    const block = header.exec(body)?.[1] ?? '';
    const records = [...block.matchAll(/^      (.+):\n([\s\S]*?)(?=^      \S|(?![\s\S]))/gm)];
    const dependencies = Object.fromEntries(records.map(match => {
      const name = match[1].replace(/^['"]|['"]$/g, '');
      const specs = [...match[2].matchAll(/^        specifier: (.+)$/gm)];
      if (specs.length !== 1) fail(`pnpm-lock.yaml ${group} specifier`);
      return [name, specs[0][1].replace(/^['"]|['"]$/g, '')];
    }));
    const declared = pkg[group] ?? {};
    if (records.length !== Object.keys(dependencies).length ||
        Object.keys(declared).length !== records.length ||
        Object.entries(declared).some(([name, value]) => dependencies[name] !== value))
      fail(`pnpm-lock.yaml ${group}`);
  }
}
function manifests(root, expected) {
  const rootText = read(root, 'Cargo.toml');
  const workspace = section(rootText, 'workspace', 'Cargo.toml').body;
  const actualMembers = arrayField(workspace, 'members', 'Cargo.toml');
  if (JSON.stringify([...actualMembers].sort()) !== JSON.stringify([...members].sort())) fail('Cargo.toml members');
  if (!arrayField(workspace, 'exclude', 'Cargo.toml').includes('src-tauri')) fail('Cargo.toml exclude');
  const values = { 'Cargo.toml': versionField(section(rootText, 'workspace.package', 'Cargo.toml').body, 'Cargo.toml') };
  for (const path of members) {
    const file = `${path}/Cargo.toml`, body = section(read(root, file), 'package', file).body;
    if (stringField(body, 'name', file) !== path.split('/')[1]) fail(`${file} name`);
    versionField(body, file, true);
  }
  const desktop = read(root, 'src-tauri/Cargo.toml');
  section(desktop, 'workspace', 'src-tauri/Cargo.toml');
  const app = section(desktop, 'package', 'src-tauri/Cargo.toml').body;
  if (stringField(app, 'name', 'src-tauri/Cargo.toml') !== 'towermd-app') fail('src-tauri/Cargo.toml name');
  values['src-tauri/Cargo.toml'] = versionField(app, 'src-tauri/Cargo.toml');
  const pkg = json(root, 'package.json');
  values['package.json'] = pkg.version;
  values['src-tauri/tauri.conf.json'] = json(root, 'src-tauri/tauri.conf.json').version;
  const plugin = json(root, pluginManifest);
  if (!plugin || plugin.name !== 'towermd') fail(`${pluginManifest} name`);
  values[pluginManifest] = plugin.version;
  assertVersions(values, expected ?? values['Cargo.toml']);
  pnpmVersion(read(root, 'pnpm-lock.yaml'), pkg, expected ?? values['Cargo.toml']);
  return values['Cargo.toml'];
}
function checkLock(root, file, expected) {
  const names = file === 'Cargo.lock' ? ['towermd-core', 'towermd'] : ['towermd-app', 'towermd-core'];
  const packages = read(root, file).split(/^\[\[package\]\][ \t]*$/m).slice(1);
  const seen = [];
  for (const body of packages) {
    const name = stringField(body, 'name', file);
    if (!name.startsWith('towermd')) continue;
    if (!names.includes(name) || seen.includes(name) || /^[ \t]*source[ \t]*=/m.test(body)) fail(`${file} package`);
    seen.push(name);
    assertVersions({ [`${file} ${name}`]: versionField(body, file) }, expected);
  }
  if (seen.length !== names.length) fail(`${file} package`);
}
// Recognize only known offline-cache diagnostics; never forward child output.
function offlineMissing(file, args, result) {
  if (!args.includes('--offline')) return;
  const output = [result?.stderr, result?.stdout].filter(value => typeof value === 'string').join('\n');
  if (file === cargo) {
    const missing = /^error: no matching package named `([a-z][a-z0-9_-]{0,63})` found$/m.exec(output);
    if (missing) return missing[1];
    const download = /^error: failed to download `([a-z][a-z0-9_-]{0,63}) v(\d{1,9}\.\d{1,9}\.\d{1,9})`$/m.exec(output);
    if (download && /^  attempting to make an HTTP request, but --offline was specified$/m.test(output))
      return `${download[1]}@${download[2]}`;
  } else {
    const missing = /^[ \t\u2009]*ERR_PNPM_NO_OFFLINE_TARBALL[ \t\u2009]+A package is missing from the store but cannot download it in offline mode\. The missing package may be downloaded from https:\/\/registry\.npmjs\.org\/((?:@[a-z0-9][a-z0-9._-]{0,63}\/)?([a-z0-9][a-z0-9._-]{0,127}))\/-\/\2-(\d{1,9}\.\d{1,9}\.\d{1,9})\.tgz\.?[ \t]*$/m.exec(output);
    if (missing) return `${missing[1]}@${missing[3]}`;
  }
}
function command(root, run, file, args) {
  const result = run(file, args, { cwd: root, shell: false });
  if (result?.status !== 0) {
    const missing = offlineMissing(file, args, result);
    throw new Error(`${file === cargo ? 'cargo' : 'project-pnpm.mjs'} ${args[0]} exit ${result?.status ?? 'unknown'}${missing ? `; missing cache: ${missing}` : ''}`);
  }
  return result.stdout;
}
function metadata(root, version, run, desktop, locked) {
  const args = ['metadata', ...(locked ? ['--locked'] : []), '--offline', '--no-deps', '--format-version', '1',
    ...(desktop ? ['--manifest-path', 'src-tauri/Cargo.toml'] : [])];
  const output = command(root, run, cargo, args);
  let data;
  try { data = JSON.parse(output); } catch { fail('cargo metadata JSON'); }
  const paths = desktop ? ['src-tauri'] : members;
  if (data.workspace_root !== join(root, desktop ? 'src-tauri' : '') ||
      !Array.isArray(data.workspace_members) || data.workspace_members.length !== paths.length ||
      !Array.isArray(data.packages)) fail('cargo metadata workspace');
  for (const path of paths) {
    const name = path === 'src-tauri' ? 'towermd-app' : path.split('/')[1];
    const matches = data.packages.filter(pkg => pkg.name === name && pkg.manifest_path === join(root, path, 'Cargo.toml'));
    if (matches.length !== 1 || !data.workspace_members.includes(matches[0].id)) fail('cargo metadata member');
    assertVersions({ [`cargo metadata ${path}`]: matches[0].version }, version);
  }
}
function checkProject(root, version, run) {
  manifests(root, version);
  for (const [index, file] of locks.entries()) {
    checkLock(root, file, version);
    metadata(root, version, run, index === 1, true);
  }
}
export function checkVersion(root, version, phase, run = runCommand) {
  versionInput(version);
  root = resolve(root);
  checkChangelogs(read(root, 'CHANGELOG.md'), read(root, 'CHANGELOG.zh-CN.md'), phase, version);
  checkProject(root, version, run);
}
function setTomlVersion(text, name, file, version) {
  const part = section(text, name, file);
  versionField(part.body, file);
  const body = part.body.replace(/^([ \t]*version[ \t]*=[ \t]*")[^"]+(".*)$/m, `$1${version}$2`);
  return text.slice(0, part.start) + body + text.slice(part.end);
}
function setJsonVersion(text, version) {
  const object = JSON.parse(text); object.version = version;
  const indent = /^([ \t]+)"/m.exec(text)?.[1] ?? 2;
  return JSON.stringify(object, null, indent) + (text.endsWith('\n') ? '\n' : '');
}
function rotate(text, title, version, date) {
  const visible = text.replace(/<!--[\s\S]*?(?:-->|$)/g, comment => comment.replace(/[^\n]/g, ' '));
  const heading = new RegExp(`^## ${title}\\n`, 'm').exec(visible);
  const start = heading.index + heading[0].length;
  return text.slice(0, start) + `\n## v${version} — ${date}\n` + text.slice(start);
}
export function prepareVersion(root, version, date, run = runCommand) {
  versionInput(version); dateInput(date);
  root = resolve(root);
  const before = Object.fromEntries(writable.map(file => [file, read(root, file)]));
  checkChangelogs(before['CHANGELOG.md'], before['CHANGELOG.zh-CN.md'], 'prepare', version);
  const previous = manifests(root);
  checkProject(root, previous, run);
  const updates = {
    'Cargo.toml': setTomlVersion(before['Cargo.toml'], 'workspace.package', 'Cargo.toml', version),
    'src-tauri/Cargo.toml': setTomlVersion(before['src-tauri/Cargo.toml'], 'package', 'src-tauri/Cargo.toml', version),
    'package.json': setJsonVersion(before['package.json'], version),
    'src-tauri/tauri.conf.json': setJsonVersion(before['src-tauri/tauri.conf.json'], version),
    [pluginManifest]: setJsonVersion(before[pluginManifest], version),
  };
  const lock = before['pnpm-lock.yaml'], part = importer(lock);
  const updated = part[0].replace(/^(    version: ['"]?)[^'"\n]+(['"]?)$/m, `$1${version}$2`);
  if (updated !== part[0]) updates['pnpm-lock.yaml'] = lock.slice(0, part.index) + updated + lock.slice(part.index + part[0].length);
  const owned = {};
  // Compare generated locks only against the precise project-version-only change.
  const expectedLocks = Object.fromEntries(locks.map(file => [file, before[file].replace(
    /(^name = "towermd(?:-core|-app)?"\nversion = ")[^"]+/gm, `$1${version}`)]));
  const writeOwned = (file, text) => {
    if (read(root, file) !== before[file]) throw new Error(`write conflict: ${file}`);
    writeFileSync(join(root, file), text);
    owned[file] = text;
  };
  const trackedRun = (file, args, options) => {
    const prior = Object.fromEntries(locks.map(path => [path, read(root, path)]));
    try { return run(file, args, options); }
    finally {
      for (const path of locks) {
        try {
          const current = read(root, path);
          if (current !== prior[path] && current === expectedLocks[path]) owned[path] = current;
        } catch { /* Recovery below reports unreadable files without masking the tool error. */ }
      }
    }
  };
  try {
    for (const [file, text] of Object.entries(updates)) writeOwned(file, text);
    for (const [index, file] of locks.entries()) {
      metadata(root, version, trackedRun, index === 1, false);
      try { checkLock(root, file, version); } catch {
        command(root, trackedRun, cargo, ['check', '--offline', ...(index === 1 ? ['--manifest-path', 'src-tauri/Cargo.toml'] : [])]);
        checkLock(root, file, version);
      }
    }
    command(root, trackedRun, process.execPath, ['scripts/project-pnpm.mjs', 'install', '--offline', '--lockfile-only']);
    checkProject(root, version, trackedRun);
    command(root, trackedRun, process.execPath, ['scripts/project-pnpm.mjs', 'install', '--frozen-lockfile', '--offline']);
    const en = rotate(before['CHANGELOG.md'], 'Unreleased', version, date);
    const zh = rotate(before['CHANGELOG.zh-CN.md'], '未发布', version, date);
    checkChangelogs(en, zh, 'frozen', version);
    writeOwned('CHANGELOG.md', en);
    writeOwned('CHANGELOG.zh-CN.md', zh);
    return writable.filter(file => read(root, file) !== before[file]);
  } catch (error) {
    const conflicts = [];
    for (const file of writable) {
      try {
        const current = read(root, file);
        if (current === before[file]) continue;
        if (current !== owned[file]) { conflicts.push(file); continue; }
        writeFileSync(join(root, file), before[file]);
      } catch { conflicts.push(file); }
    }
    if (conflicts.length) throw new Error(`${error.message}; recovery conflict: ${conflicts.join(', ')}`);
    throw error;
  }
}
export function main(args, run = runCommand) {
  const [operation, ...rest] = args;
  if (!['check', 'prepare'].includes(operation)) fail('command');
  const allowed = operation === 'check' ? ['--root', '--version', '--phase'] : ['--root', '--version', '--date'];
  const values = {};
  for (let i = 0; i < rest.length; i += 2) {
    const key = rest[i], value = rest[i + 1];
    if (!allowed.includes(key) || values[key] !== undefined || !value || value.startsWith('--')) fail('argument');
    values[key] = value;
  }
  if (allowed.some(key => !values[key])) fail('argument');
  return operation === 'check'
    ? checkVersion(values['--root'], values['--version'], values['--phase'], run)
    : prepareVersion(values['--root'], values['--version'], values['--date'], run);
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { main(process.argv.slice(2)); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
