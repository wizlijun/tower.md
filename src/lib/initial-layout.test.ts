import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { CanvasEdge, CanvasNode, FrameEntry, FrameTemplate } from './api'
import { frameChange, shownCards, toFrameNodes } from './frames'
import { CONVERT_MAX, conversion, initialLayout, needsInitialLayout } from './initial-layout'
import { crossLines, placeCards } from './placement'

const layouts = vi.hoisted(() => ({ count: 0 }))
vi.mock('elkjs/lib/elk.bundled.js', async (original) => {
  const real = (await original<{ default: new () => { layout: (graph: unknown) => Promise<unknown> } }>()).default
  return { default: class { layout(graph: unknown) { layouts.count += 1; return new real().layout(graph) } } }
})

const template = (id: string, extra: Partial<FrameTemplate>): FrameTemplate => ({ id, origin: 'starter', path: id, problems: [],
  limits: { title_max: 10, body_max: 100 }, layout: { x: 'none', y: 'none', structure: [] }, ...extra })
const kanban = template('kanban', { layout: { x: { category: { field: 'state', values: [] } }, y: 'order', structure: [] }, states: ['待办', '完成'], done_states: ['完成'] })
const mindmap = template('mindmap', { layout: { x: 'none', y: 'none', structure: ['tree'] } })
const graph = template('graph', { layout: { x: 'none', y: 'none', structure: ['relations'] } })
const node = (id: string, extra: Partial<CanvasNode> = {}): CanvasNode =>
  ({ id, kind: 'text', title: `节点 ${id}`, content: '', x: 0, y: 0, width: 200, height: 80, ...extra })
const edge = (from: string, to: string): CanvasEdge => ({ id: `${from}-${to}`, from_node: from, to_node: to })

describe('conversion (FR-144 方式 2)', () => {
  it('binds core objects to a template with states and copies the other nodes as free cards', () => {
    const nodes = [node('a', { object_id: 'T-1' }), node('b', { object_id: 'T-2' }), node('c'), node('g', { kind: 'group' })]
    expect(conversion(kanban, nodes.slice(0, 2), [])).toEqual({ mode: 'bound', objects: ['T-1', 'T-2'], copies: [], links: [] })
    const mixed = conversion(kanban, nodes, [])
    expect([mixed.mode, mixed.objects, mixed.copies.map((copy) => copy.node)]).toEqual(['mixed', ['T-1', 'T-2'], ['c']])
    const free = conversion(mindmap, nodes, [])
    expect([free.mode, free.objects, free.copies.map((copy) => copy.node)]).toEqual(['free', [], ['a', 'b', 'c']])
  })

  it('turns lines into parents in a tree, parents first, and a loop into a branch of its own', () => {
    const plan = conversion(mindmap, [node('c'), node('b'), node('a'), node('x')], [edge('a', 'b'), edge('b', 'c'), edge('x', 'x'), edge('c', 'a')])
    expect(plan.copies.map((copy) => [copy.node, copy.parent])).toEqual([['a', null], ['b', 'a'], ['c', 'b'], ['x', null]])
    expect(plan.links).toEqual([])
    expect(conversion(graph, [node('a'), node('b')], [edge('a', 'b'), edge('a', 'b'), edge('a', 'z')]).links).toEqual([{ from: 'a', to: 'b' }])
  })

  it('clips titles to the template limit and takes at most the cap', () => {
    const plan = conversion(mindmap, [node('a', { title: '  一二三四五\n六七八九十十一  ' })], [])
    expect(plan.copies[0].title).toBe('一二三四五 六七八九')
    expect(conversion(mindmap, Array.from({ length: CONVERT_MAX + 5 }, (_, index) => node(`n${index}`)), []).copies).toHaveLength(CONVERT_MAX)
  })

  it('lays out structure only; axes place cards from data', () => {
    expect([needsInitialLayout(mindmap.layout!), needsInitialLayout(graph.layout!), needsInitialLayout(kanban.layout!)]).toEqual([true, true, false])
  })
})

describe('elkjs runs only for a new frame’s first layout', () => {
  beforeEach(() => { layouts.count = 0 })

  it('is never called on the fast layer: placement, frame nodes, merging and cross-frame lines', () => {
    const mark = (seq: number) => ({ at: '2026-10-24T00:00:00Z', eid: `e-${seq}`, seq, actor: 'agent/1' })
    const frame: FrameEntry = {
      id: 'C-a', title: '图', template: { id: 'graph', version: '1', definition: {} }, layout: graph.layout!, states: [], done_states: [],
      limits: { title_max: 30, body_max: 100 }, set_by: 'delegate', mode: 'free',
      rules: { text: '', add_card: 'anyone', add_link: 'anyone', edit_text: 'anyone', change_state: 'anyone', fields: [], transitions: {}, require_source: false, done_needs_human: false },
      place: { x: 0, y: 0, width: 880, height: 600 }, created: mark(1),
      cards: Array.from({ length: 20 }, (_, index) => ({ id: `C-a.${index + 1}`, title: `${index}`, added: mark(index + 2), changed: mark(index + 2), links: index ? [`C-a.${index}`] : [] })),
    }
    const placed = placeCards(frame, shownCards(frame))
    toFrameNodes([frame], {}, true, undefined, new Map())
    frameChange([frame], [{ ...frame, cards: [...frame.cards!, { id: 'C-a.21', title: 'new', added: mark(30), changed: mark(30) }] }])
    crossLines([frame], new Map([[frame.id, placed]]), ['C-a.1'])
    expect(layouts.count).toBe(0)
  })

  it('is called when a person turns selected nodes into a frame, and gives pins inside it', async () => {
    const { pins, width, height } = await initialLayout(['a', 'b', 'c'], [{ from: 'a', to: 'b' }, { from: 'a', to: 'c' }, { from: 'a', to: 'z' }])
    expect(layouts.count).toBe(1)
    expect(Object.keys(pins).sort()).toEqual(['a', 'b', 'c'])
    expect(pins.b.x).toBeGreaterThan(pins.a.x)
    for (const pin of Object.values(pins)) expect(pin.x + 180 <= width && pin.y + 44 <= height).toBe(true)
  })

  it('is imported by the 方式 2 module alone', () => {
    const root = join(__dirname, '..')
    const files = (readdirSync(root, { recursive: true }) as string[]).filter((file) => /\.(ts|svelte)$/.test(file) && !file.endsWith('.test.ts'))
    const elk = files.filter((file) => readFileSync(join(root, file), 'utf8').match(/(from|import\()\s*['"]elkjs/))
    expect(elk).toEqual([join('lib', 'initial-layout.ts')])
  })
})
