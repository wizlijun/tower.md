import type { CandidateDraft } from './candidate';

export function candidateFixture(): CandidateDraft {
  return {
    scan_id: '0192f0aa-0000-7000-8000-000000000001', state: 'completed',
    input: { project_id: 'project', revision: 'a'.repeat(40), tree: 'b'.repeat(40), root: '/source', common_dir: '/source/.git' },
    modules: ['A-pass', 'A-fail', 'A-empty'].map(id => ({ path: `arch/${id}.md`, text: `---\r\ntype: Module\r\nid: ${id}\r\n---\r\n<script>真实原文</script>` })),
    canvas: { nodes: ['A-pass', 'A-fail', 'A-empty'].map((id, i) => ({ id, type: 'file', file: `arch/${id}.md`, towermd: { obj: id }, x: i * 400, y: -20, width: 320, height: 180 })), edges: [], future: { retained: true } },
    evidence: ['passed', 'failed', 'no_tests'].map((result, i) => ({ id: `E-${i}`, module: ['A-pass', 'A-fail', 'A-empty'][i], result, shared: false, scan_id: '0192f0aa-0000-7000-8000-000000000001', commit: 'a'.repeat(40), tree: 'b'.repeat(40), log: { report_eid: 'actual-report', report_sha256: 'c'.repeat(64) },
      coldstart: { schema: 1, host: 'actual-host', local_only: true, diagnostics: i === 2 ? [{ code: 'no_runnable_commands', reason: '未发现可运行的验证命令', future: { keep: true } }] : [], execution_indices: i === 2 ? [] : [i] } })),
    report_eid: 'actual-report', verification_report: { commit: 'a'.repeat(40), tree: 'b'.repeat(40), executions: [{ result: 'passed' }, { result: 'failed' }] },
  };
}
