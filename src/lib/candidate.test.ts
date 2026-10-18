import { beforeEach, expect, test, vi } from 'vitest';
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke }));
import { candidateFixture } from './candidate.fixture';
import { candidateClient, candidateGraph, decodeDraft, decodeScan, type CandidateDraft } from './candidate';

beforeEach(() => invoke.mockReset());
test('native invokes carry only token and issued scan id', async () => {
  const scanId = candidateFixture().scan_id;
  invoke.mockResolvedValue({ scan_id: scanId, state: 'pending' });
  await candidateClient.start('selected');
  await candidateClient.wait('selected', scanId);
  await candidateClient.draft('selected', scanId);
  expect(invoke.mock.calls).toEqual([
    ['start_candidate_scan', { token: 'selected' }], ['wait_candidate_scan', { token: 'selected', scanId }], ['load_candidate_draft', { token: 'selected', scanId }],
  ]);
});
test('wire rejects old synchronous output and wrong identities rather than fabricating completion', () => {
  expect(() => decodeScan({ state: 'prepared' })).toThrow('scan.scan_id');
  expect(() => decodeDraft(candidateFixture(), 'another-scan')).toThrow('scan.scan_id');
  const bad = candidateFixture(); bad.verification_report.commit = 'c'.repeat(40);
  expect(() => decodeDraft(bad, bad.scan_id)).toThrow('verification_report');
});
test('graph preserves exact coordinates, raw objects, raw text and all three verification states', () => {
  const draft = candidateFixture();
  const graph = candidateGraph(decodeDraft(draft, draft.scan_id) as CandidateDraft);
  expect(graph.nodes.map(node => node.status)).toEqual(['验证通过', '验证失败', '未验证 · 无可执行测试']);
  expect(graph.nodes.map(node => node.x)).toEqual([0, 400, 800]);
  expect(graph.nodes[0].content).toBe(draft.modules[0].text);
  expect(graph.nodes[0].raw).toBe(draft.canvas.nodes[0]);
  expect(graph.nodes[2].verification).toContain('"executions": []');
  expect(graph.nodes[2].verification).toContain('no_runnable_commands');
  expect(graph.nodes[2].verification).toContain('"keep": true');
  expect(draft.canvas.future).toEqual({ retained: true });
});
test('opaque canvas entries are retained with diagnostics while normal nodes still render', () => {
  const draft = candidateFixture(); draft.canvas.nodes.push(null, { type: 'future' });
  expect(candidateGraph(draft).nodes).toHaveLength(3);
  expect(candidateGraph(draft).diagnostics).toHaveLength(2);
  expect(draft.canvas.nodes).toHaveLength(5);
});

test('missing host, mismatched report and invalid execution references produce protocol errors', () => {
  const missing = candidateFixture(); delete (missing.evidence[0].coldstart as Record<string, unknown>).host;
  expect(() => decodeDraft(missing, missing.scan_id)).toThrow('coldstart.host');
  const wrong = candidateFixture(); (wrong.evidence[0].log as Record<string, unknown>).report_eid = 'another-report';
  expect(() => decodeDraft(wrong, wrong.scan_id)).toThrow('log.report_eid');
  const invalid = candidateFixture(); (invalid.evidence[2].coldstart as Record<string, unknown>).execution_indices = [99];
  expect(() => decodeDraft(invalid, invalid.scan_id)).toThrow('execution_indices[0]');
});

test('all four canvas types and non-tree relations retain raw data; only object bindings receive evidence', () => {
  const draft = candidateFixture();
  (draft.canvas.nodes[0] as Record<string, unknown>).id = 'bound-node';
  const annotations = [
    { id: 'A-pass', type: 'text', text: '用户卡片', x: 1, y: 2, width: 300, height: 160, extension: { untouched: true } },
    { id: 'link', type: 'link', url: 'https://example.invalid/只读', x: 10, y: 20, width: 300, height: 160 },
    { id: 'group', type: 'group', label: '自由分组', x: -100, y: -200, width: 900, height: 600 },
  ];
  draft.canvas.nodes.push(...annotations);
  draft.canvas.edges.push({ id: 'forward', fromNode: 'A-pass', toNode: 'link', label: '关系原文' }, { id: 'back', fromNode: 'link', toNode: 'A-pass' });
  const graph = candidateGraph(draft);
  expect(graph.nodes.map(node => node.kind)).toEqual(['file', 'file', 'file', 'text', 'link', 'group']);
  expect(graph.nodes[0].status).toBe('验证通过');
  expect(graph.nodes[3].status).toBe('候选 · 未批准');
  expect(graph.nodes[3].verification).toBeUndefined();
  expect(graph.nodes[3].raw).toBe(annotations[0]);
  expect(graph.edges).toEqual([['A-pass', 'link'], ['link', 'A-pass']]);
  expect((draft.canvas.edges[0] as Record<string, unknown>).label).toBe('关系原文');
});
