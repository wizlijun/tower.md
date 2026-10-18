import { invoke } from '@tauri-apps/api/core';
import type { DemoNode } from './demo';

export interface ScanStatus { scan_id: string; state: string; stages?: string[]; diagnostic?: string | null }
export interface CandidateDraft extends ScanStatus {
  input: { project_id: string; revision: string; tree: string; root: string; common_dir: string };
  modules: { path: string; text: string }[];
  canvas: { nodes: unknown[]; edges: unknown[]; [key: string]: unknown };
  evidence: Record<string, unknown>[];
  report_eid: string;
  verification_report: { commit: string; tree: string; executions: unknown[] };
  [key: string]: unknown;
}
export interface CandidateClient {
  start(token: string): Promise<ScanStatus>;
  wait(token: string, scanId: string): Promise<ScanStatus>;
  draft(token: string, scanId: string): Promise<CandidateDraft | ScanStatus>;
}

function object(value: unknown, path: string): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error(`${path}：需要对象`);
  return value as Record<string, unknown>;
}
function string(value: unknown, path: string): string {
  if (typeof value !== 'string' || !value) throw new Error(`${path}：需要非空字符串`);
  return value;
}
function array(value: unknown, path: string): unknown[] {
  if (!Array.isArray(value)) throw new Error(`${path}：需要数组`);
  return value;
}
export function decodeScan(value: unknown, expected?: string): ScanStatus {
  const row = object(value, 'scan');
  const id = string(row.scan_id, 'scan.scan_id');
  string(row.state, 'scan.state');
  if (expected && id !== expected) throw new Error('scan.scan_id：与正在等待的扫描不一致');
  if (row.stages !== undefined) array(row.stages, 'scan.stages').forEach((stage, i) => string(stage, `scan.stages[${i}]`));
  if (row.diagnostic !== undefined && row.diagnostic !== null && typeof row.diagnostic !== 'string') throw new Error('scan.diagnostic：需要文字或 null');
  return row as unknown as ScanStatus;
}
export function decodeDraft(value: unknown, expected: string): CandidateDraft | ScanStatus {
  const status = decodeScan(value, expected);
  if (status.state !== 'completed') return status;
  const row = object(value, 'draft');
  const input = object(row.input, 'draft.input');
  for (const key of ['project_id', 'revision', 'tree', 'root', 'common_dir']) string(input[key], `draft.input.${key}`);
  array(row.modules, 'draft.modules').forEach((module, i) => {
    const item = object(module, `draft.modules[${i}]`);
    string(item.path, `draft.modules[${i}].path`);
    string(item.text, `draft.modules[${i}].text`);
  });
  const canvas = object(row.canvas, 'draft.canvas');
  array(canvas.nodes, 'draft.canvas.nodes');
  array(canvas.edges, 'draft.canvas.edges');
  string(row.report_eid, 'draft.report_eid');
  const report = object(row.verification_report, 'draft.verification_report');
  if (report.commit !== input.revision || report.tree !== input.tree) throw new Error('draft.verification_report：来源与固定扫描不一致');
  const executions = array(report.executions, 'draft.verification_report.executions');
  array(row.evidence, 'draft.evidence').forEach((evidence, i) => {
    const path = `draft.evidence[${i}]`;
    const item = object(evidence, path);
    for (const key of ['id', 'module', 'result']) string(item[key], `${path}.${key}`);
    if (item.scan_id !== status.scan_id || item.commit !== input.revision || item.tree !== input.tree) throw new Error(`${path}：来源与固定扫描不一致`);
    if (typeof item.shared !== 'boolean') throw new Error(`${path}.shared：需要布尔值`);
    const log = object(item.log, `${path}.log`);
    if (log.report_eid !== row.report_eid) throw new Error(`${path}.log.report_eid：与共享报告不一致`);
    string(log.report_sha256, `${path}.log.report_sha256`);
    const marker = object(item.coldstart, `${path}.coldstart`);
    if (marker.schema !== 1 || marker.local_only !== true) throw new Error(`${path}.coldstart：缺少本机存储声明`);
    string(marker.host, `${path}.coldstart.host`);
    array(marker.execution_indices, `${path}.coldstart.execution_indices`).forEach((index, n) => {
      if (typeof index !== 'number' || !Number.isSafeInteger(index) || index < 0 || index >= executions.length) throw new Error(`${path}.coldstart.execution_indices[${n}]：报告执行索引无效`);
    });
    array(marker.diagnostics, `${path}.coldstart.diagnostics`);
  });
  return row as unknown as CandidateDraft;
}
export const candidateClient: CandidateClient = {
  start: async token => decodeScan(await invoke('start_candidate_scan', { token })),
  wait: async (token, scanId) => decodeScan(await invoke('wait_candidate_scan', { token, scanId }), scanId),
  draft: async (token, scanId) => decodeDraft(await invoke('load_candidate_draft', { token, scanId }), scanId),
};
export function isDraft(value: CandidateDraft | ScanStatus): value is CandidateDraft {
  return value.state === 'completed' && 'canvas' in value;
}
const results: Record<string, string> = { passed: '验证通过', failed: '验证失败', no_tests: '未验证 · 无可执行测试' };
export function candidateGraph(draft: CandidateDraft): { nodes: DemoNode[]; edges: string[][]; diagnostics: string[] } {
  const diagnostics: string[] = [];
  const nodes: DemoNode[] = [];
  for (const [index, value] of draft.canvas.nodes.entries()) {
    if (!value || typeof value !== 'object' || Array.isArray(value)) { diagnostics.push(`节点 ${index + 1} 无法显示，原条目仍保留。`); continue; }
    const row = value as Record<string, unknown>;
    if (typeof row.id !== 'string' || !['x', 'y', 'width', 'height'].every(key => Number.isSafeInteger(row[key]))) {
      diagnostics.push(`节点 ${index + 1} 缺少可显示的身份或坐标，原条目仍保留。`); continue;
    }
    const binding = row.towermd && typeof row.towermd === 'object' ? row.towermd as Record<string, unknown> : {};
    const objectId = typeof binding.obj === 'string' ? binding.obj : undefined;
    const moduleId = objectId ?? row.id;
    const module = draft.modules.find(module => module.path === row.file);
    const evidence = objectId ? draft.evidence.find(evidence => evidence.module === objectId) : undefined;
    const marker = evidence?.coldstart && typeof evidence.coldstart === 'object' ? evidence.coldstart as Record<string, unknown> : {};
    const indices = Array.isArray(marker.execution_indices) ? marker.execution_indices.filter((i): i is number => Number.isInteger(i) && i >= 0) : [];
    const result = evidence ? String(evidence.result) : '';
    const group = row.type === 'group';
    const known = ['file', 'text', 'link', 'group'].includes(String(row.type));
    nodes.push({
      id: row.id, kind: String(row.type), raw: value,
      x: row.x as number, y: row.y as number, width: row.width as number, height: row.height as number,
      title: typeof row.label === 'string' ? row.label : moduleId,
      layer: group ? '候选分区' : module ? 'L1 · 候选模块' : '候选画布',
      description: '固定提交的真实扫描结果，尚未经过 S6 综合与人工批准。',
      status: group ? '候选分区 · 未批准' : results[result] ?? (result || (known ? '候选 · 未批准' : '未知条目 · 只读')),
      tone: result === 'failed' || result === 'no_tests' || !known ? 'review' : 'module',
      symbol: result === 'passed' ? '✓' : result === 'failed' ? '×' : result === 'no_tests' ? '△' : '◇',
      facts: [ ['扫描 ID', draft.scan_id], ['来源提交', draft.input.revision], ['Git 树', draft.input.tree],
        ['共享报告', draft.report_eid], ...(evidence ? [['证据', String(evidence.id)], ['日志位置', `仅本机 · ${String(marker.host)}`]] as [string, string][] : []) ],
      content: module?.text ?? (row.type === 'text' ? String(row.text ?? '') : JSON.stringify(row, null, 2)),
      verification: evidence ? JSON.stringify({ result: evidence.result, shared: evidence.shared, diagnostics: marker.diagnostics,
        executions: indices.map(index => ({ index, execution: draft.verification_report.executions[index] })) }, null, 2) : undefined,
    });
  }
  const ids = new Set(nodes.map(node => node.id));
  const edges = draft.canvas.edges.flatMap(value => {
    if (!value || typeof value !== 'object' || Array.isArray(value)) return [];
    const row = value as Record<string, unknown>;
    return typeof row.fromNode === 'string' && typeof row.toNode === 'string' && ids.has(row.fromNode) && ids.has(row.toNode) ? [[row.fromNode, row.toNode]] : [];
  });
  return { nodes, edges, diagnostics };
}
