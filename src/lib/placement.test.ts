import { describe, expect, it } from 'vitest'
import type { FrameCard, FrameEntry, FrameLayout } from './api'
import { shownCards } from './frames'
import { CARD_H, crossLines, frameOfCard, placeCards, type Placement } from './placement'

const kanban: FrameLayout = { x: { category: { field: 'state', values: ['待办', '进行中', '完成'] } }, y: 'order', structure: [] }
const timeline: FrameLayout = { x: { time: { field: 'date' } }, y: 'none', structure: [] }
const mindmap: FrameLayout = { x: 'none', y: 'none', structure: ['tree'] }
const graph: FrameLayout = { x: 'none', y: 'none', structure: ['relations'] }

const mark = (seq: number) => ({ at: '2026-10-24T00:00:00Z', eid: `e-${seq}`, seq, actor: 'agent/1' })
function frame(layout: FrameLayout, cards: FrameCard[], extra: Partial<FrameEntry> = {}): FrameEntry {
  return {
    id: 'C-a', title: '板', template: { id: 't', version: '1', definition: {} }, layout,
    states: ['待办', '进行中', '完成'], done_states: ['完成'], limits: { title_max: 30, body_max: 100 }, set_by: 'delegate', mode: 'free',
    rules: { text: '', add_card: 'anyone', add_link: 'anyone', edit_text: 'anyone', change_state: 'anyone', fields: [], transitions: {}, require_source: false, done_needs_human: false },
    place: { x: 100, y: 50, width: 880, height: 600 }, created: mark(1), cards, ...extra,
  }
}
const card = (n: number, extra: Partial<FrameCard> = {}): FrameCard =>
  ({ id: `C-a.${n}`, title: `卡片 ${n}`, state: '待办', added: mark(10 + n), changed: mark(10 + n), ...extra })

function where(placement: Placement): Record<string, [number, number]> {
  return Object.fromEntries(placement.cards.map((placed) => [placed.card.id, [placed.x, placed.y]]))
}
function place(entry: FrameEntry, pins = {}) {
  return placeCards(entry, shownCards(entry), pins)
}
/** Adds the cards one by one and checks that no card already shown ever moves. */
function grows(layout: FrameLayout, cards: FrameCard[], extra: Partial<FrameEntry> = {}) {
  let before: Record<string, [number, number]> = {}
  for (let count = 1; count <= cards.length; count += 1) {
    const now = where(place(frame(layout, cards.slice(0, count), extra)))
    for (const [id, at] of Object.entries(before)) if (now[id]) expect(now[id], id).toEqual(at)
    expect(Object.keys(now).filter((id) => id in before)).toEqual(Object.keys(before).filter((id) => id in now))
    before = now
  }
  return before
}

