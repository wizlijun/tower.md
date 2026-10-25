import type { FrameAxis, FrameEntry, SourceLink } from './api'
import { FRAME_HEAD, cellRect, frameLanes, laneValue, perLaneMax, type Lane, type ShownCard } from './frames'

/**
 * FR-149: where each card sits inside its frame, derived from the data and
 * nothing else. The rules every layout keeps:
 * - cards are laid out in the order they arrived, so a new card lands at the
 *   end of its lane, slot or branch and never pushes a card already shown;
 * - past what fits, later arrivals fold into "+N";
 * - a card the person dragged is pinned and stays where they put it.
 * Pure and deterministic: the same frame, cards and pins give the same
 * placement. It never calls elkjs; that is only for a new frame's first
 * layout (`initial-layout.ts`).
 */
export const CARD_W = 180
export const CARD_H = 44
export const CARD_GAP = 8
const BRANCH = 48
const PAD = 12
const FOLD_H = 20
const MARK = 24
const FOOT = 28

export type Pin = { x: number; y: number }
export type Rect = { x: number; y: number; width: number; height: number }
export type PlacedCard = Rect & { card: ShownCard; pinned: boolean }
export type Fold = Rect & { count: number }
export type Line = { from: string; to: string; kind: 'parent' | 'link' }
export type AxisMark = { label: string; x: number; y: number }
export type Placement = {
  cards: PlacedCard[]
  /** "+N" chips: cards past a lane's limit or past what the frame can show. */
  folds: Fold[]
  lines: Line[]
  /** Slot labels on a time axis. */
  marks: AxisMark[]
  /** Links to cards in other frames, drawn on the canvas only on hover or select. */
  outside: { from: string; to: string }[]
  /** FR-172: a sequence diagram's messages, one row each in file order, and each participant's lifeline. */
  messages: Message[]
  lifelines: Lifeline[]
  /** FR-172: a Gantt chart's task bars; progress and milestones are written as text too. */
  bars: Bar[]
}
export type Bar = Rect & { id: string; completed?: number; milestone: boolean }
/** `edge`: which end sits on the frame's side, for a message into or out of the diagram. */
export type Message = {
  from: string; to: string; x1: number; x2: number; y: number; line: number
  label?: string; dashed: boolean; head?: SourceLink['head']; edge?: 'from' | 'to'
}
export type Lifeline = { id: string; x: number; y1: number; y2: number }

/** FR-172: the layout a text-source frame's file asks for, read from its first diagram the canvas understands. */
export function sourceModel(frame: FrameEntry): 'sequence' | 'gantt' | null {
  const diagram = frame.source?.diagrams?.find((entry) => entry.supported)
  if (diagram?.sequence) return 'sequence'
  return diagram?.kind === 'gantt' ? 'gantt' : null
}

const MESSAGE_H = 28
const COLUMN_GAP = 40
const DAY_MAX = 48
const MARK_GAP = 72

/**
 * Participants across in the order the file declares them, messages down
 * in file order. Neither depends on state, so a state change moves nothing;
 * a new participant or message lands after the ones already shown.
 */
function sequence(cards: ShownCard[], body: Rect, out: Placement) {
  cards.forEach((card, index) => {
    out.cards.push({ card, pinned: false, width: CARD_W, height: CARD_H, x: body.x + index * (CARD_W + COLUMN_GAP), y: body.y })
  })
}

