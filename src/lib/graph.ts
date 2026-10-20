import { MarkerType, type Edge, type Node } from '@xyflow/svelte'
import type { CanvasEdge, CanvasNode } from './api'
import type { CardStatus } from './verification'

export type CardNode = Node<{
  entry: CanvasNode
  status?: CardStatus
  onOpen?: (node: CanvasNode) => void
}, 'card'>

const canvasColors: Record<string, string> = {
  '1': '#c86a63', '2': '#cd9964', '3': '#b3a64e',
  '4': '#69966d', '5': '#61a5a7', '6': '#9a80b5',
}

function side(value: string | undefined, fallback: 'left' | 'right') {
  return value && ['top', 'right', 'bottom', 'left'].includes(value) ? value : fallback
}

export function toFlowNodes(
  entries: CanvasNode[],
  onOpen?: (node: CanvasNode) => void,
  statuses: Record<string, CardStatus> = {},
): CardNode[] {
  return entries.map((entry) => ({
    id: entry.id,
    type: 'card',
    position: { x: entry.x, y: entry.y },
    width: entry.width,
    height: entry.height,
    style: `width: ${entry.width}px; height: ${entry.height}px;`,
    data: { entry, onOpen, status: statuses[entry.object_id || entry.id] },
    draggable: false,
    connectable: false,
    deletable: false,
    selectable: false,
    zIndex: entry.kind === 'group' ? -1 : 0,
  }))
}

export function toFlowEdges(entries: CanvasEdge[]): Edge[] {
  return entries.map((entry) => {
    const color = entry.color && (canvasColors[entry.color] || (/^#[\da-f]{6}$/i.test(entry.color) ? entry.color : undefined))
    return {
      id: entry.id,
      source: entry.from_node,
      target: entry.to_node,
      sourceHandle: `out-${side(entry.from_side, 'right')}`,
      targetHandle: `in-${side(entry.to_side, 'left')}`,
      label: entry.label,
      type: 'default',
      deletable: false,
      selectable: false,
      style: color ? `stroke: ${color};` : undefined,
      markerStart: entry.from_end === 'arrow' ? { type: MarkerType.ArrowClosed, color: color || undefined } : undefined,
      markerEnd: entry.to_end === 'none' ? undefined : { type: MarkerType.ArrowClosed, color: color || undefined },
    }
  })
}
