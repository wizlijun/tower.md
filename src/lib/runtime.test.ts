import { describe, expect, it } from 'vitest'
import { costLabel, delegationAttention, moduleActivity, runtimeProjection, signalAttention, taskGraph, type RuntimeTask } from './runtime'
import { delegationView } from '../test/delegation'
import type { ReportRequestView } from './api'

function task(id: string, state: RuntimeTask['state'] = 'ready'): RuntimeTask {
  return { input: { id, title: id, module: 'A', touches: [], allowed_paths: ['src/**'], depends_on: [], acceptance: [] },
    state, baseline: 'towermd/B0', needs_review: false, active_run: null }
}

describe('real runtime canvas projection', () => {
  it('keeps existing task coordinates when state, ordering or dependency changes', () => {
    const positions = new Map<string, { x: number; y: number }>()
    const initial = taskGraph([task('T-2'), task('T-3')], positions, 'A')
    const changed = task('T-3', 'running')
    changed.input.depends_on = ['T-2']
    const updated = taskGraph([task('T-1'), changed, task('T-2', 'merged')], positions, 'A')
    for (const old of initial.nodes) {
      const node = updated.nodes.find((node) => node.id === old.id)!
      expect([node.x, node.y, node.width, node.height]).toEqual([old.x, old.y, old.width, old.height])
    }
    expect(updated.edges).toHaveLength(1)
    expect(updated.edges[0]).toMatchObject({ from_node: 'T-2', to_node: 'T-3' })
    expect(updated.nodes.find((node) => node.id === 'T-3')!.content).toBe('执行中')
    const reopened = taskGraph([task('T-2')], positions, 'A')
    expect(reopened.nodes[0].x).toBe(initial.nodes[0].x)
  })

  it('preserves null and missing costs as unknown and limits attention to three facts', () => {
    expect(costLabel(null)).toBe('未知（未报告）')
    expect(costLabel(undefined)).toBe('未知（未报告）')
    expect(costLabel(0)).toBe('$0.0000')
    const state = runtimeProjection({ status: 'ready', projection: {
      Task: { 'T-1': task('T-1', 'running') }, Run: {},
      Alert: Object.fromEntries([1, 2, 3, 4].map((n) => [`alert-${n}`, { data: { reason: `关注 ${n}` } }])),
    } })
    expect(state.attention).toHaveLength(3)
    expect(moduleActivity(state.tasks).A).toMatchObject({ label: '1 个任务 · 1 运行', symbol: '▶' })
    expect(runtimeProjection({ status: 'needs_recovery', projection: { Task: { 'T-1': task('T-1') } } }).tasks).toEqual([])
  })

  it('uses accepted answer and per-run delivery facts, not prompt submission as delivery', () => {
    const projection = {
      Hold: { H: { draft: { id: 'H', text: '如何实现？', options: [], recommended: null, context: [] },
        binding: { run: 'R-1', task: 'T-1', module: 'A' }, targets: [] } },
      Answer: { H: { input: { hold: 'H', text: '沿用现有实现', remember_as: { kind: 'temporary' } }, event: 'answer-id', proposal: null } },
      Prompt: { P: { id: 'P', scope: { kind: 'global' }, text: '先测试', recipients: [{ run: 'R-1', task: 'T-1', module: 'A' }, { run: 'R-2', task: 'T-2', module: 'A' }] } },
      Delivery: { D: { delivery: { run: 'R-1', message: { kind: 'prompt', id: 'P' } } } },
    }
    const result = runtimeProjection({ status: 'ready', projection })
    expect(result.attention).toEqual([])
    expect(result.questions).toHaveLength(1)
    expect(result.prompts[0].delivered).toEqual(['R-1'])
    expect(result.prompts[0].recipients).toHaveLength(2)
  })

  it('shows real blocking facts on module cards and ranks them by FR-128 without moving tasks', () => {
    const projection = {
      Task: { T: task('T', 'running') },
      Alert: {
        warning: { at: '2026-10-20T00:00:00Z', data: { reason: 'unknown_trailer', detail: { severity: 'warning' } } },
        failure: { at: '2026-10-21T00:00:00Z', data: { reason: 'task_gate_failed', detail: { task: 'T', error: '测试未通过' } } },
        violation: { data: { reason: 'unapproved_protected_change', detail: { commit: 'a'.repeat(40) } } },
      },
      Hold: {
        newer: { draft: { id: 'newer', text: '新问题', options: [], context: [] }, binding: { task: 'T', module: 'A' }, targets: [], opened_at: 200 },
        older: { draft: { id: 'older', text: '旧问题', options: [], context: [] }, binding: { task: 'T', module: 'A' }, targets: [], opened_at: 100 },
      },
      Proposal: { P: { status: 'submitted', proposal: { reason: '调整接口', impact: { modules: ['B'], tasks: ['T-B'] } } } },
    }
    const result = runtimeProjection({ status: 'ready', projection })
    // Protection first, then what waits on a person (longest wait first), then the rest by reach.
    expect(result.attention.map((item) => item.id)).toEqual(['violation', 'older', 'newer'])
    expect(result.allAttention.map((item) => item.id)).toEqual(['violation', 'older', 'newer', 'P', 'failure', 'warning'])
    expect(result.later.map((item) => item.id)).toEqual(['P', 'failure', 'warning'])
    const badges = moduleActivity(result.tasks, result.allAttention, ['A', 'B'])
    expect(badges.A.tone).toBe('failed')
    expect(badges.A.label).toContain('禁止')
    expect(badges.B.tone).toBe('failed')
    const localOnly = result.allAttention.filter((item) => item.modules.length)
    expect(moduleActivity(result.tasks, localOnly, ['B']).B.label).toContain('待批提案')
    expect(result.allAttention.find((item) => item.id === 'failure')?.detail).toBe('测试未通过')
  })

  it('raises unresolved spot checks and active overdue reports within the three-item budget; passive ones stay in details', () => {
    const report = (id: string, level: 'active' | 'passive', extra: Partial<ReportRequestView> = {}): ReportRequestView => ({
      id, object: 'T-1', attempt: 'A1', checkpoint: 0, text: `检查点 ${id}`, due: '2026-10-20T10:00:00Z', open: true, reply: null,
      overdue: { event: { at: '2026-10-21T10:00:00Z', eid: `O-${id}`, seq: 9, actor: 'process:towermd' }, data: { level } }, ...extra,
    })
    const view = delegationView({
      spot_checks: [
        { object: 'T-1', attempt: 'A1', config: 'c', work_kind: 'A', done: { at: '2026-10-20T10:00:00Z', eid: 'D', seq: 3 }, rate: 0.3, draw: 0.1, resolved: false },
        { object: 'T-1', attempt: 'A0', config: 'c', work_kind: 'A', done: { at: '2026-10-19T10:00:00Z', eid: 'D0', seq: 2 }, rate: 0.3, draw: 0.1, resolved: true },
      ],
      report_requests: [report('R-a', 'active'), report('R-p', 'passive'), report('R-closed', 'active', { open: false })],
    })
    const items = delegationAttention(view, [task('T-1')])
    expect(items.map((item) => [item.id, item.kind, item.delegation, item.modules])).toEqual([
      ['spot-check:T-1:A1', '抽查', 'T-1', ['A']],
      ['report-overdue:R-a', '汇报过期', 'T-1', ['A']],
    ])
    expect(items[0].title).toBe('抽查：真实任务')
    const state = runtimeProjection({ status: 'ready', delegation_view: { ...view, backlog: { count: 4, threshold: 3 } }, projection: {
      Task: { 'T-1': task('T-1', 'merged') }, Run: {},
      Alert: Object.fromEntries([1, 2, 3].map((n) => [`alert-${n}`, { data: { reason: `关注 ${n}` } }])),
    } })
    expect(state.attention).toHaveLength(3)
    expect(state.allAttention).toHaveLength(5)
    expect(state.backlog).toEqual({ count: 4, threshold: 3 })
    expect(runtimeProjection({ status: 'needs_recovery', delegation_view: view }).delegation).toBeNull()
  })

  it('raises merged active and time-sensitive signals as self-reported alerts; log and passive stay out', () => {
    const signal = (kind: string, level: string, target: string, extra: Record<string, unknown> = {}) => ({
      from: { id: 'ci/nightly', kind: 'source' }, target, count: 1, at: '2026-10-23T10:00:00Z',
      data: { kind, level, title: `${kind} 偏高` }, first: { at: '2026-10-23T01:00:00Z', eid: 'e1', seq: 1, actor: 'process:towermd-cli' },
      last: { at: '2026-10-23T01:00:00Z', eid: 'e1', seq: 1, actor: 'process:towermd-cli' }, ...extra,
    })
    const group = { reports: [], signals: [
      signal('eng.crap', 'active', 'T-1', { count: 3, data: { kind: 'eng.crap', level: 'active', title: 'CRAP 偏高', value: 44, threshold: 30 } }),
      signal('ops.disk', 'time_sensitive', 'proj'), signal('eng.lint', 'passive', 'T-1'), signal('eng.log', 'log', 'T-1'),
    ] }
    const items = signalAttention(group, [task('T-1')])
    expect(items.map((item) => [item.id, item.kind, item.priority, item.tasks, item.modules])).toEqual([
      ['signal:ci/nightly:eng.crap:T-1', '告警', 5, ['T-1'], ['A']],
      ['signal:ci/nightly:ops.disk:proj', '告警', 2, [], []],
    ])
    expect(items[0].title).toBe('CRAP 偏高 ×3')
    expect(items[0].detail).toBe('ci/nightly 自报 · eng.crap · 44 / 30')
    expect(signalAttention(undefined)).toEqual([])
    const state = runtimeProjection({ status: 'ready', projection: { Task: { 'T-1': task('T-1') }, Run: {}, TowerApi: group } })
    expect(state.attention.map((item) => item.id)).toEqual(['signal:ci/nightly:ops.disk:proj', 'signal:ci/nightly:eng.crap:T-1'])
    expect(state.attention[0].urgent).toBe(true)
    expect(state.attention[0].signal).toEqual({ from: 'ci/nightly', kind: 'ops.disk', target: 'proj', seen: 'e1' })
  })

  it('drops signals a person acknowledged or muted, and lets an escalation through a mute', () => {
    const now = Date.parse('2026-10-23T12:00:00Z')
    const entry = (kind: string, level: string, extra: Record<string, unknown>) => ({
      from: { id: 'ci/nightly', kind: 'source' }, target: 'proj', count: 1, data: { kind, level, title: kind },
      last: { at: '2026-10-23T01:00:00Z', eid: `e-${kind}`, seq: 1, actor: 'process:towermd-cli' }, ...extra,
    })
    const mark = { at: '2026-10-23T02:00:00Z', eid: 'ack', actor: 'human:local' }
    const group = { signals: [
      entry('acked', 'active', { acknowledged: mark }),
      entry('muted', 'active', { muted: { until: '2026-10-30T00:00:00Z', level: 'active', event: mark } }),
      entry('escalated', 'time_sensitive', { muted: { until: '2026-10-30T00:00:00Z', level: 'active', event: mark } }),
      entry('expired', 'active', { muted: { until: '2026-10-22T00:00:00Z', level: 'active', event: mark } }),
    ] }
    expect(signalAttention(group, [], now).map((item) => item.signal?.kind)).toEqual(['escalated', 'expired'])
    const state = runtimeProjection({ status: 'ready', projection: { TowerApi: group } }, now)
    expect(state.quieted).toBe(2)
  })
})
