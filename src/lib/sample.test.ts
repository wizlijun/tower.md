import { describe, expect, it } from 'vitest'
import { createSample, sampleView, transition, type SampleAction, type SampleState } from './sample'

function run(...actions: SampleAction[]): SampleState {
  return actions.reduce(transition, createSample())
}
const approved = { type: 'approve_baseline' } as const
const started = { type: 'start_task' } as const
const answered = { type: 'answer', text: 'JSON 格式' } as const
const passed = { type: 'verify', result: 'passed' } as const

describe('isolated sample workflow', () => {
  it('requires baseline approval before work and keeps all eight cards available', () => {
    const initial = createSample()
    expect(sampleView(initial).nodes).toHaveLength(8)
    expect(initial.task).toBe('blocked')
    expect(initial.question).toBe('blocked')
    expect(initial.verification).toBe('blocked')
    expect(initial.review).toBe('blocked')
    expect(() => transition(initial, started)).toThrow('先审阅')
    expect(() => transition(initial, answered)).toThrow('没有待答问题')
    expect(() => transition(initial, passed)).toThrow('先开始任务')
    expect(() => transition(initial, { type: 'approve_review' })).toThrow('本轮模拟验证通过')
    const ready = transition(initial, approved)
    expect(ready.task).toBe('ready')
    expect(() => transition(ready, approved)).toThrow('已批准')
    expect(transition(ready, started).question).toBe('open')
  })

  it('blocks verification until an answer and records each answer only once', () => {
    const waiting = run(approved, started)
    expect(waiting.task).toBe('waiting_answer')
    expect(() => transition(waiting, passed)).toThrow('回答问题')
    expect(() => transition(waiting, { type: 'answer', text: '  ' })).toThrow('填写回答')
    const ready = transition(waiting, answered)
    expect(ready.answer).toBe('JSON 格式')
    expect(ready.task).toBe('running')
    expect(ready.verification).toBe('ready')
    const repeated = transition(ready, { type: 'answer', text: ' JSON 格式 ' })
    expect(repeated).toEqual(ready)
    expect(repeated.history.filter(entry => entry.action.type === 'answer')).toHaveLength(1)
    expect(() => transition(ready, { type: 'answer', text: '另一回答' })).toThrow('已有回答会保留')
  })

  it('does not let failed or absent tests authorize completion', () => {
    for (const result of ['failed', 'no_tests'] as const) {
      const state = run(approved, started, answered, { type: 'verify', result })
      expect(state.task).toBe('running')
      expect(state.review).toBe('blocked')
      expect(() => transition(state, { type: 'approve_review' })).toThrow('本轮模拟验证通过')
      const retried = transition(state, passed)
      const done = transition(retried, { type: 'approve_review' })
      expect(done.task).toBe('done')
      expect(done.review).toBe('approved')
      expect(done.cost_usd).toBeNull()
      expect(done.history.filter(entry => entry.action.type === 'verify')).toHaveLength(2)
    }
  })

  it('requires a fresh verification after rejection while retaining the answer and history', () => {
    const firstPass = run(approved, started, answered, passed)
    const rejected = transition(firstPass, { type: 'reject_review' })
    expect(rejected.task).toBe('running')
    expect(rejected.verification).toBe('ready')
    expect(rejected.review).toBe('rejected')
    expect(rejected.question).toBe('answered')
    expect(rejected.answer).toBe('JSON 格式')
    expect(rejected.history.slice(0, firstPass.history.length)).toEqual(firstPass.history)
    expect(() => transition(rejected, { type: 'approve_review' })).toThrow('本轮模拟验证通过')
    expect(() => transition(rejected, { type: 'reject_review' })).toThrow('没有待放行')
    const done = transition(transition(rejected, passed), { type: 'approve_review' })
    expect(done.task).toBe('done')
    expect(done.history.filter(entry => entry.action.type === 'answer')).toHaveLength(1)
    expect(done.history.filter(entry => entry.action.type === 'verify')).toHaveLength(2)
    expect(() => transition(done, passed)).toThrow('先开始任务')
  })

  it('never moves nodes or mutates existing state during workflow transitions', () => {
    let state = createSample()
    expect(transition(new Proxy(state, {}), approved).task).toBe('ready')
    const coordinates = sampleView(state).nodes.map(({ id, x, y }) => ({ id, x, y }))
    for (const action of [approved, started, answered, passed, { type: 'reject_review' }, passed, { type: 'approve_review' }] as SampleAction[]) {
      const before = structuredClone(state)
      const next = transition(state, action)
      expect(state).toEqual(before)
      expect(sampleView(next).nodes.map(({ id, x, y }) => ({ id, x, y }))).toEqual(coordinates)
      expect(next.cost_usd).toBeNull()
      state = next
    }
    expect(state.history.map(entry => entry.seq)).toEqual([1, 2, 3, 4, 5, 6, 7])
    expect(sampleView(state).reference).toBe('sample')
    expect(sampleView(state).diagnostics.join('')).toContain('不生成证据')
    const action: SampleAction = { type: 'answer', text: '原回答' }
    const recorded = transition(run(approved, started), action)
    action.text = '后续改动'
    expect(recorded.answer).toBe('原回答')
    expect(recorded.history.at(-1)?.action).toEqual({ type: 'answer', text: '原回答' })
  })
})
