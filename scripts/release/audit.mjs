import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { lstatSync, readFileSync, readlinkSync, realpathSync, mkdirSync, writeFileSync } from 'node:fs';
import { dirname, isAbsolute, relative, resolve, sep } from 'node:path';
import { pathToFileURL } from 'node:url';

const sha = value => createHash('sha256').update(value).digest('hex');
const canonical = value => JSON.stringify(value, (_, v) => v && typeof v === 'object' && !Array.isArray(v) ?
  Object.fromEntries(Object.keys(v).sort().map(k => [k, v[k]])) : v);
const digest = value => sha(canonical(value));
const nonempty = value => typeof value === 'string' && value.trim().length > 0;
const inside = (root, path) => {
  const rel = relative(root, path);
  return rel !== '..' && !rel.startsWith(`..${sep}`) && !isAbsolute(rel);
};

function contentSha256(root, name) {
  const path = resolve(root, name);
  if (!inside(root, path)) throw new Error('file path escapes worktree');
  let parent = dirname(path);
  while (true) {
    try {
      if (!inside(root, realpathSync(parent))) throw new Error('file path escapes worktree');
      break;
    } catch (error) {
      if (error.code !== 'ENOENT') throw error;
      parent = dirname(parent);
    }
  }
  let stat;
  try { stat = lstatSync(path); } catch (error) {
    if (error.code === 'ENOENT') return null;
    throw new Error('file metadata unreadable');
  }
  if (stat.isSymbolicLink()) return sha(readlinkSync(path, {encoding: 'buffer'}));
  if (stat.isFile()) return sha(readFileSync(path));
  if (stat.isDirectory()) return null; // Gitlinks are bound by the binary diff hashes.
  throw new Error('unsupported file type');
}

