// Cross-check against the daemon (`crates/towermd-devkit/tests/attention_list.rs`):
// both read the same fixture. The daemon decides the order; the App's own
// assembly must find exactly the same items with the same placement, and
// the App's digest must say the same three lines.
import { describe, expect, it } from 'vitest'
import fixture from '../../crates/towermd-devkit/tests/fixtures/attention/state.json'
import expected from '../../crates/towermd-devkit/tests/fixtures/attention/expected.json'
import type { ProjectState } from './api'
import { timing, visitDigest } from './attention'
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

  it('says the same digest lines', () => {
    const runtime = runtimeProjection(state, now)
    const digest = visitDigest({ since: Date.parse(fixture.since), fallback: false, now, tasks: runtime.tasks, delegation: runtime.delegation,
      signals: (fixture.state.projection as Record<string, unknown>).TowerApi, attention: runtime.allAttention, budget: expected.shown })
    expect({ lines: digest.lines, details: digest.details }).toEqual(expected.digest)
  })
})