describe('placeCards (FR-149)', () => {
  it('is deterministic: the same cards and pins give the same placement', () => {
    const entry = frame(kanban, [card(1), card(2, { state: '完成' }), card(3)])
    expect(place(entry)).toEqual(place(structuredClone(entry)))
  })

  it('appends a new card at the end of its lane and never moves the cards already there', () => {
    const at = grows(kanban, [card(1), card(2, { state: '完成' }), card(3), card(4, { state: '进行中' }), card(5)])
    expect(at['C-a.3'][0]).toBe(at['C-a.1'][0])
    expect(at['C-a.3'][1]).toBeGreaterThan(at['C-a.1'][1])
    expect(at['C-a.5'][1]).toBeGreaterThan(at['C-a.3'][1])
    expect(at['C-a.2'][0]).toBeGreaterThan(at['C-a.4'][0])
  })

  it('folds later arrivals past the lane limit into +N', () => {
    const cards = [card(1), card(2), card(3), card(4)]
    const placed = place(frame(kanban, cards, { rules: { ...frame(kanban, []).rules, per_lane_max: 2 } }))
    expect(placed.cards.map((entry) => entry.card.id)).toEqual(['C-a.1', 'C-a.2'])
    expect(placed.folds.map((fold) => fold.count)).toEqual([2])
  })

  it('folds what runs past the frame instead of moving anything', () => {
    const cards = Array.from({ length: 30 }, (_, index) => card(index + 1))
    const placed = place(frame(kanban, cards, { place: { x: 0, y: 0, width: 600, height: 300 } }))
    expect(placed.cards.length).toBeLessThan(30)
    expect(placed.folds.reduce((sum, fold) => sum + fold.count, 0) + placed.cards.length).toBe(30)
    for (const entry of placed.cards) expect(entry.y + entry.height).toBeLessThanOrEqual(300)
  })

  it('inserts by time on a time axis without changing any order; later dates only append', () => {
    const dated = (n: number, date: string) => card(n, { state: null, fields: { date } })
    // Later or equal dates append: nothing shown moves.
    grows(timeline, [dated(1, '2026-01-01'), dated(2, '2026-02-01'), dated(3, '2026-02-01'), dated(4, '2026-03-01')])
    // An earlier date goes into its place: cards after it shift along the axis, in the same order.
    const before = where(place(frame(timeline, [dated(1, '2026-01-01'), dated(2, '2026-03-01')])))
    const after = where(place(frame(timeline, [dated(1, '2026-01-01'), dated(2, '2026-03-01'), dated(3, '2026-02-01')])))
    expect(after['C-a.1']).toEqual(before['C-a.1'])
    expect(after['C-a.1'][0]).toBeLessThan(after['C-a.3'][0])
    expect(after['C-a.3'][0]).toBeLessThan(after['C-a.2'][0])
    const marks = place(frame(timeline, [dated(1, '2026-01-01'), dated(2, '2026-03-01'), dated(3, '2026-02-01')])).marks
    expect(marks.map((entry) => entry.label)).toEqual(['2026-01-01', '2026-02-01', '2026-03-01'])
  })

  it('opens a new branch row for each new card in a tree, so no card moves when a child arrives late', () => {
    const at = grows(mindmap, [card(1, { state: null }), card(2, { state: null, parent: 'C-a.1' }), card(3, { state: null }),
      card(4, { state: null, parent: 'C-a.3' }), card(5, { state: null, parent: 'C-a.1' }), card(6, { state: null, parent: 'C-a.5' })])
    expect(at['C-a.2'][0]).toBeGreaterThan(at['C-a.1'][0])
    expect(at['C-a.2'][1]).toBe(at['C-a.1'][1])
    expect(at['C-a.6'][0]).toBeGreaterThan(at['C-a.5'][0])
    for (const [child, parent] of [['C-a.2', 'C-a.1'], ['C-a.4', 'C-a.3'], ['C-a.5', 'C-a.1'], ['C-a.6', 'C-a.5']]) {
      expect(at[child][1]).toBeGreaterThanOrEqual(at[parent][1])
    }
    const lines = place(frame(mindmap, [card(1, { state: null }), card(2, { state: null, parent: 'C-a.1' })])).lines
    expect(lines).toEqual([{ from: 'C-a.1', to: 'C-a.2', kind: 'parent' }])
  })

  it('lays relation cards out incrementally and draws lines; links to other frames wait for hover', () => {
    const at = grows(graph, [card(1, { state: null }), card(2, { state: null, links: ['C-a.1'] }), card(3, { state: null, links: ['C-b.4'] })])
    expect(Object.keys(at)).toHaveLength(3)
    const placed = place(frame(graph, [card(1, { state: null }), card(2, { state: null, links: ['C-a.1'] }), card(3, { state: null, links: ['C-b.4'] })]))
    expect(placed.lines).toEqual([{ from: 'C-a.2', to: 'C-a.1', kind: 'link' }])
    expect(placed.outside).toEqual([{ from: 'C-a.3', to: 'C-b.4' }])
  })

  it('keeps a pinned card where the person put it after any number of fast-layer writes', () => {
    const pins = { 'C-a.2': { x: 500, y: 300 } }
    const cards = [card(1), card(2), card(3)]
    for (let more = 0; more < 5; more += 1) {
      const placed = place(frame(kanban, [...cards, ...Array.from({ length: more }, (_, index) => card(4 + index, { state: index % 2 ? '完成' : '待办' }))]), pins)
      const pinned = placed.cards.find((entry) => entry.card.id === 'C-a.2')!
      expect([pinned.x, pinned.y, pinned.pinned]).toEqual([500, 300, true])
    }
    // A pinned tree card still parents later cards, which start below the pinned block.
    const tree = place(frame(mindmap, [card(1, { state: null }), card(2, { state: null, parent: 'C-a.1' })]), { 'C-a.1': { x: 12, y: 56 } })
    const child = tree.cards.find((entry) => entry.card.id === 'C-a.2')!
    expect(child.x).toBeGreaterThan(12)
    expect(child.y).toBeGreaterThanOrEqual(56 + CARD_H)
  })

  it('never pins a proposal, which is not a card yet', () => {
    const entry = frame(kanban, [], { pending: [{ id: 40, why: 'proposal', target: 'C-a', op: { op: 'add', title: '提的' }, from: 'a', by: mark(40) }] })
    const placed = place(entry, { 'pending:40': { x: 600, y: 400 } })
    expect(placed.cards[0].pinned).toBe(false)
  })

  it('reads the frame from a card ID and nothing from an object ID', () => {
    expect(frameOfCard('C-a.12')).toBe('C-a')
    expect(frameOfCard('T-1')).toBeNull()
  })
})

describe('crossLines (FR-141)', () => {
  it('draws a line to another frame only while one of its ends is hovered or selected', () => {
    const a = frame(graph, [card(1, { state: null, links: ['C-b.1'] })])
    const b = { ...frame(graph, []), id: 'C-b', place: { x: 2000, y: 0, width: 880, height: 600 },
      cards: [{ ...card(1, { state: null }), id: 'C-b.1' }] }
    const placements = new Map([[a.id, place(a)], [b.id, place(b)]])
    expect(crossLines([a, b], placements, [])).toEqual([])
    expect(crossLines([a, b], placements, ['C-a.2'])).toEqual([])
    for (const end of ['C-a.1', 'C-b.1']) {
      const [line] = crossLines([a, b], placements, [end])
      expect(line.id).toBe('C-a.1->C-b.1')
      expect(line.from.x).toBeLessThan(line.to.x)
      expect(line.to.x).toBeGreaterThan(2000)
    }
  })
})
