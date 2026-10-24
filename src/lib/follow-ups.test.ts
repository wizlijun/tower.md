import { describe, expect, it } from 'vitest'
import type { FollowUpEntry, Rhythm } from './api'
import { followUpsFrom, followUpTextError, recap, rhythmAttention, rhythmFrom, rhythmMarks, whenLabel } from './follow-ups'
import { runtimeProjection, type RuntimeTask } from './runtime'
import { delegated, delegationView } from '../test/delegation'
import { daemonList } from '../test/attention'
import type { QuestionCard } from './human'

const mark = (seq: number, at = '2026-10-20T10:00:00Z', actor = 'human:local') => ({ at, eid: `e${seq}`, seq, actor })
function task(id: string): RuntimeTask {
  return { input: { id, title: id, module: 'A', touches: [], allowed_paths: ['src/**'], depends_on: [], acceptance: [] },
    state: 'running', baseline: 'towermd/B0', needs_review: false, active_run: null }
}
const follow = (id: string, when: FollowUpEntry['when'], seq = 1, done = false): FollowUpEntry =>
  ({ id, text: `跟进 ${id}`, when, added: mark(seq), ...(done ? { done: mark(seq + 10) } : {}) })
const empty = () => ({ conclusion: null, overturned: [], open: [] })

describe('follow-ups', () => {
  it('reads only well-formed follow-ups, open ones first and newest first', () => {
    const projection = { FollowUp: {
      a: follow('a', { on: 'date', at: '2026-11-01T00:00:00Z' }, 1),
      b: follow('b', { on: 'done', target: 'T-1' }, 3, true),
      c: follow('c', { on: 'activity', target: 'T-1' }, 2),
      broken: { id: 'broken', text: 'x', when: { on: 'done' }, added: mark(4) },
    } }
    expect(followUpsFrom(projection).map((entry) => entry.id)).toEqual(['c', 'a', 'b'])
    expect(followUpsFrom(null)).toEqual([])
  })

  it('checks a sentence the way the daemon does and states each condition in words', () => {
    expect(followUpTextError(' ')).toBe('先写一句话。')
    expect(followUpTextError('一行\n两行')).toMatch(/换行/)
    expect(followUpTextError('长'.repeat(201))).toMatch(/200/)
    expect(followUpTextError('回头看看')).toBeNull()
    expect(whenLabel({ on: 'done', target: 'T-1' }, () => '登录')).toBe('登录 完成时')
    expect(whenLabel({ on: 'activity', target: 'T-1' })).toBe('T-1 有新动静时')
    expect(whenLabel({ on: 'signal', target: 'T-1', kind: 'ci.failed' })).toBe('T-1 出现 ci.failed 信号时')
    expect(whenLabel({ on: 'date', at: '明天' })).toBe('到 明天')
    expect(whenLabel({ on: 'date', at: '2026-11-01T00:00:00Z' })).toMatch(/^到 2026-1[01]-\d\d \d\d:\d\d$/)
  })

  it('reads the daemon rhythm leniently', () => {
    expect(rhythmFrom(undefined)).toBeNull()
    expect(rhythmFrom({ dormant_after: '14', objects: [] })).toBeNull()
    expect(rhythmFrom({ dormant_after: 14, objects: [{ id: 'p', quiet: false, last: null, resurfaced: null }, { id: 3 }], due: ['F', 4] }))
      .toEqual({ dormant_after: 14, objects: [{ id: 'p', quiet: false, last: null, resurfaced: null }], due: ['F'] })
  })
})

