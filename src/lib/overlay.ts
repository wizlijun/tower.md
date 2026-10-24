// U-4: what the canvas draws over the version it shows. Proposed changes are
// dashed (FR-45), both ends of a relation a proposal touches light up
// (FR-36), and the previous version can show as a ghost layer (FR-46). All of
// it is read from proposals and draft comparisons the daemon already sends;
// nothing here writes, and nothing here moves a node.
import type { CanvasNode, DiffKind, DraftComparison } from './api'
import { DIFF_LABELS } from './comparison'

/** FR-45, FR-36: marks on one card. Keys are object IDs, like card statuses. */
export type CanvasMark = {
  /** FR-45: what the proposal changes here; shown dashed until approved. */
  proposed?: string
  /** FR-36: titles of the other ends of a relation a proposal touches. */
  relation?: string[]
}

/** Shape (symbol), colour (CSS class) and words for each layer, so none rests on colour alone. */
export const OVERLAY_LABELS = {
  proposed: { symbol: '┄', label: '拟议' },
  relation: { symbol: '⇄', label: '关系一端' },
  ghost: { symbol: '◌', label: '旧版本' },
} as const

/** A card of the previous version: gone from the new one, or moved or resized in it. */
export type Ghost = { node: CanvasNode; why: 'removed' | 'moved' }
export const GHOST_REASONS: Record<Ghost['why'], string> = { removed: '已删除', moved: '原位置' }

const key = (node: Pick<CanvasNode, 'id' | 'object_id'>) => node.object_id || node.id

/**
 * FR-46: old-version cards to draw behind the new version. Groups are left
 * out: a resized container would cover the whole canvas.
 */
export function ghostNodes(before: readonly CanvasNode[], after: readonly CanvasNode[]): Ghost[] {
  const now = new Map(after.map((node) => [key(node), node]))
  return before.filter((node) => node.kind !== 'group').flatMap((node): Ghost[] => {
    const next = now.get(key(node))
    if (!next) return [{ node, why: 'removed' }]
    const same = next.x === node.x && next.y === node.y && next.width === node.width && next.height === node.height
    return same ? [] : [{ node, why: 'moved' }]
  })
}

/** Ends a relation object names in its frontmatter (`from`, `to`, `module`; matches the daemon's impact). */
export function relationEnds(content: string): string[] {
  const front = content.match(/^---\r?\n([\s\S]*?)\r?\n---(?:\r?\n|$)/)?.[1] ?? ''
  return ['from', 'to', 'from_module', 'to_module', 'module'].flatMap((name) => {
    const value = front.match(new RegExp(`^${name}:\\s*["']?([^"'\\r\\n]+?)["']?\\s*$`, 'm'))?.[1]
    return value ? [value] : []
  })
}

/** Collects marks for the cards in `shown`; `others` only lend titles (a removed card's, say). */
function marker(shown: readonly CanvasNode[], others: readonly CanvasNode[] = []) {
  const proposed = new Map<string, Set<string>>()
  const related = new Map<string, Set<string>>()
  const present = new Set(shown.map(key))
  const title = (id: string) => [...shown, ...others].find((node) => key(node) === id)?.title ?? id
  const add = (into: Map<string, Set<string>>, id: string, values: string[]) => {
    if (!present.has(id) || !values.length) return
    const set = into.get(id) ?? new Set<string>()
    for (const value of values) set.add(value)
    into.set(id, set)
  }
  return {
    propose: (id: string, text: string) => add(proposed, id, [text]),
    /** Lights up every end of one relation with the titles of the others. */
    relate(ids: readonly string[]) {
      const ends = [...new Set(ids)]
      if (ends.length < 2) return
      for (const id of ends) add(related, id, ends.filter((end) => end !== id).map(title))
    },
    marks(): Record<string, CanvasMark> {
      const marks: Record<string, CanvasMark> = {}
      for (const [id, texts] of proposed) marks[id] = { proposed: [...texts].join('；') }
      for (const [id, titles] of related) marks[id] = { ...marks[id], relation: [...titles] }
      return marks
    },
  }
}

