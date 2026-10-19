import { spawnSync } from 'node:child_process';
import { existsSync, lstatSync, readFileSync, readdirSync, realpathSync } from 'node:fs';
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const names = ['init', 'baseline', 'claim', 'ask', 'propose', 'finish'];
const read = path => readFileSync(path, 'utf8');
const within = (root, path) => {
  const value = relative(root, path);
  return value === '' || (!isAbsolute(value) && value !== '..' && !value.startsWith(`..${sep}`));
};
const diagnostic = (code, path, message) => ({ code, path, message });

function loadContracts(root) {
  const canonicalRoot = realpathSync(resolve(root));
  const contractPath = join(canonicalRoot, 'references/cli-contracts.json');
  if (!within(canonicalRoot, realpathSync(contractPath))) throw new Error('contract path escapes package');
  const value = JSON.parse(read(contractPath));
  if (value.schema !== 1 || !Array.isArray(value.contracts) || !value.blocked_workflows || typeof value.blocked_workflows !== 'object' || Array.isArray(value.blocked_workflows)) throw new Error('invalid contracts schema');
  const seen = new Set();
  for (const contract of value.contracts) {
    if (!contract.id || seen.has(contract.id) || !Array.isArray(contract.help_argv) || !contract.help_argv.every(x => typeof x === 'string') || !Array.isArray(contract.required_tokens) || !contract.required_tokens.every(x => typeof x === 'string') || typeof contract.example_pattern !== 'string' || !contract.example_pattern.startsWith('^') || !contract.example_pattern.endsWith('$')) throw new Error('invalid command contract');
    // A damaged manifest must never turn the checker into a business command runner.
    if (!(contract.help_argv.length === 1 && contract.help_argv[0] === '--version') && !(contract.help_argv.at(-1) === '--help' && contract.help_argv.slice(0, -1).every(x => /^[a-z][a-z-]*$/.test(x)))) throw new Error('only help/version probes are allowed');
    new RegExp(contract.example_pattern);
    seen.add(contract.id);
  }
  return value;
}

function filesIn(root, errors) {
  const files = [], visited = new Set();
  function visit(path) {
    const rel = relative(root, path), actual = realpathSync(path);
    if (!within(root, actual)) { errors.push(diagnostic('path_escape', rel, '包内资源越过包根')); return; }
    const stat = lstatSync(path);
    if (stat.isSymbolicLink()) { errors.push(diagnostic('package_symlink', rel, '分发源码使用普通文件，安装链接由用户创建')); return; }
    if (stat.isDirectory()) {
      if (visited.has(actual)) return;
      visited.add(actual);
      if (/(^|\/)(hooks|mcp)(\/|$)/i.test(rel)) errors.push(diagnostic('unexpected_automation', rel, '技能包不分发 hooks/MCP'));
      for (const entry of readdirSync(path).sort()) visit(join(path, entry));
    } else if (stat.isFile()) {
      files.push(path);
      if (/seed/i.test(rel) && (stat.mode & 0o111 || rel.endsWith('SKILL.md'))) errors.push(diagnostic('unexpected_seed', rel, 'M1 不交付 seed 入口'));
    } else errors.push(diagnostic('unexpected_file', rel, '包内仅允许普通资源'));
  }
  visit(root);
  return files;
}