/** After pins: each message runs between its ends' lifelines; one past the frame's foot folds into "+N". */
function messages(links: SourceLink[], width: number, body: Rect, out: Placement) {
  const column = new Map(out.cards.map((placed) => [placed.card.id, placed]))
  const top = body.y + CARD_H + MESSAGE_H / 2 + CARD_GAP
  const rows = Math.max(0, Math.floor((body.y + body.height - top) / MESSAGE_H) + 1)
  const edge = (key: string) => key === '[' ? PAD / 2 : key === ']' ? width - PAD / 2 : null
  const xOf = (key: string) => {
    const placed = column.get(key)
    return placed ? placed.x + placed.width / 2 : edge(key)
  }
  let skipped = 0
  for (const link of links) {
    const x1 = xOf(link.from)
    const x2 = xOf(link.to)
    if (x1 === null || x2 === null) continue
    if (out.messages.length >= rows) { skipped += 1; continue }
    out.messages.push({
      from: link.from, to: link.to, x1, x2, y: top + out.messages.length * MESSAGE_H, line: link.line, label: link.label,
      dashed: !!link.dashed, head: link.head, edge: link.boundary ? (column.has(link.from) ? 'to' : 'from') : undefined,
    })
  }
  const bottom = out.messages.length ? out.messages[out.messages.length - 1].y + MESSAGE_H / 2 : top
  for (const placed of out.cards) {
    if (placed.card.kind === 'source') out.lifelines.push({ id: placed.card.id, x: placed.x + placed.width / 2, y1: placed.y + placed.height, y2: bottom })
  }
  if (skipped) out.folds.push({ x: PAD, y: body.y + body.height - FOLD_H, width: 96, height: FOLD_H, count: skipped })
}

/** Day `day` from the chart's origin, as a short label: the date when the file names one, else `D+n`. */
function dayLabel(origin: string | undefined, day: number): string {
  const at = origin ? Date.parse(`${origin}T00:00:00Z`) : NaN
  if (Number.isNaN(at)) return `D+${day}`
  return new Date(at + day * 86_400_000).toISOString().slice(0, 10)
}

/** Where day 0 of a Gantt chart sits and how wide one day is. */
type Scale = { x: number; day: number; first: number }

/**
 * Tasks down, time across: each task's card heads its row in file order and
 * its bar runs right of the cards. The day width fits every task's span
 * into the frame, so it changes only with the file's dates or the frame's
 * size, never with state; an add not yet written has no bar yet.
 */
function gantt(all: ShownCard[], cards: ShownCard[], origin: string | undefined, body: Rect, out: Placement): Scale {
  const timed = all.flatMap((card) => card.time?.start === undefined ? [] : [card.time])
  const first = Math.min(0, ...timed.map((time) => time.start!))
  const last = Math.max(first + 1, ...timed.map((time) => time.start! + (time.days ?? 0)))
  const x = body.x + CARD_W + CARD_GAP
  const room = Math.max(0, body.x + body.width - x)
  const day = Math.min(DAY_MAX, room / (last - first))
  cards.forEach((card, index) => {
    out.cards.push({ card, pinned: false, width: CARD_W, height: CARD_H, x: body.x, y: body.y + index * (CARD_H + CARD_GAP) })
  })
  const every = Math.max(1, Math.ceil(MARK_GAP / Math.max(day, 1e-6)))
  for (let at = first; at <= last; at += every) out.marks.push({ label: dayLabel(origin, at), x: x + (at - first) * day, y: body.y - MARK + 4 })
  return { x, day, first }
}

/** After pins: a bar on each placed task's row, from its start day for its length; a milestone is a point. */
function bars(scale: Scale, out: Placement) {
  for (const placed of out.cards) {
    const time = placed.card.time
    if (time?.start === undefined) continue
    const x = scale.x + (time.start - scale.first) * scale.day
    const width = time.milestone ? 0 : Math.max(2, (time.days ?? 0) * scale.day)
    out.bars.push({ id: placed.card.id, x, y: placed.y, width, height: placed.height, completed: time.completed, milestone: !!time.milestone })
  }
}

/**
 * A line's two ends: a Gantt dependency runs from the end of one bar to the
 * start of the next; any other line joins the cards' centers.
 */
export function lineEnds(placement: Placement, line: Line): { x1: number; y1: number; x2: number; y2: number } | null {
  const bar = (id: string) => placement.bars.find((entry) => entry.id === id)
  const box = (id: string) => placement.cards.find((entry) => entry.card.id === id)
  const from = bar(line.from)
  const to = bar(line.to)
  if (line.kind === 'link' && from && to) return { x1: from.x + from.width, y1: from.y + from.height / 2, x2: to.x, y2: to.y + to.height / 2 }
  const a = box(line.from)
  const b = box(line.to)
  return a && b ? { x1: a.x + a.width / 2, y1: a.y + a.height / 2, x2: b.x + b.width / 2, y2: b.y + b.height / 2 } : null
}

