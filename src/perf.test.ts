// U-6 performance acceptance, front-end share (NFR-03).
//
// - Z0 first render: 50 frames binding 500 tasks drawn from scratch by the real
//   ProjectCanvas (Svelte Flow included), budget 1 s.
// - Live state → canvas: a new live state carrying one changed bound card is turned
//   into frames and re-rendered; the result plus the daemon share measured in
//   crates/towermd/tests/perf_cli.rs and the FR-50/FR-151 1 s merge window makes
//   up the 2 s NFR-03 budget.
//
// The default run uses the spec scale and stays well under a second. Set
// TOWERMD_PERF_STATE to a file written by the Rust scale test
// (TOWERMD_PERF_STATE_OUT) to replay a real daemon state instead.
import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { cleanup, render } from '@testing-library/svelte'
import { flushSync } from 'svelte'
import ProjectCanvas from './components/ProjectCanvas.svelte'
import type { FrameEntry, FrameLayout, ProjectView } from './lib/api'
import { frameChange, framesFrom } from './lib/frames'

const Z0_FIRST_RENDER_MS = 1000
const FRAMES = 50
const CARDS_PER_FRAME = 10
const kanban: FrameLayout = { x: { category: { field: 'state', values: ['待办', '进行中', '待评审', '完成'] } }, y: 'order', structure: [] }

function frame(index: number, state = '进行中'): FrameEntry {
  const id = `C-perf-${index}`
  return {
    id, title: `容器 ${index}`, template: { id: 'kanban', version: '1.0.0', definition: {} }, layout: kanban,
    states: ['待办', '进行中', '待评审', '完成'], done_states: ['完成'], limits: { title_max: 30, body_max: 100 },
    set_by: 'human', mode: 'bound',
    rules: { text: '', add_card: 'anyone', add_link: 'anyone', edit_text: 'anyone', change_state: 'anyone', fields: [], transitions: {}, require_source: false, done_needs_human: true },
    place: { x: (index % 10) * 1000, y: Math.floor(index / 10) * 500, width: 880, height: 420 },
    created: { at: '2026-10-24T00:00:00Z', eid: `e-${id}`, seq: index + 1, actor: 'human:local' },
    objects: Array.from({ length: CARDS_PER_FRAME }, (_, card) => `T-perf-${index * CARDS_PER_FRAME + card}`),
    bound: Array.from({ length: CARDS_PER_FRAME }, (_, card) => ({
      id: `T-perf-${index * CARDS_PER_FRAME + card}`, title: `任务 ${index * CARDS_PER_FRAME + card}`,
      state: card === 0 ? state : '待办', phase: 'open', done: false,
    })),
  }
}

/** A Z0 view with one module card per frame, as the global map shows them. */
function view(count: number): ProjectView {
  return {
    title: 'perf', reference: 'main', revision: 'r1', docs_dir: 'docs', diagnostics: [], edges: [], unplaced: [],
    nodes: Array.from({ length: count }, (_, index) => ({
      id: `M-${index}`, kind: 'text' as const, title: `模块 ${index}`, content: '', x: (index % 10) * 1000, y: -400 - Math.floor(index / 10) * 200, width: 240, height: 120,
    })),
  }
}

/** Frames and view from a dumped daemon state, or the synthetic spec-size set. */
function input(): { frames: FrameEntry[]; view: ProjectView; next: (frames: FrameEntry[]) => FrameEntry[] } {
  const path = process.env.TOWERMD_PERF_STATE
  if (path) {
    const dump = JSON.parse(readFileSync(path, 'utf8')) as { view: ProjectView; state: { projection: unknown; frames?: FrameEntry[] } }
    const frames = framesFrom(dump.state.projection, dump.state.frames)
    return { frames, view: dump.view, next: (shown) => shown.map((entry, index) => index ? entry
      : { ...entry, bound: (entry.bound ?? []).map((card, at) => at ? card : { ...card, state: card.state === '完成' ? '待办' : '完成' }) }) }
  }
  return {
    frames: Array.from({ length: FRAMES }, (_, index) => frame(index)),
    view: view(FRAMES),
    next: (shown) => shown.map((entry, index) => index ? entry : frame(0, '待评审')),
  }
}

function report(name: string, samples: number[], limit: number) {
  const sorted = [...samples].sort((a, b) => a - b)
  const at = (q: number) => sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))]
  console.log(`PERF ${name} n=${samples.length} p50=${at(0.5).toFixed(0)}ms p95=${at(0.95).toFixed(0)}ms max=${sorted.at(-1)!.toFixed(0)}ms limit=${limit}ms ${sorted.at(-1)! <= limit ? 'PASS' : 'FAIL'}`)
}

describe('performance acceptance (NFR-03)', () => {
  it('draws Z0 with 50 frames and 500 bound tasks within 1 s and applies a live change quickly', async () => {
    const { frames, view: shownView, next } = input()
    const cards = frames.reduce((sum, entry) => sum + (entry.bound?.length ?? 0) + (entry.cards?.length ?? 0), 0)
    const first: number[] = []
    const live: number[] = []
    // Warm once with one small frame: in the App the canvas code has already run by the
    // time a project opens; a cold Node/happy-dom first pass is reported, not asserted.
    const coldStart = performance.now()
    render(ProjectCanvas, { view: view(1), frames: [frame(0)], onOpen: () => {} })
    flushSync()
    const cold = performance.now() - coldStart
    cleanup()
    for (let sample = 0; sample < 3; sample += 1) {
      const started = performance.now()
      const { container, rerender } = render(ProjectCanvas, { view: shownView, frames, onOpen: () => {} })
      flushSync()
      first.push(performance.now() - started)
      expect(container.querySelectorAll('.frame-node')).toHaveLength(frames.length)
      expect(container.querySelectorAll('.frame-card').length).toBeGreaterThan(0)

      // A live state arrives: the same derivation App runs (frameChange), then the canvas redraws.
      const changed = next(frames)
      const began = performance.now()
      expect(frameChange(frames, changed)).toBe('later')
      await rerender({ frames: changed })
      flushSync()
      live.push(performance.now() - began)
      cleanup()
    }
    console.log(`PERF z0 frames=${frames.length} cards=${cards}`)
    console.log(`PERF part.canvas_cold_warmup one_frame=${cold.toFixed(0)}ms (not asserted)`)
    report('nfr03.z0_first_render', first, Z0_FIRST_RENDER_MS)
    report('part.live_state_to_canvas', live, Z0_FIRST_RENDER_MS)
    expect(Math.max(...first)).toBeLessThanOrEqual(Z0_FIRST_RENDER_MS)
  })
})
