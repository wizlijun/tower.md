// FR-141–146, FR-153, FR-156: frames a person places from a template. The
// daemon checks every rule again; these functions only shape what the App
// shows and asks for. Nothing here moves a frame the person did not move.
import type { Node } from '@xyflow/svelte'
import type { FrameEntry, FrameLayout, FramePlace, FrameTemplate, FrameAxis } from './api'
import { signalQuiet, type SignalEntry } from './attention'

/** The daemon's limits (`frames::TITLE_CHARS`, `RULES_CHARS`, `MIN_SIZE`, `MAX_SIZE`). */
export const FRAME_TITLE_CHARS = 60
export const FRAME_RULES_CHARS = 300
export const FRAME_MIN_SIZE = 160
export const FRAME_MAX_SIZE = 20000
/** Room for the title bar above the lanes. */
export const FRAME_HEAD = 44
const LANE = 220
const GAP = 40

/** What a template carries while the person drags it onto the canvas. */
export const TEMPLATE_MIME = 'application/x-towermd-template'

export type FrameBadge = { count: number; label: string; tone: 'warn' | 'info' }
/** What the person chose in the template tray. */
export type FrameChoice = { template: FrameTemplate; title: string; rules: string; setBy: 'human' | 'delegate' }
export type FrameNode = Node<{ frame: FrameEntry; badge?: FrameBadge; onPlace?: (id: string, place: FramePlace) => void }, 'frame'>

function isPlace(value: unknown): value is FramePlace {
  const place = value as Partial<FramePlace> | null
  return !!place && [place.x, place.y, place.width, place.height].every((n) => Number.isSafeInteger(n))
}

/** Frames from the projection, oldest first so later ones draw on top. */
export function framesFrom(projection: unknown): FrameEntry[] {
  const group = (projection as Record<string, unknown> | null | undefined)?.Frame
  if (!group || typeof group !== 'object') return []
  return Object.values(group as Record<string, FrameEntry>)
    .filter((frame) => typeof frame?.id === 'string' && typeof frame.title === 'string' && isPlace(frame.place)
      && Number.isSafeInteger(frame.created?.seq) && !!frame.layout)
    .sort((a, b) => a.created.seq - b.created.seq)
}

function values(axis: FrameAxis | undefined): string[] {
  return axis && typeof axis === 'object' && 'category' in axis ? axis.category.values : []
}

/** A size that fits the template's lanes; the person resizes from there. */
export function defaultSize(layout: FrameLayout): { width: number; height: number } {
  const columns = values(layout.x).length
  const rows = values(layout.y).length
  const clamp = (n: number) => Math.min(Math.max(Math.round(n), FRAME_MIN_SIZE), FRAME_MAX_SIZE)
  if (columns && rows) return { width: clamp(columns * 320), height: clamp(rows * 240 + FRAME_HEAD) }
  if (columns) return { width: clamp(columns * LANE), height: 420 }
  if (rows) return { width: 640, height: clamp(rows * 160 + FRAME_HEAD) }
  if (layout.x && typeof layout.x === 'object' && 'time' in layout.x) return { width: 960, height: 320 }
  return { width: 720, height: 480 }
}

export type Lane = { label: string; start: number; size: number }
/** Columns and rows drawn inside a frame of this size, in frame coordinates. */
export function frameLanes(layout: FrameLayout, width: number, height: number): { columns: Lane[]; rows: Lane[] } {
  const split = (labels: string[], start: number, total: number): Lane[] => labels.map((label, index) => {
    const from = Math.round(start + (total * index) / labels.length)
    const to = Math.round(start + (total * (index + 1)) / labels.length)
    return { label, start: from, size: to - from }
  })
  return { columns: split(values(layout.x), 0, width), rows: split(values(layout.y), FRAME_HEAD, Math.max(height - FRAME_HEAD, 0)) }
}

/** A short description of how a template lays cards out. */
export function layoutLabel(layout: FrameLayout): string {
  const axis = (value: FrameAxis | undefined, name: string) => {
    if (!value || value === 'none') return null
    if (value === 'order') return `${name}按添加顺序`
    if ('time' in value) return `${name}按时间（${value.time.field}）`
    return `${name}按${value.category.field === 'state' ? '状态' : value.category.field}分栏`
  }
  const parts = [axis(layout.x, '横向'), axis(layout.y, '纵向')].filter(Boolean) as string[]
  if (layout.structure?.includes('tree')) parts.push('树状展开')
  if (layout.structure?.includes('nesting')) parts.push('可嵌套')
  if (layout.structure?.includes('relations')) parts.push('显示关系')
  return parts.join('，') || '自由摆放'
}

function overlaps(a: FramePlace, b: FramePlace): boolean {
  return a.x < b.x + b.width + GAP && b.x < a.x + a.width + GAP && a.y < b.y + b.height + GAP && b.y < a.y + a.height + GAP
}

