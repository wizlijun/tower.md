import { describe, expect, it } from 'vitest'
import type { FrameEntry, FrameLayout } from './api'
import type { SignalEntry } from './attention'
import { blankSpot, defaultSize, frameBadges, frameLanes, framesFrom, frameTitleError, layoutLabel, newFrameId, rulesTextError, toFrameNodes, type FrameNode } from './frames'

const kanban: FrameLayout = { x: { category: { field: 'state', values: ['待办', '进行中', '待评审', '完成'] } }, y: 'order', structure: [] }
const quadrant: FrameLayout = { x: { category: { field: 'urgency', values: ['紧急', '不紧急'] } }, y: { category: { field: 'importance', values: ['重要', '不重要'] } }, structure: [] }
const timeline: FrameLayout = { x: { time: { field: 'date' } }, y: 'none', structure: [] }
const mindmap: FrameLayout = { x: 'none', y: 'none', structure: ['tree'] }

function frame(id: string, seq: number, place = { x: 0, y: 0, width: 880, height: 420 }): FrameEntry {
  return {
    id, title: `容器 ${id}`, template: { id: 'kanban', version: '1.0.0', definition: {} }, layout: kanban,
    states: ['待办', '完成'], done_states: ['完成'], limits: { title_max: 30, body_max: 100 }, set_by: 'human', mode: 'free',
    rules: { text: '', add_card: 'anyone', add_link: 'anyone', edit_text: 'anyone', change_state: 'anyone', fields: [], transitions: {}, require_source: false, done_needs_human: true },
    place, created: { at: '2026-10-23T00:00:00Z', eid: `e-${id}`, seq, actor: 'human:local' },
  }
}

function signal(target: string, level: SignalEntry['data']['level'], title: string, quiet = false): SignalEntry {
  return {
    from: { id: `cron/${title}`, kind: 'source' }, target, count: 1, data: { kind: 'eng.backlog', level, title },
    last: { at: '2026-10-23T00:00:00Z', eid: `s-${title}`, seq: 9, actor: `cron/${title}` },
    acknowledged: quiet ? { at: '2026-10-23T00:00:00Z', eid: 'a', actor: 'human:local' } : null,
  }
}