export function inventory(repo, execGit = spawnSync) {
  const root = realpathSync(repo);
  const env = {...process.env, GIT_OPTIONAL_LOCKS: '0'};
  // The explicit repository must not inherit another repository or injected Git options.
  for (const key of ['GIT_DIR', 'GIT_WORK_TREE', 'GIT_COMMON_DIR', 'GIT_INDEX_FILE',
    'GIT_OBJECT_DIRECTORY', 'GIT_ALTERNATE_OBJECT_DIRECTORIES', 'GIT_CEILING_DIRECTORIES',
    'GIT_DISCOVERY_ACROSS_FILESYSTEM', 'GIT_IMPLICIT_WORK_TREE', 'GIT_PREFIX', 'GIT_GRAFT_FILE',
    'GIT_SHALLOW_FILE', 'GIT_REPLACE_REF_BASE', 'GIT_NAMESPACE', 'GIT_CONFIG',
    'GIT_CONFIG_PARAMETERS', 'GIT_CONFIG_COUNT']) delete env[key];
  for (const key of Object.keys(env))
    if (key.startsWith('GIT_CONFIG_KEY_') || key.startsWith('GIT_CONFIG_VALUE_')) delete env[key];
  const git = (args, cwd = root, input) => {
    const result = execGit('git', args, {cwd, shell: false, input,
      maxBuffer: 128 * 1024 * 1024, env});
    if (result.error || result.status !== 0) throw new Error(`git ${args[0]} failed`);
    return Buffer.isBuffer(result.stdout) ? result.stdout : Buffer.from(result.stdout ?? '');
  };
  const text = (args, cwd) => git(args, cwd).toString('utf8');
  const head = text(['rev-parse', '--verify', 'HEAD']).trim();
  const main = text(['rev-parse', '--verify', 'refs/heads/main']).trim();
  const items = [];
  const add = (kind, identity, details) => items.push({id: canonical([kind, ...identity]), kind,
    sha256: digest(details), summary: kind, details});
  const fields = text(['worktree', 'list', '--porcelain', '-z']).split('\0');
  const trees = [];
  let current;
  for (const field of fields) {
    if (!field) { if (current) trees.push(current); current = undefined; continue; }
    const space = field.indexOf(' '), key = space < 0 ? field : field.slice(0, space);
    const value = space < 0 ? true : field.slice(space + 1);
    if (key === 'worktree') current = {worktree: value};
    else if (current) current[key] = value;
    else throw new Error('invalid worktree list');
  }
  if (current) trees.push(current);
  if (!trees.length) throw new Error('empty worktree list');
  for (const tree of trees) {
    const worktree = realpathSync(tree.worktree);
    const records = text(['status', '--porcelain=v1', '-z', '--untracked-files=all'], worktree).split('\0');
    const workDiffSha256 = sha(git(['diff', '--binary', '--no-ext-diff', '--no-textconv'], worktree));
    const indexDiffSha256 = sha(git(['diff', '--cached', '--binary', '--no-ext-diff', '--no-textconv'], worktree));
    let dirty = false;
    for (let n = 0; n < records.length; n++) {
      const record = records[n];
      if (!record) continue;
      if (record.length < 4 || record[2] !== ' ') throw new Error('invalid status record');
      const xy = record.slice(0, 2), path = record.slice(3);
      let originalPath = null;
      if (/[RC]/.test(xy)) {
        originalPath = records[++n];
        if (!originalPath) throw new Error('invalid rename record');
      }
      dirty = true;
      add('file', [worktree, xy, path, originalPath], {worktree, xy, path, originalPath,
        contentSha256: contentSha256(worktree, path),
        originalContentSha256: originalPath ? contentSha256(worktree, originalPath) : null});
    }
    add('worktree', [worktree], {...tree, worktree, dirty, workDiffSha256, indexDiffSha256});
  }
  const patchCache = new Map();
  const patchId = commit => {
    if (!patchCache.has(commit)) {
      const patch = git(['show', '--format=', '--binary', '--no-ext-diff', '--no-textconv', commit, '--']);
      const output = git(['patch-id', '--stable'], root, patch).toString('utf8').trim();
      patchCache.set(commit, output ? output.split(/\s+/)[0] : null);
    }
    return patchCache.get(commit);
  };
  let mainPatches;
  for (const row of text(['for-each-ref', '--format=%(refname)%09%(objectname)', 'refs/heads']).trim().split('\n')) {
    if (!row) continue;
    const [branch, tip] = row.split('\t');
    if (!branch || !tip) throw new Error('invalid branch list');
    add('branch', [branch, tip], {branch, tip});
    const commits = text(['log', '--format=%H', `${main}..${tip}`]).trim().split('\n').filter(Boolean);
    const cherry = new Map(text(['cherry', main, tip]).trim().split('\n').filter(Boolean)
      .map(line => [line.slice(2), line[0]]));
    for (const commit of commits) {
      const id = patchId(commit), equivalents = [];
      if (cherry.get(commit) === '-') {
        mainPatches ??= text(['log', '--no-merges', '--format=%H', main]).trim().split('\n')
          .filter(Boolean).map(commit => ({commit, patchId: patchId(commit)}));
        equivalents.push(...mainPatches.filter(p => id && p.patchId === id));
      }
      add('commit', [branch, tip, commit], {branch, tip, commit, cherry: cherry.get(commit) ?? null,
        patchId: id, mainPatches: equivalents});
    }
  }
  // Subjects and patch bodies are deliberately never retained in the local JSON.
  const stashes = text(['stash', 'list', '--format=%H%x09%gd']).trim().split('\n').filter(Boolean);
  for (const row of stashes) {
    const [object, ref] = row.split('\t');
    if (!object || !/^stash@\{\d+\}$/.test(ref)) throw new Error('invalid stash list');
    const fields = text(['stash', 'show', '--include-untracked', '--name-status', '-z', ref]).split('\0');
    const paths = [];
    for (let n = 0; n < fields.length && fields[n]; n++) {
      const status = fields[n], path = fields[++n];
      if (!path) throw new Error('invalid stash path');
      const newPath = /^[RC]/.test(status) ? fields[++n] : null;
      if (/^[RC]/.test(status) && !newPath) throw new Error('invalid stash rename');
      paths.push({status, path, newPath});
    }
    const patchSha256 = sha(git(['stash', 'show', '--include-untracked', '--binary',
      '--no-ext-diff', '--no-textconv', ref]));
    add('stash', [object], {object, ref, paths, patchSha256});
  }
  items.sort((a, b) => a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
  const result = {head, main, items};
  return {...result, inventorySha256: digest(result)};
}

export function checkScope(inv, decisions) {
  const {head, main, items} = inv;
  if (!Array.isArray(items) || inv.inventorySha256 !== digest({head, main, items}) ||
      decisions?.inventorySha256 !== inv.inventorySha256) throw new Error('inventory digest mismatch');
  if (!Array.isArray(decisions.items) || decisions.items.length !== items.length)
    throw new Error('scope coverage mismatch');
  const remaining = new Map(items.map(item => [item.id, item]));
  if (remaining.size !== items.length) throw new Error('duplicate inventory item');
  for (const decision of decisions.items) {
    const item = remaining.get(decision.id);
    if (!item || !nonempty(decision.reason)) throw new Error('unknown, duplicate or unexplained scope item');
    remaining.delete(decision.id);
    if (decision.disposition === 'patch-equivalent') {
      const e = decision.evidence, d = item.details;
      if (item.kind !== 'commit' || d.cherry !== '-' || !d.patchId || !e ||
          e.commit !== d.commit || e.frozenMain !== main || e.patchId !== d.patchId ||
          e.mainPatchId !== d.patchId || !d.mainPatches.some(p => p.commit === e.mainCommit && p.patchId === e.mainPatchId) ||
          !nonempty(e.verification)) throw new Error('patch equivalence evidence incomplete');
    } else {
      if (!['include', 'exclude'].includes(decision.disposition) || !nonempty(decision.evidence))
        throw new Error('invalid disposition or evidence');
      if (decision.disposition === 'include' && (item.kind === 'file' || item.kind === 'stash' ||
          (item.kind === 'worktree' && item.details.dirty)))
        throw new Error('include requires verified commit and fresh inventory');
    }
  }
}

function localJson(root, name) {
  if (!nonempty(name) || isAbsolute(name) || !name.endsWith('.json')) throw new Error('local JSON path required');
  const path = resolve(root, name);
  if (!inside(root, path) || path === root) throw new Error('JSON path escapes root');
  let parent = path;
  while (parent !== root) {
    try {
      const stat = lstatSync(parent);
      if (stat.isSymbolicLink()) throw new Error('JSON symlink forbidden');
    } catch (error) { if (error.code !== 'ENOENT') throw error; }
    parent = dirname(parent);
  }
  return path;
}

function cli(args) {
  const command = args.shift(), options = {};
  if (!['scan', 'check'].includes(command)) throw new Error('expected scan or check');
  const allowed = ['--root', command === 'scan' ? '--output' : '--decisions'];
  while (args.length) {
    const key = args.shift(), value = args.shift();
    if (!allowed.includes(key) || options[key] !== undefined || !value || value.startsWith('--'))
      throw new Error('invalid CLI argument');
    options[key] = value;
  }
  if (allowed.some(key => !options[key])) throw new Error('missing CLI argument');
  const root = realpathSync(options['--root']);
  const path = localJson(root, options[allowed[1]]);
  const inv = inventory(root);
  if (command === 'scan') {
    mkdirSync(dirname(path), {recursive: true});
    writeFileSync(path, `${JSON.stringify(inv, null, 2)}\n`, {mode: 0o600});
  } else {
    checkScope(inv, JSON.parse(readFileSync(path, 'utf8')));
    console.log('scope verified');
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try { cli(process.argv.slice(2)); }
  catch { console.error('release audit failed: check arguments, Git inputs and scope evidence'); process.exitCode = 1; }
}
