import { describe, expect, it, vi } from 'vitest'
import type { CanvasNode } from './api'
import { lowConfidence, moduleConfidence, sameProgress, toFlowEdges, toFlowNodes, type CardNode } from './graph'
import type { ActivityBadge, RuntimeAttention } from './runtime'

const nodes: CanvasNode[] = ['A', 'B', 'Alone'].map((id, index) => ({
  id, kind: 'text', title: id, content: id, x: index * 300 - 20, y: 41,
  width: 260, height: 180,
}))

describe('module confidence', () => {
  it('reads only the frontmatter confidence', () => {
    expect(moduleConfidence('---\ntype: Module\nconfidence: 0.42\n---\n# A')).toBe(0.42)
    expect(moduleConfidence('---\ntype: Module\n---\nconfidence: 0.1')).toBeNull()
    expect(moduleConfidence('no frontmatter')).toBeNull()
  })

  it('marks only Module cards below 0.6', () => {
    const card = (object_type: string | undefined, confidence: number) => ({ object_type, content: `---\ntype: Module\nconfidence: ${confidence}\n---\n` })
    expect(lowConfidence(card('Module', 0.3))).toBe(0.3)
    expect(lowConfidence(card('Module', 0.6))).toBeNull()
    expect(lowConfidence(card('Interface', 0.1))).toBeNull()
    expect(lowConfidence(card(undefined, 0.1))).toBeNull()
  })
})