/**
 * FR-144: the nearest spot to `at` (the drop point or the view's centre)
 * where a new frame covers nothing already on the canvas. Nothing else moves.
 */
export function blankSpot(at: { x: number; y: number }, size: { width: number; height: number }, taken: FramePlace[]): FramePlace {
  const start = { x: Math.round(at.x - size.width / 2), y: Math.round(at.y - size.height / 2) }
  const free = (x: number, y: number) => !taken.some((other) => overlaps({ x, y, ...size }, other))
  if (free(start.x, start.y)) return { ...start, ...size }
  const step = 80
  for (let ring = 1; ring <= 200; ring += 1) {
    const spots: { x: number; y: number; d: number }[] = []
    for (let i = -ring; i <= ring; i += 1) {
      for (const [dx, dy] of [[i, -ring], [i, ring], [-ring, i], [ring, i]]) {
        spots.push({ x: start.x + dx * step, y: start.y + dy * step, d: dx * dx + dy * dy })
      }
    }
    spots.sort((a, b) => a.d - b.d || a.y - b.y || a.x - b.x)
    const spot = spots.find((candidate) => free(candidate.x, candidate.y))
    if (spot) return { x: spot.x, y: spot.y, ...size }
  }
  return { ...start, ...size }
}

/** FR-156: open signals that name a frame show as a badge on it; logs and quieted ones do not. */
export function frameBadges(signals: SignalEntry[], frames: FrameEntry[], now: number): Record<string, FrameBadge> {
  const ids = new Set(frames.map((frame) => frame.id))
  const badges: Record<string, FrameBadge> = {}
  for (const signal of signals) {
    if (!ids.has(signal.target) || signal.data.level === 'log' || signalQuiet(signal, now)) continue
    const badge = badges[signal.target] ?? { count: 0, label: '', tone: 'info' as const }
    badge.count += 1
    if (signal.data.level !== 'passive') badge.tone = 'warn'
    badge.label = badge.count === 1 ? signal.data.title : `${badge.count} 条提醒`
    badges[signal.target] = badge
  }
  return badges
}

/** A fresh `C-…` ID no frame on this canvas has. */
export function newFrameId(taken: Iterable<string>, random: () => number = Math.random): string {
  const used = new Set(taken)
  for (;;) {
    let suffix = ''
    for (let i = 0; i < 8; i += 1) suffix += Math.floor(random() * 36).toString(36)
    const id = `C-${suffix}`
    if (!used.has(id)) return id
  }
}

export function frameTitleError(title: string): string | null {
  const value = title.trim()
  if (!value) return '给容器起个名字。'
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001f\u007f-\u009f]/.test(value)) return '名字里不能有换行或控制字符。'
  if ([...value].length > FRAME_TITLE_CHARS) return `名字最多 ${FRAME_TITLE_CHARS} 个字。`
  return null
}

export function rulesTextError(text: string): string | null {
  if ([...text.trim()].length > FRAME_RULES_CHARS) return `规则最多 ${FRAME_RULES_CHARS} 个字。`
  return null
}

export function usable(template: FrameTemplate): boolean {
  return !template.problems.length && !!template.layout
}

export function samePlace(a: FramePlace, b: FramePlace): boolean {
  return a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height
}

/**
 * Frames as canvas nodes, behind every card. A node is reused while its
 * frame, badge and editability stay the same, so a fast-layer change never
 * resets a frame the person is dragging.
 */
export function toFrameNodes(frames: FrameEntry[], badges: Record<string, FrameBadge>, editable: boolean,
  onPlace: ((id: string, place: FramePlace) => void) | undefined, cache: Map<string, FrameNode>): FrameNode[] {
  const live = new Set(frames.map((frame) => frame.id))
  const nodes = frames.map((frame): FrameNode => {
    const previous = cache.get(frame.id)
    const badge = badges[frame.id]
    if (previous && previous.draggable === editable && previous.data.onPlace === onPlace && samePlace(previous.data.frame.place, frame.place)
      && previous.data.frame.title === frame.title && previous.data.badge?.label === badge?.label
      && previous.data.badge?.tone === badge?.tone && previous.data.badge?.count === badge?.count) return previous
    const { x, y, width, height } = frame.place
    const node: FrameNode = {
      id: `frame:${frame.id}`,
      type: 'frame',
      position: { x, y },
      width,
      height,
      style: `width: ${width}px; height: ${height}px;`,
      data: { frame, badge: badge ? { ...badge } : undefined, onPlace },
      draggable: editable,
      dragHandle: '.frame-node-head',
      connectable: false,
      deletable: false,
      selectable: false,
      zIndex: -2,
    }
    cache.set(frame.id, node)
    return node
  })
  for (const id of cache.keys()) if (!live.has(id)) cache.delete(id)
  return nodes
}