export function checkPackage(inputRoot) {
  const errors = [];
  let root, contracts, files;
  try { root = realpathSync(resolve(inputRoot)); files = filesIn(root, errors); if (errors.length) return errors; contracts = loadContracts(root); }
  catch (error) { return [...errors, diagnostic('invalid_package', String(inputRoot), error.message)]; }
  try {
    const manifest = JSON.parse(read(join(root, '.claude-plugin/plugin.json')));
    if (manifest.name !== 'towermd' || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(manifest.version) || typeof manifest.description !== 'string' || !manifest.description.trim() || Object.keys(manifest).some(key => !['name', 'version', 'description', 'author'].includes(key))) throw new Error('包身份或版本无效，不能包含自动执行配置');
    if (manifest.author !== undefined && (!manifest.author || typeof manifest.author !== 'object' || Array.isArray(manifest.author) || typeof manifest.author.name !== 'string' || !manifest.author.name.trim() || Object.keys(manifest.author).some(key => key !== 'name'))) throw new Error('署名元数据无效');
  } catch (error) { errors.push(diagnostic('invalid_manifest', '.claude-plugin/plugin.json', error.message)); }
  for (const required of ['INSTALL.md', 'references/session-contract.md', 'references/cli-contracts.json', 'scripts/check-contracts.mjs']) {
    if (!existsSync(join(root, required)) || !read(join(root, required)).trim()) errors.push(diagnostic('missing_resource', required, '缺共享资源'));
  }
  const expected = new Set(names.map(name => `skills/towermd-${name}/SKILL.md`));
  const actual = files.filter(path => path.endsWith(`${sep}SKILL.md`));
  const bodies = new Set();
  for (const path of actual) {
    const rel = relative(root, path), text = read(path), matter = /^---\n([\s\S]*?)\n---(?:\n|$)/.exec(text);
    if (!expected.delete(rel)) errors.push(diagnostic('unexpected_skill', rel, '仅允许六项规范技能来源'));
    const name = /^name: (\S+)$/m.exec(matter?.[1] ?? '')?.[1];
    if (!name || !/^description: \S.+$/m.test(matter?.[1] ?? '') || rel !== `skills/${name}/SKILL.md` || !text.includes('../../references/session-contract.md')) errors.push(diagnostic('invalid_skill', rel, '缺 name/description 或共享契约'));
    const body = text.slice(matter?.[0].length ?? 0).trim();
    if (!body || /尚未接入完整行为|\bplaceholder\b|\bstub\b/i.test(body)) errors.push(diagnostic('missing_workflow', rel, '未交付真实工作流'));
    if (bodies.has(body)) errors.push(diagnostic('duplicate_skill', rel, '技能正文重复'));
    bodies.add(body);
  }
  for (const missing of expected) errors.push(diagnostic('missing_skill', missing, '缺规范技能'));
  for (const [workflow, reason] of Object.entries(contracts.blocked_workflows)) {
    if (!names.includes(workflow) || typeof reason !== 'string' || !reason.trim()) errors.push(diagnostic('invalid_package', 'references/cli-contracts.json', '无效未完成流程'));
    else errors.push(diagnostic('missing_workflow', `skills/towermd-${workflow}/SKILL.md`, reason));
  }
  const registry = new Map(contracts.contracts.map(item => [item.id, item]));
  for (const path of files.filter(path => path.endsWith('.md'))) {
    const rel = relative(root, path), text = read(path);
    for (const match of text.matchAll(/\[[^\]]*\]\(([^\s)]+)\)/g)) {
      const target = match[1];
      if (/^https:\/\//.test(target) || target.startsWith('#')) continue;
      try {
        const local = decodeURIComponent(target.split('#')[0]);
        if (isAbsolute(local) || /^[a-z]+:/i.test(local)) throw new Error('禁止绝对或非本地资源');
        const resolved = realpathSync(resolve(dirname(path), local));
        if (!within(root, resolved)) throw new Error('链接越过包根');
      } catch (error) { errors.push(diagnostic('invalid_link', rel, `${target}: ${error.message}`)); }
    }
    let marker = null, inShell = false;
    for (const line of text.split('\n')) {
      const tag = /^<!-- towermd-cli:([a-z0-9-]+) -->$/.exec(line);
      if (tag && !inShell) marker = tag[1];
      if (/^```(?:sh|bash|shell)\s*$/.test(line)) { inShell = true; continue; }
      if (line.startsWith('```') && inShell) { inShell = false; marker = null; continue; }
      if (!inShell || !line.includes('$TOWERMD_CLI') || !line.trim().startsWith('"$TOWERMD_CLI"')) continue;
      const contract = registry.get(marker);
      if (!contract || !new RegExp(contract.example_pattern).test(line)) errors.push(diagnostic('unlisted_cli', rel, `未登记的命令示例: ${line}`));
    }
  }
  return errors;
}

export function checkCli(inputRoot, binary, run = spawnSync) {
  const errors = [];
  if (!isAbsolute(binary)) return [diagnostic('invalid_cli', binary, 'CLI 必须是明确的绝对路径')];
  let contracts, version;
  try {
    const root = realpathSync(resolve(inputRoot));
    contracts = loadContracts(root);
    const manifestPath = join(root, '.claude-plugin/plugin.json');
    if (!within(root, realpathSync(manifestPath))) throw new Error('manifest path escapes package');
    version = JSON.parse(read(manifestPath)).version;
  }
  catch (error) { return [diagnostic('invalid_package', inputRoot, error.message)]; }
  const cache = new Map();
  for (const contract of contracts.contracts) {
    const key = JSON.stringify(contract.help_argv);
    if (!cache.has(key)) cache.set(key, run(binary, contract.help_argv, { encoding: 'utf8', timeout: 5000, maxBuffer: 1024 * 1024, shell: false }));
    const result = cache.get(key), output = String(result?.stdout ?? '') + String(result?.stderr ?? '');
    if (result?.error || result?.status !== 0 || contract.required_tokens.some(token => !output.includes(token)) || (contract.help_argv[0] === '--version' && output.trim() !== `towermd ${version}`)) errors.push(diagnostic('missing_cli_contract', contract.id, `真实 help/version 不满足 ${key}`));
    if (/--(?:approve|actor|human|skip-gate)(?:[\s=,]|$)/m.test(output)) errors.push(diagnostic('unsafe_cli_contract', contract.id, 'CLI 不得提供人工批准或跳门禁参数'));
  }
  return errors;
}

export function main(args) {
  let root, cli;
  for (let i = 0; i < args.length; i += 2) {
    if (!args[i + 1] || !['--root', '--cli'].includes(args[i])) throw new Error('usage: check-contracts.mjs --root <package> --cli <absolute-binary>');
    if (args[i] === '--root') root = args[i + 1];
    else cli = args[i + 1];
  }
  if (!root || !cli) throw new Error('必须提供 --root 和 --cli');
  const diagnostics = [...checkPackage(root), ...checkCli(root, cli)];
  console.log(JSON.stringify({ ready: diagnostics.length === 0, diagnostics }, null, 2));
  return diagnostics.length ? 1 : 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { process.exitCode = main(process.argv.slice(2)); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
