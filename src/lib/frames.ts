// FR-141–146, FR-153, FR-156: frames a person places from a template. The
// daemon checks every rule again; these functions only shape what the App
// shows and asks for. Nothing here moves a frame the person did not move.
import type { Node } from '@xyflow/svelte'
import type { CalloutKind, CardChange, CardOp, FrameCallout, FrameEntry, FrameHistory, FrameHistoryAt, FrameLayout, FramePending, FramePendingWhy, FramePlace, FrameRules, FrameStep, FrameTemplate, FrameAxis } from './api'
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
export type FrameChoice = { template: FrameTemplate; title: string; rules: string; setBy: 'human' | 'delegate'; mode?: 'bound' | 'mixed' }
/** FR-148: bound and mixed cards follow core objects, so the template needs a done state and a state before it. */
export function bindable(template: FrameTemplate): boolean {
  const done = template.done_states ?? []
  return done.length > 0 && (template.states ?? []).length > done.length
}
/** A person's acts on a frame's content, all through the daemon (H2). */
export type FrameActions = {
  card?: (frame: string, target: string, op: Exclude<CardOp, { op: 'rules' }>) => void
  rules?: (frame: string, rules: FrameRules) => void
  resolve?: (frame: string, pending: number, accept: boolean) => void
  /** FR-149: a person drags cards to fixed places; `null` lets one go back to the derived layout. */
  pin?: (frame: string, pins: Record<string, { x: number; y: number } | null>) => void
  /** FR-154: ask to write the frame into `tower.canvas`; the App shows what it would write before the person approves. */
  materialize?: (frame: string) => void
  /** FR-156: ask about the frame, or ask whoever writes it to update it by its rules. */
  ask?: (frame: string) => void
}
/** FR-157/158: read-only history; reading it changes nothing. */
export type FrameReader = {
  steps: (frame: string) => Promise<FrameHistory>
  at: (frame: string, seq: number) => Promise<FrameHistoryAt>
}
/** What the canvas tells a frame, beside the person's acts (H3). */
export type FrameView = {
  /** FR-151: cards changed after this time (ms) get a thin border. */
  since?: number
  /** FR-141: the card the pointer is on, or the one clicked, for lines that cross frames. */
  onFocus?: (card: string | null, sticky: boolean) => void
  history?: FrameReader
}
export type FrameNode = Node<{ frame: FrameEntry; badge?: FrameBadge; onPlace?: (id: string, place: FramePlace) => void; actions?: FrameActions } & FrameView, 'frame'>

function isPlace(value: unknown): value is FramePlace {
  const place = value as Partial<FramePlace> | null
  return !!place && [place.x, place.y, place.width, place.height].every((n) => Number.isSafeInteger(n))
}

/**
 * Frames from the projection, oldest first so later ones draw on top. The
 * daemon's `frames` view, when present, adds bound cards and `frozen`.
 */
export function framesFrom(projection: unknown, views?: FrameEntry[] | null): FrameEntry[] {
  const group = (projection as Record<string, unknown> | null | undefined)?.Frame
  if (!group || typeof group !== 'object') return []
  const derived = new Map((Array.isArray(views) ? views : []).filter((view) => typeof view?.id === 'string').map((view) => [view.id, view]))
  return Object.values(group as Record<string, FrameEntry>)
    .filter((frame) => typeof frame?.id === 'string' && typeof frame.title === 'string' && isPlace(frame.place)
      && Number.isSafeInteger(frame.created?.seq) && !!frame.layout)
    .map((frame) => {
      const view = derived.get(frame.id)
      return view ? { ...frame, bound: Array.isArray(view.bound) ? view.bound : [], frozen: view.frozen === true } : frame
    })
    .sort((a, b) => a.created.seq - b.created.seq)
}

/** One card as a frame shows it: a free card, a core object's, or a proposal waiting for a person. */
export type ShownCard = {
  id: string; title: string; body?: string; state?: string | null; fields: Record<string, unknown>
  kind: 'free' | 'bound' | 'proposed'; done: boolean; parent?: string | null; links: string[]
  /** Suggestions, confirmations and proposals that name this card. */
  waiting: FramePending[]
  /** Sequence of the last change; the order cards arrived in. */
  seq: number
  /** When it last changed, for FR-151's thin border; bound cards follow their object and carry none. */
  changed?: string
}

export const pendingLabels: Record<FramePendingWhy, string> = {
  proposal: '提案', suggestion: '建议', confirm: '待确认完成', rules: '规则提案',
}