function arrival(cards: ShownCard[]): ShownCard[] {
  return cards.map((card, index) => ({ card, index }))
    .sort((a, b) => a.card.seq - b.card.seq || a.index - b.index)
    .map((entry) => entry.card)
}

function timeField(axis: FrameAxis | undefined): string | null {
  return axis && typeof axis === 'object' && 'time' in axis ? axis.time.field : null
}

function dateOf(card: ShownCard, field: string): string | null {
  const value = card.fields[field]
  return typeof value === 'string' && /^\d{4}-\d{2}-\d{2}/.test(value) ? value.slice(0, 10) : null
}

/** Cards fill a rectangle row by row in arrival order; what does not fit folds into one chip at its foot. */
function flow(rect: Rect, cards: ShownCard[], limit: number | null, out: Placement) {
  const columns = Math.max(1, Math.floor((rect.width + CARD_GAP) / (CARD_W + CARD_GAP)))
  const width = Math.max(0, Math.floor((rect.width - (columns - 1) * CARD_GAP) / columns))
  const rows = Math.max(0, Math.floor((rect.height - FOLD_H + CARD_GAP) / (CARD_H + CARD_GAP)))
  const shown = Math.min(limit ?? Infinity, columns * rows)
  cards.forEach((card, index) => {
    if (index >= shown) return
    out.cards.push({ card, pinned: false, width, height: CARD_H,
      x: rect.x + (index % columns) * (width + CARD_GAP), y: rect.y + Math.floor(index / columns) * (CARD_H + CARD_GAP) })
  })
  if (cards.length > shown) out.folds.push({ x: rect.x, y: rect.y + rect.height - FOLD_H, width: rect.width, height: FOLD_H, count: cards.length - shown })
}

function group<T>(cards: ShownCard[], key: (card: ShownCard) => T): Map<string, { key: T; cards: ShownCard[] }> {
  const groups = new Map<string, { key: T; cards: ShownCard[] }>()
  for (const card of cards) {
    const value = key(card)
    const id = JSON.stringify(value)
    const entry = groups.get(id) ?? { key: value, cards: [] }
    entry.cards.push(card)
    groups.set(id, entry)
  }
  return groups
}

/** Category axes: one cell per lane (and row); cards stack in their cell in arrival order. */
function cells(frame: FrameEntry, cards: ShownCard[], width: number, height: number, out: Placement) {
  const limit = perLaneMax(frame)
  for (const { key: [x, y], cards: list } of group(cards, (card) => [laneValue(card, frame.layout.x), laneValue(card, frame.layout.y)]).values()) {
    const rect = cellRect(frame.layout, width, height, x, y)
    flow({ x: rect.left, y: rect.top, width: rect.width, height: rect.height }, list, limit, out)
  }
}

/**
 * A time axis: one slot per date, earliest first; cards without a date take
 * the first slot. A card with a new date goes into its place in time, so
 * cards after it shift along the axis, but no two cards ever change order.
 */
function timeline(frame: FrameEntry, cards: ShownCard[], field: string, vertical: boolean, body: Rect, size: { width: number; height: number }, out: Placement) {
  const dates = [...new Set(cards.map((card) => dateOf(card, field)))]
    .sort((a, b) => (a === null ? -1 : b === null ? 1 : a.localeCompare(b)))
  const other = vertical ? frame.layout.x : frame.layout.y
  const drawn = frameLanes(frame.layout, size.width, size.height)
  const lanes: Lane[] = vertical ? drawn.columns : drawn.rows
  const limit = perLaneMax(frame)
  const step = vertical ? CARD_H + CARD_GAP : CARD_W + CARD_GAP
  for (const { key: [date, lane], cards: list } of group(cards, (card) => [dateOf(card, field), laneValue(card, other)]).values()) {
    const slot = dates.indexOf(date)
    const band = lanes.find((entry) => entry.label === lane)
    if (vertical) {
      const left = (band ? band.start : body.x) + MARK * 3
      const right = band ? band.start + band.size - PAD : body.x + body.width
      flowRow({ x: left, y: body.y + slot * step, width: Math.max(right - left, 0), height: CARD_H + FOLD_H }, list, limit, out)
    } else {
      const top = band ? band.start + MARK : body.y
      const bottom = band ? band.start + band.size - PAD : body.y + body.height
      flow({ x: body.x + slot * step, y: top, width: CARD_W, height: Math.max(bottom - top, 0) }, list, limit, out)
    }
  }
  dates.forEach((date, slot) => out.marks.push(vertical
    ? { label: date ?? '无日期', x: body.x, y: body.y + slot * step }
    : { label: date ?? '无日期', x: body.x + slot * step, y: body.y - MARK + 4 }))
}

