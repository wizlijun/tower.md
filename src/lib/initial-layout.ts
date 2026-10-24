import type { ElkNode } from 'elkjs/lib/elk-api'
import type { CanvasEdge, CanvasNode, FrameLayout, FrameTemplate } from './api'
import { FRAME_HEAD, FRAME_MAX_SIZE, FRAME_MIN_SIZE, bindable } from './frames'
import { CARD_H, CARD_W, type Pin } from './placement'

const PAD = 12
const FOOT = 28
/** At most this many nodes turn into one frame at a time. */
export const CONVERT_MAX = 50

/**
 * Whether a frame made from selected nodes gets a first layout from elkjs:
 * only one laid out by structure (trees, nesting, relation lines). Category
 * and time axes place cards from their data (`placeCards`).
 */
export function needsInitialLayout(layout: FrameLayout): boolean {
  return (layout.structure ?? []).length > 0
}

/** FR-144 方式 2: what the selected nodes become in a frame of this template. */
export type Conversion = {
  mode: 'bound' | 'mixed' | 'free'
  /** Core objects the frame follows (bound cards; their state comes from the object's phase). */
  objects: string[]
  /** Nodes copied as the person's own free cards, parents before children. */
  copies: { node: string; title: string; parent: string | null }[]
  /** Relation lines between copies, by node ID. */
  links: { from: string; to: string }[]
}

/**
 * FR-144 方式 2, the field mapping: with a template that has states and a
 * done state, selected core objects become bound cards and follow their
 * phase (FR-148, FR-124); every other node is copied as a free card in
 * the person's name. The canvas lines between copies become parents in a
 * tree or nesting and relation lines in a relation frame. Nothing on the
 * canvas changes; the nodes stay where they are.
 */
export function conversion(template: FrameTemplate, nodes: CanvasNode[], edges: CanvasEdge[]): Conversion {
  const chosen = nodes.filter((node) => node.kind !== 'group').slice(0, CONVERT_MAX)
  const binds = bindable(template)
  const objects = binds ? [...new Set(chosen.flatMap((node) => (node.object_id ? [node.object_id] : [])))] : []
  const copied = chosen.filter((node) => !binds || !node.object_id)
  const byId = new Map(copied.map((node) => [node.id, node]))
  const structure = template.layout?.structure ?? []
  const nested = structure.includes('tree') || structure.includes('nesting')
  const within = edges.filter((edge) => byId.has(edge.from_node) && byId.has(edge.to_node) && edge.from_node !== edge.to_node)
  const parentOf = new Map<string, string>()
  if (nested) for (const edge of within) if (!parentOf.has(edge.to_node)) parentOf.set(edge.to_node, edge.from_node)
  const max = template.limits?.title_max ?? 30
  const copies: Conversion['copies'] = []
  const seen = new Set<string>()
  const visit = (node: CanvasNode) => {
    if (seen.has(node.id)) return
    seen.add(node.id)
    const above = parentOf.get(node.id)
    if (above) visit(byId.get(above)!)
    // A line that closes a loop gives no parent: the card starts its own branch.
    const parent = above && copies.some((entry) => entry.node === above) ? above : null
    const title = [...(node.title.replace(/\s+/g, ' ').trim() || node.id)].slice(0, max).join('')
    copies.push({ node: node.id, title, parent })
  }
  copied.forEach(visit)
  const parents = new Set(copies.flatMap((copy) => (copy.parent ? [`${copy.parent}>${copy.node}`] : [])))
  const links = structure.includes('relations')
    ? [...new Map(within.filter((edge) => !parents.has(`${edge.from_node}>${edge.to_node}`))
      .map((edge) => [`${edge.from_node}>${edge.to_node}`, { from: edge.from_node, to: edge.to_node }])).values()]
    : []
  const mode = objects.length ? (copies.length ? 'mixed' : 'bound') : 'free'
  return { mode, objects, copies, links }
}

/**
 * The only place elkjs runs (FR-144 方式 2): the first layout of the cards
 * a person turns into a frame, from the lines between them. It is loaded on
 * demand, so a fast-layer update never loads or calls it. The result is
 * sent as the person's pins, so later updates never lay these cards out
 * again.
 */
export async function initialLayout(cards: string[], edges: { from: string; to: string }[]): Promise<{ pins: Record<string, Pin>; width: number; height: number }> {
  const { default: ELK } = await import('elkjs/lib/elk.bundled.js')
  const ids = new Set(cards)
  const graph: ElkNode = await new ELK().layout({
    id: 'frame',
    layoutOptions: { 'elk.algorithm': 'layered', 'elk.direction': 'RIGHT', 'elk.spacing.nodeNode': '16', 'elk.layered.spacing.nodeNodeBetweenLayers': '48' },
    children: cards.map((id) => ({ id, width: CARD_W, height: CARD_H })),
    edges: edges.filter((edge) => ids.has(edge.from) && ids.has(edge.to) && edge.from !== edge.to)
      .map((edge, index) => ({ id: `e${index}`, sources: [edge.from], targets: [edge.to] })),
  })
  const pins: Record<string, Pin> = {}
  for (const child of graph.children ?? []) pins[child.id] = { x: PAD + Math.round(child.x ?? 0), y: FRAME_HEAD + PAD + Math.round(child.y ?? 0) }
  const clamp = (n: number) => Math.min(Math.max(Math.round(n), FRAME_MIN_SIZE), FRAME_MAX_SIZE)
  return { pins, width: clamp((graph.width ?? 0) + 2 * PAD), height: clamp(FRAME_HEAD + PAD + (graph.height ?? 0) + PAD + FOOT) }
}