describe('canvas graph mapping', () => {
  it('preserves independent nodes and exact baseline coordinates without changing input', () => {
    const original = structuredClone(nodes)
    const result = toFlowNodes(nodes)
    expect(result.map((node) => node.id)).toEqual(['A', 'B', 'Alone'])
    expect(result.map((node) => node.position)).toEqual([{ x: -20, y: 41 }, { x: 280, y: 41 }, { x: 580, y: 41 }])
    expect(result.every((node) => !node.draggable && !node.connectable && !node.deletable)).toBe(true)
    expect(nodes).toEqual(original)
  })

  it('preserves cycles, multiple relations and self relations instead of making a tree', () => {
    const edges = [
      { id: 'ab', from_node: 'A', to_node: 'B', label: '提供' },
      { id: 'ba', from_node: 'B', to_node: 'A', label: '依赖' },
      { id: 'aa', from_node: 'A', to_node: 'A', label: '复用', to_end: 'none' },
    ]
    const mapped = toFlowEdges(edges)
    expect(mapped.map(({ source, target, label }) => ({ source, target, label }))).toEqual([
      { source: 'A', target: 'B', label: '提供' },
      { source: 'B', target: 'A', label: '依赖' },
      { source: 'A', target: 'A', label: '复用' },
    ])
    expect(mapped[0].markerEnd).toBeDefined()
    expect(mapped[2].markerEnd).toBeUndefined()
  })

  it('keeps groups behind cards and link content as inert text', () => {
    const result = toFlowNodes([
      { ...nodes[0], kind: 'group' },
      { ...nodes[1], kind: 'link', url: 'javascript:alert(1)' },
    ])
    expect(result[0].zIndex).toBe(-1)
    expect(result[1].data.entry.url).toBe('javascript:alert(1)')
    expect(result[1].data.entry.kind).toBe('link')
  })

  it('maps all four endpoint sides and defaults missing sides to right-to-left', () => {
    const result = toFlowEdges([
      { id: 'tb', from_node: 'A', to_node: 'B', from_side: 'top', to_side: 'bottom' },
      { id: 'lr', from_node: 'B', to_node: 'A', from_side: 'left', to_side: 'right' },
      { id: 'bt', from_node: 'A', to_node: 'A', from_side: 'bottom', to_side: 'top' },
      { id: 'default', from_node: 'A', to_node: 'B' },
    ])
    expect(result.map(({ sourceHandle, targetHandle }) => [sourceHandle, targetHandle])).toEqual([
      ['out-top', 'in-bottom'],
      ['out-left', 'in-right'],
      ['out-bottom', 'in-top'],
      ['out-right', 'in-left'],
    ])
    expect(result[3].label).toBeUndefined()
    expect(result[3].style).toBeUndefined()
    expect(result[3].markerStart).toBeUndefined()
  })

  it('reuses unchanged nodes by ID across complete frames and changes only the updated badge', () => {
    const cache = new Map<string, CardNode>()
    const onOpen = vi.fn()
    const badge: ActivityBadge = { label: '1 运行', symbol: '▶', tone: 'verifying' }
    const first = toFlowNodes(nodes, onOpen, { A: 'passed' }, false, { A: badge }, cache)
    const repeated = toFlowNodes(structuredClone(nodes), onOpen, { A: 'passed' }, false, { A: { ...badge } }, cache)
    repeated.forEach((node, index) => expect(node).toBe(first[index]))
    const changed = toFlowNodes(structuredClone(nodes), onOpen, { A: 'passed' }, false,
      { A: { ...badge, label: '2 运行' } }, cache)
    expect(changed[0]).not.toBe(first[0])
    expect(changed[0].position).toBe(first[0].position)
    expect(changed[0].data.activity?.label).toBe('2 运行')
    expect(changed[1]).toBe(first[1])
    expect(changed[2]).toBe(first[2])
    expect(changed.map((node) => node.position)).toEqual(first.map((node) => node.position))
  })

  it('fades a quiet card and labels a resurfaced one without moving any node', () => {
    const cache = new Map<string, CardNode>()
    const first = toFlowNodes(nodes, undefined, {}, false, {}, cache)
    const marked = toFlowNodes(structuredClone(nodes), undefined, {}, false, {}, cache, undefined, undefined, undefined, { [nodes[0].id]: 'quiet' })
    expect(marked[0]).not.toBe(first[0])
    expect(marked[0].data.rhythm).toBe('quiet')
    expect(marked[1]).toBe(first[1])
    expect(marked.map((node) => node.position)).toEqual(first.map((node) => node.position))
    const back = toFlowNodes(structuredClone(nodes), undefined, {}, false, {}, cache, undefined, undefined, undefined, { [nodes[0].id]: 'resurfaced' })
    expect(back[0].data.rhythm).toBe('resurfaced')
    expect(back[0].position).toBe(first[0].position)
  })

  it('dashes a proposed card and rings a relation end without moving any node (FR-45, FR-36)', () => {
    const cache = new Map<string, CardNode>()
    const entries = nodes.map((node) => ({ ...node, object_id: `module-${node.id}` }))
    const first = toFlowNodes(entries, undefined, {}, false, {}, cache)
    const marks = { 'module-A': { proposed: '提案待审阅', relation: ['B'] }, 'module-B': { relation: ['A'] } }
    const marked = toFlowNodes(structuredClone(entries), undefined, {}, false, {}, cache, undefined, undefined, undefined, {}, marks)
    expect(marked[0].data.mark).toEqual({ proposed: '提案待审阅', relation: ['B'] })
    expect(marked[0].data.mark).not.toBe(marks['module-A'])
    expect(marked[1].data.mark).toEqual({ relation: ['A'] })
    expect(marked[2]).toBe(first[2])
    marked.forEach((node, index) => expect(node.position).toBe(first[index].position))
    const same = toFlowNodes(structuredClone(entries), undefined, {}, false, {}, cache, undefined, undefined, undefined, {}, structuredClone(marks))
    same.forEach((node, index) => expect(node).toBe(marked[index]))
    const approved = toFlowNodes(structuredClone(entries), undefined, {}, false, {}, cache)
    expect(approved[0].data.mark).toBeUndefined()
    approved.forEach((node, index) => expect(node.position).toBe(first[index].position))
  })

  it('tells whether progress changed by what the canvas shows (FR-50)', () => {
    const badge: ActivityBadge = { label: '1 运行', symbol: '▶', tone: 'verifying' }
    const concern: RuntimeAttention = { id: '1', kind: '提问', title: '问题', priority: 2, at: 1, modules: [], tasks: [], action: '回答' }
    const base = { activity: { A: badge }, attention: [concern], backlog: 1, rhythm: {} }
    expect(sameProgress(base, structuredClone(base))).toBe(true)
    expect(sameProgress(base, { ...base, attention: [{ ...concern, at: 9 }] })).toBe(true)
    expect(sameProgress(base, { ...base, activity: { A: { ...badge, label: '2 运行' } } })).toBe(false)
    expect(sameProgress(base, { ...base, attention: [{ ...concern, title: '新问题' }] })).toBe(false)
    expect(sameProgress(base, { ...base, backlog: undefined })).toBe(false)
    expect(sameProgress(base, { ...base, rhythm: { A: 'quiet' } })).toBe(false)
  })

  it('compares every badge field and verification status by object ID, including their removal', () => {
    const cache = new Map<string, CardNode>()
    const entries = [{ ...nodes[0], object_id: 'module-A' }]
    const badge: ActivityBadge = { label: '待关注', symbol: '?', tone: 'pending' }
    const first = toFlowNodes(entries, undefined, { 'module-A': 'passed' }, false, { 'module-A': badge }, cache)[0]
    badge.symbol = '!'
    const symbol = toFlowNodes(entries, undefined, { 'module-A': 'passed' }, false, { 'module-A': badge }, cache)[0]
    expect(symbol).not.toBe(first)
    expect(first.data.activity?.symbol).toBe('?')
    badge.tone = 'failed'
    const tone = toFlowNodes(entries, undefined, { 'module-A': 'passed' }, false, { 'module-A': badge }, cache)[0]
    expect(tone).not.toBe(symbol)
    expect(tone.data.activity?.tone).toBe('failed')
    const status = toFlowNodes(entries, undefined, { 'module-A': 'failed' }, false, { 'module-A': badge }, cache)[0]
    expect(status).not.toBe(tone)
    expect(status.data.status).toBe('failed')
    const removed = toFlowNodes(entries, undefined, {}, false, {}, cache)[0]
    expect(removed).not.toBe(status)
    expect(removed.data.status).toBeUndefined()
    expect(removed.data.activity).toBeUndefined()
    expect(removed.position).toBe(first.position)
  })

  it('updates content, approved layout, callbacks and editability without mutating cached inputs', () => {
    const cache = new Map<string, CardNode>()
    const entries = structuredClone(nodes)
    const open = vi.fn(), nextOpen = vi.fn()
    const first = toFlowNodes(entries, open, {}, false, {}, cache)
    entries[0].title = '新标题'
    entries[0].content = '新正文'
    entries[0].file = 'docs/new.md'
    const content = toFlowNodes(entries, open, {}, false, {}, cache)
    expect(content[0]).not.toBe(first[0])
    expect(content[0].data.entry).toEqual(entries[0])
    expect(first[0].data.entry.title).toBe('A')
    expect(content[0].position).toBe(first[0].position)
    expect(content[1]).toBe(first[1])
    entries[0].x = 900
    entries[0].y = 600
    entries[0].width = 300
    entries[0].height = 220
    const moved = toFlowNodes(entries, open, {}, false, {}, cache)[0]
    expect(moved.position).toEqual({ x: 900, y: 600 })
    expect(moved.position).not.toBe(content[0].position)
    expect(moved.style).toBe('width: 300px; height: 220px;')
    const callback = toFlowNodes(entries, nextOpen, {}, false, {}, cache)[0]
    expect(callback).not.toBe(moved)
    const interactive = toFlowNodes(entries, nextOpen, {}, true, {}, cache)[0]
    expect(interactive).not.toBe(callback)
    expect(interactive.draggable).toBe(true)
    expect(interactive.dragHandle).toBe('.canvas-card-drag')
    expect(interactive.position).toBe(moved.position)
    interactive.data.onOpen?.(interactive.data.entry)
    expect(nextOpen).toHaveBeenCalledWith(entries[0])
    expect(open).not.toHaveBeenCalled()
  })

  it('keeps ID identity on reorder and drops removed nodes from the component cache', () => {
    const cache = new Map<string, CardNode>()
    const first = toFlowNodes(nodes, undefined, {}, false, {}, cache)
    const reordered = toFlowNodes([nodes[1], nodes[0]], undefined, {}, false, {}, cache)
    expect(reordered).toEqual([first[1], first[0]])
    expect(reordered[0]).toBe(first[1])
    expect(cache.has('Alone')).toBe(false)
    const restored = toFlowNodes(nodes, undefined, {}, false, {}, cache)
    expect(restored[0]).toBe(first[0])
    expect(restored[2]).not.toBe(first[2])
    cache.clear() // ProjectCanvas does this when fitVersion changes project/view.
    const newView = toFlowNodes(nodes, undefined, {}, false, {}, cache)
    expect(newView[0]).not.toBe(restored[0])
    expect(newView[0].position).toEqual({ x: -20, y: 41 })
  })
})


