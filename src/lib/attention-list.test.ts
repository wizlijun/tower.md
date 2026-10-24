// Cross-check against the daemon (`crates/towermd-devkit/tests/attention_list.rs`):
// both read the same fixture. The daemon decides the order; the App's own
// assembly must find exactly the same items with the same placement, and
// the App's digest must say the same three lines.
import { describe, expect, it } from 'vitest'
import fixture from '../../crates/towermd-devkit/tests/fixtures/attention/state.json'
import expected from '../../crates/towermd-devkit/tests/fixtures/attention/expected.json'
import type { ProjectState } from './api'
import { changeSummaries, timing, visitDigest, whyFor } from './attention'
import { runtimeProjection } from './runtime'

const now = Date.parse(fixture.now)
const state = fixture.state as unknown as ProjectState
const shape = (id: string, kind: string, tasks: string[], modules: string[]) => ({ id, kind, tasks, modules })

describe('the App and the daemon agree on the attention list', () => {
  it('assembles the same items with the same kind and placement', () => {
    const own = runtimeProjection(state, now).allAttention.map((item) => shape(item.id, item.kind, item.tasks, item.modules))
    const daemon = expected.items.map((item) => shape(item.id, item.kind, item.objects, item.areas))
    const byId = (a: { id: string }, b: { id: string }) => a.id.localeCompare(b.id)
    expect([...own].sort(byId)).toEqual([...daemon].sort(byId))
  })

  it('follows the daemon’s order, first screen, quiet split and timings', () => {
    const items = expected.items.map((item) => ({ ...item, label: item.kind, title: item.id, at: 0, object: null, detail: null, action: '',
      urgent: item.timing === 'now', delegation: null, signal: null, card: '' }))
    const runtime = runtimeProjection({ ...state, attention_list: { items, shown: expected.shown, asleep: expected.asleep, budget: expected.budget } } as ProjectState, now)
    expect(runtime.allAttention.map((item) => [item.id, timing(item), item.quiet])).toEqual(expected.items.map((item) => [item.id, item.timing, item.quiet]))
    expect([runtime.shown, runtime.asleep, runtime.later.length]).toEqual([expected.shown, expected.asleep, expected.items.length - expected.shown])
    expect(runtime.allAttention.find((item) => item.id === 'Q-1')?.question?.draft.id).toBe('Q-1')
  })

  it('brings a due decision back with the daemon’s card text (FR-186)', () => {
    const review = runtimeProjection(state, now).allAttention.find((item) => item.id === 'decision-review:D-1')
    expect([review?.kind, review?.title, review?.object, review?.decisionReview, review?.detail]).toEqual(['复查决策', '导出用什么格式', '决策 D-1', 'D-1',
      '2026-10-20 到期复查；当时的选项：JSON、CSV；当时定为“JSON”，理由：下游只读 JSON'])
  })

  it('opens an extension’s change summary behind “为什么”, as sent (FR-138, FR-139)', () => {
    const summaries = changeSummaries((fixture.state.projection as Record<string, unknown>).TowerApi)
    const items = runtimeProjection(state, now).allAttention
    const proposal = items.find((item) => item.proposal === 'P-1')!
    // The proposal's own summary, then those of the tasks it touches.
    expect(whyFor(proposal, summaries).map((summary) => [summary.target, summary.lines.length])).toEqual([['P-1', 1], ['T-1', 3]])
    const gate = items.find((item) => item.id === 'A-gate')!
    expect(whyFor(gate, summaries).map((summary) => [summary.from, summary.lines])).toEqual([['pkg:eng-diff', ['新增函数：3', '新增依赖：1', '结尾：改了']]])
    expect(whyFor(items.find((item) => item.id === 'A-disk')!, summaries)).toEqual([])
  })

  it('says the same digest lines', () => {
    const runtime = runtimeProjection(state, now)
    const digest = visitDigest({ since: Date.parse(fixture.since), fallback: false, now, tasks: runtime.tasks, delegation: runtime.delegation,
      signals: (fixture.state.projection as Record<string, unknown>).TowerApi, attention: runtime.allAttention, budget: expected.shown })
    expect({ lines: digest.lines, details: digest.details }).toEqual(expected.digest)
  })
})
