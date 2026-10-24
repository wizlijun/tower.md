import { describe, expect, it } from 'vitest'
import { costLabel, delegationAttention, moduleActivity, promptState, promptSummary, runtimeProjection, signalAttention, taskGraph, waiverAttention, type RuntimeTask } from './runtime'
import { delegated, delegationView } from '../test/delegation'
import type { GenericRequestView } from './api'
import { attentionCard, attentionSymbols } from './attention'

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
    expect(result.prompts[0].handled).toEqual({})
    expect(promptState(result.prompts[0], 'R-1')).toBe('已送达')
    expect(promptState(result.prompts[0], 'R-2')).toBe('排队中')
  })

  it('shows a prompt as handled only from the delegate receipt, never from delivery alone', () => {
    const projection = {
      Prompt: { P: { id: 'P', scope: { kind: 'global' }, text: '先测试', recipients: [
        { run: 'R-1', task: 'T-1', module: 'A' }, { run: 'R-2', task: 'T-2', module: 'A' }, { run: 'R-3', task: 'T-3', module: 'A' }] } },
      Delivery: {
        D1: { delivery: { run: 'R-1', message: { kind: 'prompt', id: 'P' } } },
        D2: { delivery: { run: 'R-2', message: { kind: 'prompt', id: 'P' } } },
      },
      PromptHandled: { D1: { delivery: { run: 'R-1', message: { kind: 'prompt', id: 'P' } }, note: '已补测试', event: 'e', handled_at: 't' } },
    }
    const [prompt] = runtimeProjection({ status: 'ready', projection }).prompts
    expect(prompt.handled).toEqual({ 'R-1': '已补测试' })
    expect(['R-1', 'R-2', 'R-3'].map((run) => promptState(prompt, run))).toEqual(['已处理', '已送达', '排队中'])
    expect(promptSummary(prompt)).toBe('1 已处理 · 1 已送达 · 1 排队中')
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

  it('names a process-configuration-only commit a minor change, still blocking until registered (FR-03)', () => {
    const alert = (id: string, kind: string) => ({ data: { reason: 'unapproved_protected_change', detail: { commit: id.repeat(40), class: kind } } })
    const result = runtimeProjection({ status: 'ready', projection: { Alert: { m: alert('a', 'minor'), M: alert('b', 'major') } } })
    const items = Object.fromEntries(result.allAttention.map((item) => [item.id, item]))
    expect([items.m.kind, items.m.title, items.m.commit]).toEqual(['禁止', '主干含未登记的流程配置变更（次要变更）', 'a'.repeat(40)])
    expect(items.m.action).toContain('转提案登记')
    expect([items.M.kind, items.M.title]).toEqual(['禁止', '主干含未经批准的人定内容变更'])
  })

  it('turns runner alerts into 错误 and 预算 cards and counts a failed task as stuck (12.1)', () => {
    const projection = {
      Task: { F: task('F', 'failed'), H: task('H', 'held') },
      Alert: {
        crash: { title: 'harness 崩溃', data: { reason: 'run_error', detail: { task: 'H', error: 'signal 11' } } },
        spent: { title: '运行达到预算上限', data: { reason: 'budget_exhausted', detail: { task: 'H' } } },
        unknown: { title: '设了预算，但这次运行没有报告费用', data: { reason: 'budget_unknown', detail: { task: 'F' } } },
      },
    }
    const result = runtimeProjection({ status: 'ready', projection })
    const kinds = Object.fromEntries(result.allAttention.map((item) => [item.id, [item.kind, item.tasks, item.detail]]))
    expect(kinds).toEqual({ crash: ['错误', ['H'], 'signal 11'], spent: ['预算', ['H'], undefined], unknown: ['预算', ['F'], undefined] })
    expect(result.allAttention.find((item) => item.id === 'unknown')?.action).toContain('不报告费用')
    expect(moduleActivity([task('F', 'failed')], [], ['A']).A.symbol).toBe('!')
  })

  it('turns task interrupts into 中断 cards that name the interrupt, and open reviews into 评审 (12.1, FR-58)', () => {
    const held = (id: string, kind: string, opened: string, gate?: string) =>
      ({ ...task(id, 'held'), active_run: `R-${id}`, interrupt: { kind, reason: `${kind} reason`, opened, gate } })
    const alert = (reason: string, taskId: string, error?: string) => ({ title: reason, data: { reason, detail: { task: taskId, error, severity: 'blocking' } } })
    const projection = {
      Task: { G: held('G', 'gate_failed', 'e-g', 'crap'), F: held('F', 'forbidden', 'e-f'), I: { ...task('I', 'proposed'), interrupt: { kind: 'missing_info', reason: 'G0', opened: 'e-i' } },
        E: held('E', 'run_error', 'e-e'), V: held('V', 'review', 'e-v') },
      Alert: { 'e-g': alert('gate_exhausted', 'G', '检查未通过'), 'e-old': alert('task_gate_failed', 'G'), 'e-f': alert('task_forbidden', 'F'),
        'e-i': alert('task_info_missing', 'I'), 'e-e': alert('run_error', 'E', 'signal 11') },
      Review: {
        'V-1': { id: 'V-1', request: { object: 'V', stage: 'plan', summary: '先拆解析器', artifacts: ['docs/plan.md'], attempt: 'R-V' }, requested: { at: '2026-10-24T09:00:00Z' } },
        'V-0': { id: 'V-0', request: { object: 'V', stage: 'code', summary: 'old' }, requested: { at: '2026-10-23T09:00:00Z' }, resolution: { verdict: 'passed' } },
      },
    }
    const result = runtimeProjection({ status: 'ready', projection })
    const items = Object.fromEntries(result.allAttention.map((item) => [item.id, item]))
    expect(Object.keys(items).sort()).toEqual(['e-e', 'e-f', 'e-g', 'e-i', 'review:V-1'])
    for (const [id, taskId] of [['e-g', 'G'], ['e-f', 'F'], ['e-i', 'I']]) {
      expect([items[id].kind, items[id].tasks, items[id].interrupt, items[id].delegation]).toEqual(['中断', [taskId], { task: taskId, opened: id }, taskId])
    }
    expect(items['e-g'].action).toContain('豁免')
    expect(items['e-f'].action).toContain('不能豁免')
    expect([items['e-e'].kind, items['e-e'].interrupt]).toEqual(['错误', { task: 'E', opened: 'e-e' }])
    expect([items['review:V-1'].kind, items['review:V-1'].review?.id, items['review:V-1'].delegation]).toEqual(['评审', 'V-1', 'V'])
    expect(result.reviews.map((review) => review.id)).toEqual(['V-1'])
    // A person is waited on: these lead ordinary alerts, and never rely on colour alone.
    expect(attentionSymbols['中断']).toBeTruthy()
    expect(attentionSymbols['评审']).toBeTruthy()
    expect(attentionCard(items['review:V-1']).ask).toBe('通过、要求修改或驳回')
  })

  it('raises unresolved spot checks and questions nobody can answer within the three-item budget', () => {
    const ask = (id: string, target: string, recipients: string[], extra: Partial<GenericRequestView> = {}): GenericRequestView => ({
      id, event: { at: '2026-10-21T10:00:00Z', eid: id, seq: 9, actor: 'human:local' },
      data: { target, ask: 'question', text: `问题 ${id}`, due: '2026-10-22T10:00:00Z', to: recipients },
      open: true, recipients, silence: null, ...extra,
    })
    const view = delegationView({
      spot_checks: [
        { object: 'T-1', attempt: 'A1', config: 'c', work_kind: 'A', done: { at: '2026-10-20T10:00:00Z', eid: 'D', seq: 3 }, rate: 0.3, draw: 0.1, resolved: false },
        { object: 'T-1', attempt: 'A0', config: 'c', work_kind: 'A', done: { at: '2026-10-19T10:00:00Z', eid: 'D0', seq: 2 }, rate: 0.3, draw: 0.1, resolved: true },
      ],
      requests: [ask('q-task', 'T-1', []), ask('q-module', 'mod-x', []), ask('q-addressed', 'T-1', ['claude-code/2.1']),
        ask('q-withdrawn', 'T-1', [], { open: false })],
    })
    const items = delegationAttention(view, [task('T-1')])
    expect(items.map((item) => [item.id, item.kind, item.delegation, item.modules])).toEqual([
      ['spot-check:T-1:A1', '抽查', 'T-1', ['A']],
      ['unanswerable:q-task', '无人可答', 'T-1', ['A']],
      ['unanswerable:q-module', '无人可答', undefined, []],
    ])
    expect(items[0].title).toBe('抽查：真实任务')
    expect([items[1].title, items[2].object]).toEqual(['无人可答：真实任务 · 问题 q-task', 'mod-x'])
    const state = runtimeProjection({ status: 'ready', delegation_view: { ...view, backlog: { count: 4, threshold: 3 } }, projection: {
      Task: { 'T-1': task('T-1', 'merged') }, Run: {},
      Alert: Object.fromEntries([1, 2, 3].map((n) => [`alert-${n}`, { data: { reason: `关注 ${n}` } }])),
    } })
    expect(state.attention).toHaveLength(3)
    expect(state.allAttention).toHaveLength(6)
    expect(state.backlog).toEqual({ count: 4, threshold: 3 })
    expect(runtimeProjection({ status: 'needs_recovery', delegation_view: view }).delegation).toBeNull()
  })

  it('offers one-step approval and confirmation only where the full flow would allow them (FR-138)', () => {
    const proposal = (status: string, extra: Record<string, unknown> = {}) => ({ status, at: '2026-10-21T00:00:00Z',
      proposal: { reason: '调整接口', impact: { modules: ['B'], tasks: [] }, draft_id: 'D-1', project_id: 'p', source_revision: 'r1', ...extra } })
    const lowering = { signals: [{ from: { id: 'process:towermd', kind: 'source' }, target: 'P-weak', count: 1,
      data: { kind: 'core.weakening', level: 'active', title: '承诺弱化：1 处' },
      last: { at: '2026-10-23T01:00:00Z', eid: 'w', seq: 1, actor: 'process:towermd' } }] }
    const state = runtimeProjection({ status: 'ready', projection: { Task: {}, Run: {}, TowerApi: lowering, Proposal: {
      'P-ok': proposal('submitted'), 'P-draft': proposal('draft'), 'P-weak': proposal('submitted'),
      'P-bad': proposal('submitted', { source_revision: 7 }),
    } } })
    const approve = Object.fromEntries(state.allAttention.filter((item) => item.kind === '待批提案').map((item) => [item.id, item.approve]))
    expect(approve).toEqual({ 'P-ok': { draft_id: 'D-1', project_id: 'p', source_revision: 'r1' }, 'P-draft': undefined, 'P-weak': undefined, 'P-bad': undefined })
    const check = (attempt: string) => ({ object: 'T-1', attempt, config: 'c', work_kind: 'A', done: { at: '2026-10-20T10:00:00Z', eid: 'D', seq: 3 },
      rate: 0.3, draw: 0.1, resolved: false })
    expect(delegationAttention(delegationView({ spot_checks: [check('A1')] }), [task('T-1')])[0].confirm)
      .toEqual({ object: 'T-1', attempt: 'A1', unproven: [2] })
    expect(delegationAttention(delegationView({ spot_checks: [check('A0')] }), [task('T-1')])[0].confirm).toBeUndefined()
    const unjudged = delegated()
    unjudged.card = { ...unjudged.card!, can_judge: false }
    expect(delegationAttention(delegationView({ objects: [unjudged], spot_checks: [check('A1')] }), [task('T-1')])[0].confirm).toBeUndefined()
  })

  it('keeps the person’s budget on the first screen and the rest under later (FR-128)', () => {
    const projection = { Task: {}, Run: {}, Alert: Object.fromEntries([1, 2, 3, 4].map((n) => [`alert-${n}`, { data: { reason: `关注 ${n}` } }])) }
    const state = runtimeProjection({ status: 'ready', projection, attention: { budget: 1, focus_time: [] } })
    expect([state.attention.length, state.later.length, state.allAttention.length, state.settings.budget]).toEqual([1, 3, 4, 1])
    expect(runtimeProjection({ status: 'ready', projection, attention: { budget: 7, focus_time: [] } }).attention).toHaveLength(3)
    expect(runtimeProjection({ status: 'ready', projection }).settings).toEqual({ budget: 3, focus_time: [] })
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

  it('turns Tower.md’s own weakening signal into a 承诺弱化 card, never a forged one', () => {
    const weakening = (actor: string, target: string) => ({
      from: { id: 'process:towermd', kind: 'source' }, target, count: 1,
      data: { kind: 'core.weakening', level: 'active', title: '承诺弱化：1 处', detail: '删除检查「cargo test」', value: 1 },
      last: { at: '2026-10-23T01:00:00Z', eid: `w-${target}`, seq: 1, actor },
    })
    const items = signalAttention({ signals: [weakening('process:towermd', 'P-1'), weakening('process:towermd', 'T-1'),
      weakening('process:towermd-cli', 'proj')] }, [task('T-1')])
    expect(items.map((item) => [item.kind, item.object, item.tasks, item.detail])).toEqual([
      ['承诺弱化', '提案 P-1', [], '删除检查「cargo test」'],
      ['承诺弱化', undefined, ['T-1'], '删除检查「cargo test」'],
      ['告警', 'proj', [], 'process:towermd 自报 · core.weakening · 1 · 删除检查「cargo test」'],
    ])
    expect(items[0].signal).toEqual({ from: 'process:towermd', kind: 'core.weakening', target: 'P-1', seen: 'w-P-1' })
  })

  it('turns Tower.md’s own silence and stuck signals into 失联 and 兜圈子 cards; passive silence stays out', () => {
    const own = (kind: string, level: string, target: string, actor = 'process:towermd') => ({
      from: { id: 'process:towermd', kind: 'source' }, target, count: 1,
      data: { kind, level, title: kind === 'core.silent' ? '请求无人回答' : '兜圈子', detail: `${kind} 依据` },
      last: { at: '2026-10-23T01:00:00Z', eid: `${kind}-${target}`, seq: 1, actor },
    })
    const items = signalAttention({ signals: [own('core.silent', 'active', 'T-1'), own('core.silent', 'passive', 'T-2'),
      own('core.stuck', 'active', 'T-1'), own('core.stuck', 'active', 'proj', 'claude-code/2.1')] }, [task('T-1')])
    expect(items.map((item) => [item.kind, item.priority, item.tasks, item.detail])).toEqual([
      ['失联', 3, ['T-1'], 'core.silent 依据'],
      ['兜圈子', 3, ['T-1'], 'core.stuck 依据'],
      ['告警', 5, [], 'process:towermd 自报 · core.stuck · core.stuck 依据'],
    ])
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

  it('lists a waiver only in its last day; a lapsed one is gone, since the gate blocks again (FR-72)', () => {
    const waiver = (id: string, object: string, until: string) => ({ id, object, gate: 'crap', reason: '上游修复前', until,
      granted: { at: '2026-10-20T00:00:00Z', eid: id, seq: 1, actor: 'human:local' } })
    const group = {
      W1: waiver('W1', 'T-1', '2026-10-25T00:00:00Z'), W2: waiver('W2', 'X-9', '2026-10-24T12:00:00Z'),
      W3: waiver('W3', 'T-1', '2026-10-26T00:00:00Z'), W4: waiver('W4', 'T-1', '2026-10-24T00:00:00Z'), broken: { id: 'W5' },
    }
    const now = Date.parse('2026-10-24T00:00:00Z')
    const items = waiverAttention(group, [task('T-1')], now)
    expect(items.map((item) => [item.id, item.kind, item.tasks, item.object, item.delegation])).toEqual([
      ['waiver:W1', '豁免到期', ['T-1'], undefined, 'T-1'],
      ['waiver:W2', '豁免到期', [], 'X-9', 'X-9'],
    ])
    expect(items[0].detail).toContain('不续期')
    expect(waiverAttention(group, [], Date.parse('2026-10-23T23:59:59Z')).map((item) => item.id)).toEqual(['waiver:W2', 'waiver:W4'])
    expect(waiverAttention(undefined)).toEqual([])
    const state = runtimeProjection({ status: 'ready', projection: { Task: { 'T-1': task('T-1') }, Run: {}, Waiver: group } }, now)
    expect(state.allAttention.map((item) => item.id).sort()).toEqual(['waiver:W1', 'waiver:W2'])
  })
})
