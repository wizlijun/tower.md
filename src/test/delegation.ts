import type { DelegationEvidence, DelegationObject, DelegationView, TrackRecord } from '../lib/api'

const stamp = (seq: number) => ({ at: '2026-10-20T10:00:00Z', eid: `E-${seq}`, seq })

export function evidence(id: string, source_kind: DelegationEvidence['source_kind'], criteria: number[], passed: boolean | null = null): DelegationEvidence {
  return { id, source_kind, criteria, passed, version: null, actor: source_kind === 'used' ? 'human:local' : 'claude-code/2.0',
    at: '2026-10-20T10:00:00Z', note: null, attempt: 'A1' }
}

/** A delegated task the checks merged: two criteria, one proven by a check, one only claimed. */
export function delegated(overrides: Partial<DelegationObject> = {}): DelegationObject {
  return {
    id: 'T-1', title: '真实任务', phase: 'done', work_kind: 'A', default_work_kind: 'A',
    brief: { why: '用户需要', done_when: ['测试通过', '文档更新', '不改接口'] }, brief_set_by: 'human:local', clarification: null,
    human_can_complete: false,
    attempts: [{ id: 'A1', config: 'cfg-1', delegate: { name: 'claude-code', version: '2.0' }, delegated: stamp(1),
      delivered: stamp(2), done: stamp(3), done_by: 'check', active: false, version: 'abc', out_of_bounds: null, verdict: null }],
    card: {
      attempt: 'A1', version: 'abc', can_judge: true, unproven: [2],
      rows: [
        { index: 0, text: '测试通过', evidence: [evidence('E-c', 'check', [0], true)], unproven: false },
        { index: 1, text: '文档更新', evidence: [evidence('E-s', 'claimed', [1])], unproven: false },
        { index: 2, text: '不改接口', evidence: [], unproven: true },
      ],
      whole: [evidence('E-w', 'check', [], true)],
    },
    completion: { verdict: 'passed', version: 'abc', gates: [
      { gate: 'core.verify', check: 'core.verify', hard: true, builtin: true, status: 'passed', evidence: ['E-w'] },
    ] },
    ...overrides,
  }
}

export function trackRecord(overrides: Partial<TrackRecord> = {}): TrackRecord {
  return {
    config: 'cfg-1', delegate: { name: 'claude-code', version: '2.0' }, work_kind: 'A', records: 2,
    first_pass: { count: 1, total: 2, rate: 0.5 }, on_time: { count: 0, total: 0, rate: null }, loops: null, weakened: null,
    met_done_when: { count: 2, total: 2, rate: 1 }, out_of_bounds: null,
    delegate_to_deliver_ms: { count: 2, median: 90 * 60_000, total: 180 * 60_000 }, review_ms: { count: 0, median: null, total: 0 },
    sent_back: { count: 2, median: 0, total: 1 }, config_changed: true, spot_check_rate: 0.3, level: '先问后做',
    level_source: 'default', suggestion: { direction: 'up', level: '做完汇报' },
    ...overrides,
  }
}

export function delegationView(overrides: Partial<DelegationView> = {}): DelegationView {
  return {
    objects: [delegated()], track_record: [trackRecord()], configs: [], spot_checks: [], backlog: null, report_requests: [],
    rules: [], renames: {}, levels: [],
    settings: { review_backlog: 3, spot_check_min: 0.1, track_record_window: 20, delegation_levels: ['先问后做', '做完汇报', '只看结果'] },
    ...overrides,
  }
}
