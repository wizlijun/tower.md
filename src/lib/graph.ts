import { MarkerType, type Edge, type Node } from '@xyflow/svelte'
import type { CanvasEdge, CanvasNode } from './api'
import type { CardStatus } from './verification'
import type { ActivityBadge, RuntimeAttention } from './runtime'

export type CardNode = Node<{
  entry: CanvasNode
  status?: CardStatus
  activity?: ActivityBadge
  attention?: Pick<RuntimeAttention, 'id' | 'kind' | 'title'>[]
  onOpen?: (node: CanvasNode) => void
}, 'card'>

const canvasColors: Record<string, string> = {
  '1': '#c86a63', '2': '#cd9964', '3': '#b3a64e',
  '4': '#69966d', '5': '#61a5a7', '6': '#9a80b5',
}

function side(value: string | undefined, fallback: 'left' | 'right') {
  return value && ['top', 'right', 'bottom', 'left'].includes(value) ? value : fallback
}

function sameEntry(a: CanvasNode, b: CanvasNode): boolean {
  return a.id === b.id && a.kind === b.kind && a.title === b.title && a.content === b.content
    && a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height
    && a.file === b.file && a.url === b.url && a.object_id === b.object_id && a.object_type === b.object_type
    && a.region === b.region
}

function sameActivity(a: ActivityBadge | undefined, b: ActivityBadge | undefined): boolean {
  return a?.label === b?.label && a?.symbol === b?.symbol && a?.tone === b?.tone
}

export function toFlowNodes(
  entries: CanvasNode[],
  onOpen?: (node: CanvasNode) => void,
  statuses: Record<string, CardStatus> = {},
  editable = false,
  activity: Record<string, ActivityBadge> = {},
  cache?: Map<string, CardNode>,
  attention: RuntimeAttention[] = [],
): CardNode[] {
  const live = new Set(entries.map((entry) => entry.id))
  const nodes = entries.map((entry): CardNode => {
    const previous = cache?.get(entry.id)
    const status = statuses[entry.object_id || entry.id]
    const badge = activity[entry.object_id || entry.id]
    const concerns = entry.kind === 'group' && entry.region === 'project'
      ? attention.slice(0, 3).map(({ id, kind, title }) => ({ id, kind, title })) : undefined
    const sameConcerns = previous?.data.attention?.length === concerns?.length
      && (concerns ?? []).every((item, index) => {
        const old = previous?.data.attention?.[index]
        return old?.id === item.id && old.kind === item.kind && old.title === item.title
      })
    if (previous && sameEntry(previous.data.entry, entry) && previous.data.onOpen === onOpen
      && previous.data.status === status && previous.draggable === editable
      && sameActivity(previous.data.activity, badge) && sameConcerns) return previous
    const node: CardNode = {
      id: entry.id,
      type: 'card',
      position: previous && previous.data.entry.x === entry.x && previous.data.entry.y === entry.y
        ? previous.position : { x: entry.x, y: entry.y },
      width: entry.width,
      height: entry.height,
      style: `width: ${entry.width}px; height: ${entry.height}px;`,
      // Keep a value snapshot: mutating an incoming frame must not mutate the
      // cached comparison values or silently replace an old callback's input.
      data: { entry: { ...entry }, onOpen, status, activity: badge ? { ...badge } : undefined, attention: concerns },
      draggable: editable,
      dragHandle: editable ? '.canvas-card-drag' : undefined,
      connectable: false,
      deletable: false,
      selectable: false,
      zIndex: entry.kind === 'group' ? -1 : 0,
    }
    cache?.set(entry.id, node)
    return node
  })
  if (cache) for (const id of cache.keys()) if (!live.has(id)) cache.delete(id)
  return nodes
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