/** A time axis running down: cards of one date sit side by side. */
function flowRow(rect: Rect, cards: ShownCard[], limit: number | null, out: Placement) {
  const fits = Math.max(0, Math.floor((rect.width - 40 + CARD_GAP) / (CARD_W + CARD_GAP)))
  const shown = Math.min(limit ?? Infinity, fits)
  cards.forEach((card, index) => {
    if (index < shown) out.cards.push({ card, pinned: false, width: CARD_W, height: CARD_H, x: rect.x + index * (CARD_W + CARD_GAP), y: rect.y })
  })
  if (cards.length > shown) out.folds.push({ x: rect.x + shown * (CARD_W + CARD_GAP), y: rect.y, width: 40, height: CARD_H, count: cards.length - shown })
}

/**
 * Nesting and trees: one column per depth. A card takes the first free row
 * at or below its parent and below everything already in its column and
 * the deeper ones, so a new card opens a row of its own and never pushes
 * an existing one. New cards start below any pinned card.
 */
function tree(cards: ShownCard[], pins: Record<string, Pin>, body: Rect, out: Placement) {
  const rowH = CARD_H + CARD_GAP
  const colW = CARD_W + BRANCH
  const pinned = Object.entries(pins)
  const floor = pinned.length ? Math.max(0, ...pinned.map(([, pin]) => Math.ceil((pin.y + CARD_H + CARD_GAP - body.y) / rowH))) : 0
  const next: number[] = []
  const depthOf = new Map<string, number>()
  const rowOf = new Map<string, number>()
  for (const [id, pin] of pinned) depthOf.set(id, Math.max(0, Math.round((pin.x - body.x) / colW)))
  for (const card of cards) {
    const parent = card.parent ?? null
    const depth = parent !== null && depthOf.has(parent) ? depthOf.get(parent)! + 1 : 0
    let row = Math.max(floor, parent !== null ? rowOf.get(parent) ?? 0 : 0)
    for (let column = depth; column < next.length; column += 1) row = Math.max(row, next[column] ?? 0)
    next[depth] = row + 1
    depthOf.set(card.id, depth)
    rowOf.set(card.id, row)
    out.cards.push({ card, pinned: false, width: CARD_W, height: CARD_H, x: body.x + depth * colW, y: body.y + row * rowH })
  }
}

/**
 * FR-149: every shown card's place in a frame of this size, in frame
 * coordinates. `pins` are the person's `card.pinned` positions.
 */