const ORDER = Object.keys(DIFF_LABELS) as DiffKind[]

/**
 * FR-45, FR-36, FR-35: a draft entered from the approved version, drawn as a
 * structure diagram. Changed cards are dashed with what changed; removed ones
 * show only as ghosts. A merge, a split or a moved relation end lights up both
 * ends; `before` is the approved version the draft is compared with.
 */
export function draftMarks(comparison: DraftComparison['comparison'], after: readonly CanvasNode[], before: readonly CanvasNode[]): Record<string, CanvasMark> {
  const { marks, propose, relate } = marker(after, before)
  const content = (nodes: readonly CanvasNode[], id: string) => nodes.find((node) => key(node) === id)?.content ?? ''
  for (const object of comparison.objects) {
    if (!object.kinds.includes('removed')) {
      propose(object.id, ORDER.filter((kind) => object.kinds.includes(kind)).map((kind) => `${DIFF_LABELS[kind].mark} ${DIFF_LABELS[kind].label}`).join(' · '))
    }
    if (object.related) relate([object.id, object.related])
    if (object.kinds.includes('endpoints')) {
      const ends = [...relationEnds(content(after, object.id)), ...relationEnds(content(before, object.id))]
      // An interface is the relation itself; an acceptance is one end of its relation to a module.
      relate(object.object_type === 'Interface' ? ends : [object.id, ...ends])
    }
  }
  return marks()
}

function record(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {}
}
function strings(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []
}

/** Proposal kinds that change how two objects relate (FR-36). */
const RELATION_KINDS = ['merge', 'split', 'interface']

/**
 * FR-45, FR-36 on the approved version: objects a waiting proposal would
 * change are dashed, and a merge, split or interface proposal lights up every
 * module it relates. Approved, rejected and superseded proposals draw nothing,
 * so an approved change turns solid.
 */
export function proposalMarks(proposals: unknown, nodes: readonly CanvasNode[]): Record<string, CanvasMark> {
  const { marks, propose, relate } = marker(nodes)
  for (const value of Object.values(record(proposals))) {
    const entry = record(value)
    if (entry.status !== 'submitted' && entry.status !== 'draft') continue
    const proposal = record(entry.proposal), impact = record(proposal.impact)
    const text = entry.status === 'submitted' ? '提案待审阅' : '提案待补全'
    for (const id of [...strings(impact.modules), ...strings(impact.interfaces)]) propose(id, text)
    if (RELATION_KINDS.includes(String(proposal.kind))) relate(strings(impact.modules))
  }
  return marks()
}

export function sameMark(a: CanvasMark | undefined, b: CanvasMark | undefined): boolean {
  return a?.proposed === b?.proposed && (a?.relation ?? []).join('\n') === (b?.relation ?? []).join('\n')
}

/** FR-46: how long the layout takes to settle into a new version (300–800 ms). */
export const TRANSITION_MS = 500
export type Point = { x: number; y: number }
export type Move = { from: Point; to: Point }

/** FR-46: cards whose place changed between two versions, by canvas node ID. */
export function layoutMoves(before: readonly CanvasNode[], after: readonly CanvasNode[]): Map<string, Move> {
  const old = new Map(before.map((node) => [key(node), node]))
  const moves = new Map<string, Move>()
  for (const node of after) {
    const was = old.get(key(node))
    if (was && (was.x !== node.x || was.y !== node.y)) moves.set(node.id, { from: { x: was.x, y: was.y }, to: { x: node.x, y: node.y } })
  }
  return moves
}

/** Where a moving card is at `t` (0–1) of the transition, eased in and out. */
export function tweenAt(move: Move, t: number): Point {
  const p = Math.min(Math.max(t, 0), 1)
  const eased = p < 0.5 ? 4 * p * p * p : 1 - (-2 * p + 2) ** 3 / 2
  return { x: move.from.x + (move.to.x - move.from.x) * eased, y: move.from.y + (move.to.y - move.from.y) * eased }
}