describe('resurfacing and quiet objects', () => {
  const rhythm: Rhythm = { dormant_after: 14, due: ['F1', 'F2'], objects: [
    { id: 'proj', last: mark(9), quiet: false, resurfaced: null },
    { id: 'T-1', last: mark(2), quiet: false, resurfaced: { by: 'follow_up', follow_up: 'F1' } },
    { id: 'T-2', last: mark(3, '2026-10-01T00:00:00Z'), quiet: false, resurfaced: { by: 'check_every', days: 7 } },
    { id: 'T-3', last: mark(4), quiet: true, resurfaced: null },
  ] }

  it('turns due follow-ups and resurfaced objects into class-4 items with a recap', () => {
    const followUps = [follow('F1', { on: 'activity', target: 'T-1' }), follow('F2', { on: 'date', at: '2026-10-01T00:00:00Z' }),
      follow('F3', { on: 'done', target: 'T-3' })]
    const items = rhythmAttention({ rhythm, followUps, tasks: [task('T-1'), task('T-2')], delegation: null, recap: (id) => ({ ...empty(), conclusion: `结论 ${id}` }) })
    expect(items.map((item) => [item.id, item.kind, item.followUp ?? item.resurfaced])).toEqual([
      ['follow-up:F1', '跟进', 'F1'], ['follow-up:F2', '跟进', 'F2'], ['resurfaced:T-2', '浮现', 'T-2']])
    expect(items[0]).toMatchObject({ tasks: ['T-1'], modules: ['A'], recap: { conclusion: '结论 T-1' } })
    expect(items[0].detail).toMatch(/T-1 已休眠，现在浮现/)
    expect(items[1]).toMatchObject({ tasks: [], object: undefined, recap: undefined })
    expect(items[2]).toMatchObject({ title: 'T-2 重新浮现', detail: '到了 7 天的复查节奏', recap: { conclusion: '结论 T-2' } })
    expect(items.every((item) => ['跟进', '浮现'].includes(item.kind) && !item.urgent)).toBe(true)
    expect(rhythmAttention({ rhythm: null, followUps, tasks: [], delegation: null, recap: empty })).toEqual([])
  })

  it('marks quiet and resurfaced canvas cards, the project card under its own key', () => {
    expect(rhythmMarks({ ...rhythm, objects: [{ ...rhythm.objects[0], quiet: true }, ...rhythm.objects.slice(1)] })).toEqual({
      proj: 'quiet', 'project-proj': 'quiet', 'T-1': 'resurfaced', 'T-2': 'resurfaced', 'T-3': 'quiet' })
    expect(rhythmMarks(null)).toEqual({})
  })

  it('recaps the latest conclusion, what was overturned and what is still open', () => {
    const view = delegationView({ objects: [delegated({ attempts: [
      { ...delegated().attempts[0], id: 'A1', verdict: { kind: 'sent_back', event: mark(3, '2026-10-01T00:00:00Z'), review_ms: 1, unproven: [], reason: '接口变了', takeaway: null } },
      { ...delegated().attempts[0], id: 'A2', verdict: { kind: 'confirmed', event: mark(5, '2026-10-03T00:00:00Z'), review_ms: 1, unproven: [], reason: null, takeaway: { text: '按新接口做', set_by: 'human:local' } } },
    ] })] })
    const towerApi = { reports: [
      { from: { id: 'notemd', kind: 'source' }, target: 'T-1', data: { state: 'in_progress', summary: '有新笔记引用' }, last: mark(6, '2026-10-05T00:00:00Z') },
      { from: { id: 'ci', kind: 'source' }, target: 'T-9', data: { state: 'in_progress', summary: '别的对象' }, last: mark(7, '2026-10-06T00:00:00Z') },
    ] }
    const question = { draft: { id: 'H-1', text: '沿用旧接口吗？', options: [], context: [] }, binding: { module: 'A', task: 'T-1', run: 'R' }, targets: [] } as unknown as QuestionCard
    const proposals = { 'P-1': { status: 'rejected', at: '2026-10-02T00:00:00Z', proposal: { reason: '放宽检查', impact: { tasks: ['T-1'] } } } }
    const result = recap('T-1', { delegation: view, towerApi, questions: [question], answers: {}, proposals,
      followUps: [follow('F1', { on: 'done', target: 'T-1' }), follow('F2', { on: 'done', target: 'T-1' }, 2, true)] })
    expect(result).toEqual({ conclusion: 'notemd 汇报：有新笔记引用', overturned: ['驳回提案 P-1：放宽检查', '退回：接口变了'],
      open: ['问题：沿用旧接口吗？', '跟进：跟进 F1'] })
    expect(recap('T-1', { delegation: view, towerApi: {}, questions: [], answers: {}, proposals: {}, followUps: [] }).conclusion).toBe('已确认完成：按新接口做')
    expect(recap('T-none', { delegation: null, towerApi: null, questions: [], answers: {}, proposals: null, followUps: [] })).toEqual(empty())
  })

  it('keeps items the daemon put aside off the first screen and counts them', () => {
    // The daemon's list for this state (`attention_list::tests::quiet_items_wait_under_later`).
    const attention_list = daemonList(['signal:ci:ci:T-1', 'follow-up:F2', 'signal:ci:ci:T-3'], 2, ['signal:ci:ci:T-3'])
    const state = runtimeProjection({ status: 'ready', attention_list, projection: {
      Task: { 'T-1': task('T-1'), 'T-3': task('T-3') }, Run: {},
      FollowUp: { F2: follow('F2', { on: 'date', at: '2026-10-01T00:00:00Z' }) },
      TowerApi: { signals: [
        { from: { id: 'ci', kind: 'source' }, target: 'T-3', count: 1, data: { kind: 'ci', level: 'active', title: '休眠任务上的提醒' }, last: mark(8) },
        { from: { id: 'ci', kind: 'source' }, target: 'T-1', count: 1, data: { kind: 'ci', level: 'active', title: '醒着的提醒' }, last: mark(9) },
      ] },
    }, quiet: { dormant_after: 14, due: ['F2'], objects: [
      { id: 'proj', last: mark(9), quiet: false, resurfaced: null }, { id: 'T-3', last: mark(1), quiet: true, resurfaced: null }] } })
    expect(state.allAttention.map((item) => item.title)).toEqual(['醒着的提醒', '跟进 F2', '休眠任务上的提醒'])
    expect([state.shown, state.asleep, state.attention.length, state.later.length]).toEqual([2, 1, 2, 1])
    // The App's own fields stay on the daemon's items.
    expect(state.allAttention[0].signal).toEqual({ from: 'ci', kind: 'ci', target: 'T-1', seen: 'e9' })
    expect(state.allAttention[1].followUp).toBe('F2')
  })
})