it('updates only the approved project region with at most three real summaries and stable geometry', () => {
  const cache = new Map<string, CardNode>()
  const project: CanvasNode = { ...nodes[0], id: 'project', kind: 'group', region: 'project', x: -380, y: 0, width: 320, height: 240 }
  const concerns: RuntimeAttention[] = [1, 2, 3, 4].map((id) => ({
    id: String(id), kind: '提问', title: `问题 ${id}`, priority: 2, at: id, modules: [], tasks: [], action: '回答',
  }))
  const entries = [project, nodes[1]]
  const first = toFlowNodes(entries, undefined, {}, false, {}, cache, concerns)
  expect(first[0].data.attention?.map((item) => item.title)).toEqual(['问题 1', '问题 2', '问题 3'])
  expect(first[0].data.later).toBe(1)
  expect(first[1].data.attention).toBeUndefined()
  const repeated = toFlowNodes(structuredClone(entries), undefined, {}, false, {}, cache, structuredClone(concerns))
  expect(repeated[0]).toBe(first[0])
  concerns[0].title = '问题已更新'
  const changed = toFlowNodes(entries, undefined, {}, false, {}, cache, concerns)
  expect(changed[0]).not.toBe(first[0])
  expect(changed[0].position).toBe(first[0].position)
  expect(changed[0].style).toBe(first[0].style)
  expect(changed[1]).toBe(first[1])
  expect(first[0].data.attention?.[0].title).toBe('问题 1')
  const resolved = toFlowNodes(entries, undefined, {}, false, {}, cache, [])
  expect(resolved[0].data.attention).toEqual([])
  expect(resolved[0].data.later).toBe(0)
  expect(resolved[0].position).toBe(first[0].position)
  expect(toFlowNodes(nodes, undefined, {}, false, {}, undefined, concerns)).toHaveLength(nodes.length)
  const budgeted = toFlowNodes(entries, undefined, {}, false, {}, cache, concerns, undefined, 1)
  expect([budgeted[0].data.attention?.length, budgeted[0].data.later]).toEqual([1, 3])
  expect(budgeted[0].position).toBe(first[0].position)
})


it('distinguishes an unapproved project region from an approved region with no concerns', () => {
  const cache = new Map<string, CardNode>()
  const project: CanvasNode = { ...nodes[0], kind: 'group', region: 'project' }
  const candidate = toFlowNodes([project], undefined, {}, false, {}, cache)[0]
  expect(candidate.data.attention).toBeUndefined()
  const approved = toFlowNodes([project], undefined, {}, false, {}, cache, [])[0]
  expect(approved.data.attention).toEqual([])
  expect(approved).not.toBe(candidate)
  expect(approved.position).toBe(candidate.position)
  const draft = toFlowNodes([project], undefined, {}, false, {}, cache, undefined)[0]
  expect(draft.data.attention).toBeUndefined()
  expect(draft).not.toBe(approved)
  expect(draft.position).toBe(approved.position)
  expect(draft.style).toBe(approved.style)
})