/** FR-148, FR-153: what a frame shows, in arrival order. Archived cards are gone; bound cards follow their object. */
export function shownCards(frame: FrameEntry): ShownCard[] {
  const pending = frame.pending ?? []
  const done = new Set(frame.done_states)
  const waitingOn = (id: string) => pending.filter((item) => item.target === id && item.op.op !== 'add')
  const free: ShownCard[] = (frame.cards ?? []).filter((card) => !card.archived).map((card) => ({
    id: card.id, title: card.title, body: card.body, state: card.state ?? null, fields: card.fields ?? {}, kind: 'free',
    done: !!card.state && done.has(card.state), parent: card.parent ?? null, links: card.links ?? [], waiting: waitingOn(card.id), seq: card.added.seq, changed: card.changed?.at,
  }))
  const bound: ShownCard[] = (frame.bound ?? []).map((object) => ({
    id: object.id, title: object.title, state: object.state, fields: {}, kind: 'bound', done: object.done, links: [], waiting: [], seq: 0,
  }))
  const proposed: ShownCard[] = pending.filter((item) => item.op.op === 'add').map((item) => {
    const op = item.op as Extract<CardOp, { op: 'add' }>
    return { id: `pending:${item.id}`, title: op.title, body: op.body, state: op.state ?? frame.states[0] ?? null, fields: op.fields ?? {},
      kind: 'proposed', done: false, parent: op.parent ?? null, links: [], waiting: [item], seq: item.id, changed: item.by?.at }
  })
  return [...bound, ...free, ...proposed]
}

/** The category a card falls in on this axis; `null` when the axis is not a category or the card has no valid value. */
export function laneValue(card: Pick<ShownCard, 'state' | 'fields'>, axis: FrameAxis | undefined): string | null {
  if (!axis || typeof axis !== 'object' || !('category' in axis)) return null
  const { field, values } = axis.category
  const value = field === 'state' ? card.state : card.fields[field]
  return typeof value === 'string' && values.includes(value) ? value : null
}

/** FR-151: the per-lane limit, the rules' over the template's. */
export function perLaneMax(frame: FrameEntry): number | null {
  return frame.rules.per_lane_max ?? frame.limits.per_lane_max ?? null
}

/** Where a cell's cards go inside a frame of this size, below the lane labels and above the rules line. */
export function cellRect(layout: FrameLayout, width: number, height: number, x: string | null, y: string | null): { left: number; top: number; width: number; height: number } {
  const lanes = frameLanes(layout, width, height)
  const column = x === null ? undefined : lanes.columns.find((lane) => lane.label === x)
  const row = y === null ? undefined : lanes.rows.find((lane) => lane.label === y)
  const top = (row?.start ?? FRAME_HEAD) + 24
  const bottom = row ? row.start + row.size : height - 28
  return { left: (column?.start ?? 0) + 8, top, width: Math.max((column?.size ?? width) - 16, 0), height: Math.max(bottom - top, 0) }
}

