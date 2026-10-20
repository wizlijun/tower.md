import { describe, expect, it } from 'vitest'
import type { CanvasNode } from './api'
import { toFlowEdges, toFlowNodes } from './graph'

const nodes: CanvasNode[] = ['A', 'B', 'Alone'].map((id, index) => ({
  id, kind: 'text', title: id, content: id, x: index * 300 - 20, y: 41,
  width: 260, height: 180,
}))

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
})