describe('frames', () => {
  it('reads frames from the projection oldest first and skips malformed ones', () => {
    const projection = { Frame: { 'C-b': frame('C-b', 7), 'C-a': frame('C-a', 3), 'C-bad': { id: 'C-bad', title: 'x', place: { x: 0.5 } } } }
    expect(framesFrom(projection).map((entry) => entry.id)).toEqual(['C-a', 'C-b'])
    expect(framesFrom({})).toEqual([])
    expect(framesFrom(null)).toEqual([])
  })

  it('sizes a new frame to its template and draws lanes that tile it exactly', () => {
    expect(defaultSize(kanban)).toEqual({ width: 880, height: 420 })
    expect(defaultSize(quadrant)).toEqual({ width: 640, height: 524 })
    expect(defaultSize(timeline)).toEqual({ width: 960, height: 320 })
    expect(defaultSize(mindmap)).toEqual({ width: 720, height: 480 })
    const lanes = frameLanes(kanban, 1001, 420)
    expect(lanes.columns.map((lane) => lane.label)).toEqual(['待办', '进行中', '待评审', '完成'])
    expect(lanes.columns.reduce((sum, lane) => sum + lane.size, 0)).toBe(1001)
    expect(lanes.columns.at(-1)!.start + lanes.columns.at(-1)!.size).toBe(1001)
    expect(lanes.rows).toEqual([])
    const cells = frameLanes(quadrant, 640, 524)
    expect(cells.rows).toEqual([{ label: '重要', start: 44, size: 240 }, { label: '不重要', start: 284, size: 240 }])
    expect(frameLanes(mindmap, 720, 480)).toEqual({ columns: [], rows: [] })
  })

  it('describes a layout in words', () => {
    expect(layoutLabel(kanban)).toBe('横向按状态分栏，纵向按添加顺序')
    expect(layoutLabel(quadrant)).toBe('横向按urgency分栏，纵向按importance分栏')
    expect(layoutLabel(timeline)).toBe('横向按时间（date）')
    expect(layoutLabel(mindmap)).toBe('树状展开')
  })

  it('places a new frame in the nearest blank spot without covering any other', () => {
    const size = { width: 400, height: 300 }
    expect(blankSpot({ x: 0, y: 0 }, size, [])).toEqual({ x: -200, y: -150, width: 400, height: 300 })
    const taken = [{ x: -300, y: -300, width: 600, height: 600 }]
    const spot = blankSpot({ x: 0, y: 0 }, size, taken)
    const clear = (a: typeof spot, b: typeof spot) => a.x + a.width <= b.x || b.x + b.width <= a.x || a.y + a.height <= b.y || b.y + b.height <= a.y
    expect(clear(spot, taken[0])).toBe(true)
    expect(taken).toEqual([{ x: -300, y: -300, width: 600, height: 600 }])
    expect(Number.isSafeInteger(spot.x) && Number.isSafeInteger(spot.y)).toBe(true)
  })

  it('badges a frame with its open signals, never with logs or quieted ones', () => {
    const frames = [frame('C-a', 1), frame('C-b', 2)]
    const signals = [signal('C-a', 'passive', '栏位已满'), signal('C-b', 'log', '记录'), signal('C-b', 'active', '已读', true), signal('T-1', 'active', '别处')]
    expect(frameBadges(signals, frames, Date.parse('2026-10-24T00:00:00Z'))).toEqual({ 'C-a': { count: 1, label: '栏位已满', tone: 'info' } })
    const two = frameBadges([signal('C-a', 'passive', '一'), signal('C-a', 'time_sensitive', '二')], frames, 0)
    expect(two['C-a']).toEqual({ count: 2, label: '2 条提醒', tone: 'warn' })
  })

  it('makes fresh C- IDs and checks what the person typed', () => {
    let calls = 0
    const id = newFrameId(['C-00000000'], () => (calls++ < 8 ? 0 : 0.5))
    expect(id).toMatch(/^C-[0-9a-z]{8}$/)
    expect(id).not.toBe('C-00000000')
    expect(frameTitleError(' ')).toBe('给容器起个名字。')
    expect(frameTitleError('读\n论文')).toBe('名字里不能有换行或控制字符。')
    expect(frameTitleError('读'.repeat(61))).toBe('名字最多 60 个字。')
    expect(frameTitleError('读论文')).toBeNull()
    expect(rulesTextError('规'.repeat(301))).toBe('规则最多 300 个字。')
    expect(rulesTextError('')).toBeNull()
  })

  it('keeps a frame node while nothing about it changed, so a signal never resets a drag', () => {
    const cache = new Map<string, FrameNode>()
    const onPlace = () => {}
    const [first] = toFrameNodes([frame('C-a', 1)], {}, true, onPlace, cache)
    expect(first).toMatchObject({ id: 'frame:C-a', type: 'frame', position: { x: 0, y: 0 }, width: 880, height: 420, draggable: true, zIndex: -2 })
    expect(toFrameNodes([frame('C-a', 1)], {}, true, onPlace, cache)[0]).toBe(first)
    const [badged] = toFrameNodes([frame('C-a', 1)], { 'C-a': { count: 1, label: '满', tone: 'info' } }, true, onPlace, cache)
    expect(badged).not.toBe(first)
    expect(badged.position).toEqual({ x: 0, y: 0 })
    const [moved] = toFrameNodes([frame('C-a', 1, { x: 40, y: 0, width: 880, height: 420 })], {}, false, onPlace, cache)
    expect(moved.draggable).toBe(false)
    expect(moved.position).toEqual({ x: 40, y: 0 })
    toFrameNodes([], {}, false, onPlace, cache)
    expect(cache.size).toBe(0)
  })
})