export function placeCards(frame: FrameEntry, cards: ShownCard[], pins: Record<string, Pin> = {},
  size: { width: number; height: number } = frame.place): Placement {
  const out: Placement = { cards: [], folds: [], lines: [], marks: [], outside: [], messages: [], lifelines: [], bars: [] }
  const { width, height } = size
  const ordered = arrival(cards)
  const fixed = (card: ShownCard) => card.kind !== 'proposed' && !!pins[card.id]
  const auto = ordered.filter((card) => !fixed(card))
  const usedPins = Object.fromEntries(ordered.filter(fixed).map((card) => [card.id, pins[card.id]]))
  // FR-172: a sequence diagram or a Gantt chart brings its own layout model; the template's structure is for the rest.
  const model = sourceModel(frame)
  const structure = frame.layout.structure ?? []
  const nested = !model && (structure.includes('tree') || structure.includes('nesting'))
  const xTime = timeField(frame.layout.x)
  const yTime = timeField(frame.layout.y)
  const categories = laneAxis(frame.layout.x) || laneAxis(frame.layout.y)
  const body = { x: PAD, y: FRAME_HEAD + PAD, width: width - 2 * PAD, height: height - FRAME_HEAD - PAD - FOOT }
  const timed = { x: PAD, y: FRAME_HEAD + MARK, width: width - 2 * PAD, height: height - FRAME_HEAD - MARK - FOOT }
  let scale: Scale | null = null
  if (model === 'sequence') sequence(auto, body, out)
  else if (model === 'gantt') scale = gantt(ordered, auto, frame.source?.diagrams?.find((entry) => entry.kind === 'gantt')?.origin, timed, out)
  else if (nested) tree(auto, usedPins, body, out)
  else if (xTime || yTime) timeline(frame, auto, (xTime ?? yTime)!, !xTime, timed, size, out)
  else if (categories) cells(frame, auto, width, height, out)
  else flow(body, auto, perLaneMax(frame), out)
  // What runs past the frame folds into one chip; nothing shown moves for it.
  const inside = (rect: Rect) => rect.x >= 0 && rect.y >= FRAME_HEAD && rect.x + rect.width <= width && rect.y + rect.height <= height - FOOT + CARD_GAP
  const outside = out.cards.filter((placed) => !inside(placed))
  if (outside.length) {
    out.cards = out.cards.filter(inside)
    out.folds.push({ x: width - 64 - PAD, y: height - FOOT - FOLD_H, width: 64, height: FOLD_H, count: outside.length })
  }
  for (const card of ordered.filter(fixed)) out.cards.push({ card, pinned: true, x: pins[card.id].x, y: pins[card.id].y, width: CARD_W, height: CARD_H })
  // Messages are drawn as message rows, not as relation lines between cards.
  if (model === 'sequence') {
    messages(frame.source?.links ?? [], width, body, out)
    return out
  }
  if (scale) bars(scale, out)
  const shown = new Set(out.cards.map((placed) => placed.card.id))
  for (const { card } of out.cards) {
    if (nested && card.parent && shown.has(card.parent)) out.lines.push({ from: card.parent, to: card.id, kind: 'parent' })
    for (const to of card.links) {
      if (shown.has(to)) out.lines.push({ from: card.id, to, kind: 'link' })
      else if (frameOfCard(to) !== frame.id) out.outside.push({ from: card.id, to })
    }
  }
  return out
}

function laneAxis(axis: FrameAxis | undefined): boolean {
  return !!axis && typeof axis === 'object' && 'category' in axis
}

/** `<frame>.<n>` names its frame; anything else is a core object. */
export function frameOfCard(id: string): string | null {
  const dot = id.lastIndexOf('.')
  return dot > 0 && /^\d+$/.test(id.slice(dot + 1)) ? id.slice(0, dot) : null
}

/** A card's place on the canvas, for lines that cross frames. */
export function canvasPoint(frame: FrameEntry, placement: Placement, card: string): { x: number; y: number } | null {
  const placed = placement.cards.find((entry) => entry.card.id === card)
  return placed ? { x: frame.place.x + placed.x + placed.width / 2, y: frame.place.y + placed.y + placed.height / 2 } : null
}

/**
 * FR-141: lines between cards in different frames, only for the cards in
 * `focus` (hovered or clicked), at either end. Canvas coordinates.
 */
export function crossLines(frames: FrameEntry[], placements: Map<string, Placement>, focus: Iterable<string>):
  { id: string; from: { x: number; y: number }; to: { x: number; y: number } }[] {
  const wanted = new Set(focus)
  if (!wanted.size) return []
  const byId = new Map(frames.map((frame) => [frame.id, frame]))
  const lines: { id: string; from: { x: number; y: number }; to: { x: number; y: number } }[] = []
  for (const frame of frames) {
    const placement = placements.get(frame.id)
    for (const link of placement?.outside ?? []) {
      if (!wanted.has(link.from) && !wanted.has(link.to)) continue
      const other = frameOfCard(link.to)
      const target = other ? byId.get(other) : undefined
      const otherPlacement = other ? placements.get(other) : undefined
      const from = canvasPoint(frame, placement!, link.from)
      const to = target && otherPlacement ? canvasPoint(target, otherPlacement, link.to) : null
      if (from && to) lines.push({ id: `${link.from}->${link.to}`, from, to })
    }
  }
  return lines
}