/** The rules with `update_until` set or cleared; every other rule is kept as is. */
export function withUntil(rules: FrameRules, date: string | null): FrameRules {
  const next: FrameRules = { ...rules }
  if (date) next.update_until = { date }
  else delete next.update_until
  return next
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

function sameContent(a: FrameEntry, b: FrameEntry): boolean {
  const content = (frame: FrameEntry) => JSON.stringify([frame.cards ?? [], frame.pending ?? [], frame.bound ?? [], frame.frozen ?? false, frame.rules, frame.pins ?? {},
    frame.callouts ?? [], frame.materialized ?? null])
  return content(a) === content(b)
}

/** FR-151 (after FR-50): `later` when only cards, rules or waiting items changed, merged into one visual change a second; `now` otherwise. */
export function frameChange(shown: FrameEntry[], next: FrameEntry[]): 'same' | 'now' | 'later' {
  if (shown.length !== next.length || shown.some((frame, index) => frame.id !== next[index].id)) return 'now'
  let change: 'same' | 'later' = 'same'
  for (const [index, frame] of next.entries()) {
    const old = shown[index]
    if (!samePlace(old.place, frame.place) || old.title !== frame.title || JSON.stringify(old.pins ?? {}) !== JSON.stringify(frame.pins ?? {})) return 'now'
    if (!sameContent(old, frame)) change = 'later'
  }
  return change
}

/** Pins the person sent, shown until the live state has them; `null` unpins. */
export function withPins(frame: FrameEntry, pins: Record<string, { x: number; y: number } | null>): FrameEntry {
  const next = { ...(frame.pins ?? {}) }
  for (const [id, pin] of Object.entries(pins)) {
    if (pin) next[id] = pin
    else delete next[id]
  }
  return { ...frame, pins: next }
}

/**
 * Frames as canvas nodes, behind every card. A node is reused while its
 * frame, badge and editability stay the same, so a fast-layer change never
 * resets a frame the person is dragging.
 */
export function toFrameNodes(frames: FrameEntry[], badges: Record<string, FrameBadge>, editable: boolean,
  onPlace: ((id: string, place: FramePlace) => void) | undefined, cache: Map<string, FrameNode>, actions?: FrameActions, view: FrameView = {}): FrameNode[] {
  const live = new Set(frames.map((frame) => frame.id))
  const nodes = frames.map((frame): FrameNode => {
    const previous = cache.get(frame.id)
    const badge = badges[frame.id]
    if (previous && previous.draggable === editable && previous.data.onPlace === onPlace && previous.data.actions === actions
      && previous.data.since === view.since && previous.data.onFocus === view.onFocus && previous.data.history === view.history
      && samePlace(previous.data.frame.place, frame.place) && previous.data.frame.title === frame.title && sameContent(previous.data.frame, frame)
      && previous.data.badge?.label === badge?.label
      && previous.data.badge?.tone === badge?.tone && previous.data.badge?.count === badge?.count) return previous
    const { x, y, width, height } = frame.place
    const node: FrameNode = {
      id: `frame:${frame.id}`,
      type: 'frame',
      position: { x, y },
      width,
      height,
      style: `width: ${width}px; height: ${height}px;`,
      data: { frame, badge: badge ? { ...badge } : undefined, onPlace, actions, since: view.since, onFocus: view.onFocus, history: view.history },
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

// FR-160/161: callouts sit beside a card or the frame; they are drawn over the
// frame and never take part in where a card goes.
/** Shape, colour class and words, so a callout never relies on colour alone. */
export const calloutLabels: Record<CalloutKind, { icon: string; label: string }> = {
  why: { icon: '◆', label: '为什么' },
  adjustment: { icon: '✎', label: '调整' },
  question: { icon: '?', label: '问题' },
}
/** The callouts a person did not hide, grouped by what they point at. */
export function calloutsFor(frame: FrameEntry, hidden: readonly CalloutKind[] = []): Map<string, FrameCallout[]> {
  const out = new Map<string, FrameCallout[]>()
  for (const callout of frame.callouts ?? []) {
    if (hidden.includes(callout.kind)) continue
    out.set(callout.target, [...(out.get(callout.target) ?? []), callout])
  }
  return out
}

// FR-157/158: change ranges and history steps. Highlighting only adds badges
// and fades cards; it never moves one.
export type ChangeRange = 'visit' | 'today' | 'week' | 'date'
export const changeRangeLabels: Record<ChangeRange, string> = { visit: '上次查看以来', today: '今天', week: '本周', date: '自某日' }
export const cardChangeLabels: Record<CardChange, { icon: string; label: string }> = {
  new: { icon: '+', label: '新' },
  state: { icon: '→', label: '状态' },
  text: { icon: '✎', label: '文本' },
}
/** When a range starts (ms, local time), or `null` when it cannot be known. */
export function rangeStart(range: ChangeRange, now: number, since?: number, date?: string): number | null {
  const day = new Date(now)
  day.setHours(0, 0, 0, 0)
  switch (range) {
    case 'visit': return since ?? null
    case 'today': return day.getTime()
    case 'week': day.setDate(day.getDate() - ((day.getDay() + 6) % 7)); return day.getTime()
    case 'date': {
      if (!date || !/^\d{4}-\d{2}-\d{2}$/.test(date)) return null
      const start = new Date(`${date}T00:00:00`).getTime()
      return Number.isNaN(start) ? null : start
    }
  }
}
const changeOrder: CardChange[] = ['new', 'state', 'text']
/** Every card that changed at or after `start`, with each kind of change once. */
export function changesSince(steps: readonly FrameStep[], start: number): Record<string, CardChange[]> {
  const found: Record<string, Set<CardChange>> = {}
  for (const step of steps) {
    if (Date.parse(step.at) < start) continue
    for (const [card, changes] of Object.entries(step.cards)) for (const change of changes) (found[card] ??= new Set()).add(change)
  }
  return Object.fromEntries(Object.entries(found).map(([card, kinds]) => [card, changeOrder.filter((kind) => kinds.has(kind))]))
}
/** The frame as it was at a past step, kept where the live frame is so nothing moves. */
export function pastFrame(live: FrameEntry, at: FrameHistoryAt): FrameEntry | null {
  const then = at.view.find((frame) => frame.id === live.id)
  return then ? { ...then, place: live.place } : null
}
