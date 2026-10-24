import { describe, expect, it } from 'vitest'
import type { FrameCard, FrameEntry, FrameLayout, FrameSourceView, FrameStep, FrameTemplate } from './api'
import type { SignalEntry } from './attention'
import { blankSpot, calloutsFor, cellRect, changesSince, pastFrame, rangeStart, defaultSize, frameBadges, frameLanes, framesFrom, frameChange, frameTitleError, laneValue, layoutLabel, newFrameId, rulesTextError, shownCards, sourceChangeText, sourcePathError, sourceStatus, toFrameNodes, withPins, withUntil, type FrameNode, flowChart, offersFlow, FLOW_SLOTS } from './frames'

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

  describe('cards (H2)', () => {
    const mark = (seq: number) => ({ at: '2026-10-24T00:00:00Z', eid: `e-${seq}`, seq, actor: 'agent/1' })
    const card = (n: number, state: string, extra: Partial<FrameCard> = {}): FrameCard =>
      ({ id: `C-a.${n}`, title: `卡片 ${n}`, state, added: mark(10 + n), changed: mark(10 + n), ...extra })
    const board = (): FrameEntry => ({ ...frame('C-a', 1), states: ['待办', '进行中', '待评审', '完成'] })

    it('shows bound cards, then free ones in arrival order, then proposals; archived ones are gone', () => {
      const entry: FrameEntry = { ...board(), mode: 'mixed',
        cards: [card(1, '待办'), card(2, '完成', { archived: true }), card(3, '完成')],
        bound: [{ id: 'T-1', title: '任务', state: '进行中', phase: 'running', done: false }],
        pending: [
          { id: 40, why: 'proposal', target: 'C-a', op: { op: 'add', title: '提的卡片' }, from: 'agent-a', by: mark(40) },
          { id: 41, why: 'suggestion', target: 'C-a.1', op: { op: 'update', title: '新标题' }, from: 'agent-a', by: mark(41) },
        ] }
      const shown = shownCards(entry)
      expect(shown.map((item) => [item.id, item.kind])).toEqual([['T-1', 'bound'], ['C-a.1', 'free'], ['C-a.3', 'free'], ['pending:40', 'proposed']])
      expect(shown[1].waiting.map((item) => item.why)).toEqual(['suggestion'])
      expect(shown[2].done).toBe(true)
      expect(shown[3].state).toBe('待办')
    })

    it('reads a lane from the state or a field, and nothing from other axes', () => {
      const item = { state: '完成', fields: { urgency: '紧急', importance: 'x' } }
      expect(laneValue(item, kanban.x)).toBe('完成')
      expect(laneValue(item, quadrant.x)).toBe('紧急')
      expect(laneValue(item, quadrant.y)).toBeNull()
      expect(laneValue(item, timeline.x)).toBeNull()
      expect(laneValue(item, 'order')).toBeNull()
    })

    it('puts a cell under its lane label and above the rules line', () => {
      expect(cellRect(kanban, 880, 420, '进行中', null)).toEqual({ left: 228, top: 68, width: 204, height: 324 })
      expect(cellRect(quadrant, 640, 524, '紧急', '不重要')).toEqual({ left: 8, top: 308, width: 304, height: 216 })
      expect(cellRect(mindmap, 720, 480, null, null)).toEqual({ left: 8, top: 68, width: 704, height: 384 })
    })

    it('sets and clears update_until without touching the other rules', () => {
      const rules = board().rules
      expect(withUntil(rules, '2026-11-01')).toEqual({ ...rules, update_until: { date: '2026-11-01' } })
      expect(withUntil({ ...rules, update_until: { object: 'T-1' } }, null)).toEqual(rules)
    })

    it('adds the daemon view to the projection and rebuilds a frame node only when its content changes', () => {
      const entry = { ...board(), cards: [card(1, '待办')] }
      const merged = framesFrom({ Frame: { 'C-a': entry } }, [{ ...entry, bound: [], frozen: true }])
      expect(merged[0].frozen).toBe(true)
      expect(framesFrom({ Frame: { 'C-a': entry } }, null)[0].frozen).toBeUndefined()
      const cache = new Map<string, FrameNode>()
      const actions = {}
      const [first] = toFrameNodes([entry], {}, true, undefined, cache, actions)
      expect(toFrameNodes([{ ...entry, cards: [card(1, '待办')] }], {}, true, undefined, cache, actions)[0]).toBe(first)
      const [moved] = toFrameNodes([{ ...entry, cards: [card(1, '完成')] }], {}, true, undefined, cache, actions)
      expect(moved).not.toBe(first)
      expect(moved.position).toEqual(first.position)
      expect(moved.data.actions).toBe(actions)
    })
  })

  describe('layout (H3)', () => {
    const card = (n: number, at = '2026-10-24T00:00:00Z'): FrameCard =>
      ({ id: `C-a.${n}`, title: `${n}`, added: { at, eid: `c${n}`, seq: 10 + n, actor: 'agent/1' }, changed: { at, eid: `c${n}`, seq: 10 + n, actor: 'agent/1' } })
    it('merges content updates into one visual change, but shows moves, titles and pins at once (FR-151)', () => {
      const a = { ...frame('C-a', 1), cards: [card(1)] }
      expect(frameChange([a], [structuredClone(a)])).toBe('same')
      expect(frameChange([a], [{ ...a, cards: [card(1), card(2)] }])).toBe('later')
      expect(frameChange([a], [{ ...a, rules: { ...a.rules, text: '新规则' } }])).toBe('later')
      expect(frameChange([a], [{ ...a, place: { ...a.place, x: 5 } }])).toBe('now')
      expect(frameChange([a], [{ ...a, title: '改名' }])).toBe('now')
      expect(frameChange([a], [{ ...a, pins: { 'C-a.1': { x: 1, y: 2 } } }])).toBe('now')
      expect(frameChange([a], [a, frame('C-b', 2)])).toBe('now')
      expect(frameChange([], [])).toBe('same')
    })

    it('shows sent pins until the live state has them, and unpins with null', () => {
      const a = { ...frame('C-a', 1), pins: { 'C-a.1': { x: 1, y: 2 } } }
      expect(withPins(a, { 'C-a.1': null, 'C-a.2': { x: 3, y: 4 } }).pins).toEqual({ 'C-a.2': { x: 3, y: 4 } })
      expect(a.pins).toEqual({ 'C-a.1': { x: 1, y: 2 } })
    })

    it('rebuilds a frame node for new pins or a new visit, and carries when each card changed', () => {
      const cache = new Map<string, FrameNode>()
      const a = { ...frame('C-a', 1), cards: [card(1, '2026-10-24T08:00:00Z')] }
      const [first] = toFrameNodes([a], {}, true, undefined, cache, undefined, { since: 1 })
      expect(toFrameNodes([a], {}, true, undefined, cache, undefined, { since: 1 })[0]).toBe(first)
      expect(toFrameNodes([a], {}, true, undefined, cache, undefined, { since: 2 })[0]).not.toBe(first)
      const pinned = toFrameNodes([{ ...a, pins: { 'C-a.1': { x: 0, y: 0 } } }], {}, true, undefined, cache, undefined, { since: 2 })[0]
      expect(pinned.data.frame.pins).toEqual({ 'C-a.1': { x: 0, y: 0 } })
      expect(shownCards(a)[0].changed).toBe('2026-10-24T08:00:00Z')
    })
  })

  describe('callouts and history (FR-157/158, FR-160/161)', () => {
    it('groups callouts by what they point at and hides the kinds the person hid', () => {
      const a: FrameEntry = { ...frame('C-a', 1), callouts: [
        { kind: 'why', target: 'C-a.1', text: '因为决策 D-1', source: 'Q-1' },
        { kind: 'question', target: 'C-a.1', text: '要不要拆？', source: 'Q-2' },
        { kind: 'adjustment', target: 'C-a', text: '先做读者调研', source: 'P-1' },
      ] }
      expect([...calloutsFor(a).entries()].map(([target, list]) => [target, list.map((c) => c.kind)]))
        .toEqual([['C-a.1', ['why', 'question']], ['C-a', ['adjustment']]])
      expect(calloutsFor(a, ['question', 'adjustment']).get('C-a.1')?.map((c) => c.kind)).toEqual(['why'])
      expect(calloutsFor(a, ['adjustment']).has('C-a')).toBe(false)
    })

    it('starts each range where the person expects, in local time', () => {
      const now = new Date(2026, 9, 24, 15, 30).getTime() // a Saturday
      expect(rangeStart('visit', now, 123)).toBe(123)
      expect(rangeStart('visit', now)).toBeNull()
      expect(rangeStart('today', now)).toBe(new Date(2026, 9, 24).getTime())
      expect(rangeStart('week', now)).toBe(new Date(2026, 9, 19).getTime())
      expect(rangeStart('date', now, undefined, '2026-10-01')).toBe(new Date(2026, 9, 1).getTime())
      expect(rangeStart('date', now, undefined, '10/01')).toBeNull()
    })

    it('merges what each step changed after the start, once per kind', () => {
      const steps: FrameStep[] = [
        { seq: 2, at: '2026-10-20T00:00:00Z', kind: 'card.added', actor: 'human:local', cards: { 'C-a.1': ['new'] } },
        { seq: 5, at: '2026-10-23T00:00:00Z', kind: 'card.updated', actor: 'claude-code/2', cards: { 'C-a.1': ['text'], 'C-a.2': ['new'] } },
        { seq: 6, at: '2026-10-24T00:00:00Z', kind: 'card.moved', actor: 'claude-code/2', cards: { 'C-a.1': ['state', 'text'] } },
        { seq: 7, at: '2026-10-24T01:00:00Z', kind: 'frame.pinned', actor: 'human:local', cards: {} },
      ]
      expect(changesSince(steps, Date.parse('2026-10-22T00:00:00Z'))).toEqual({ 'C-a.1': ['state', 'text'], 'C-a.2': ['new'] })
      expect(changesSince(steps, 0)['C-a.1']).toEqual(['new', 'state', 'text'])
      expect(changesSince(steps, Date.parse('2026-10-25T00:00:00Z'))).toEqual({})
    })

    it('shows a past step where the live frame is, so replay moves nothing', () => {
      const live = frame('C-a', 1, { x: 500, y: 600, width: 900, height: 400 })
      const then = frame('C-a', 1)
      const at = { frame: 'C-a', seq: 3, step: { seq: 3, at: '2026-10-23T00:00:00Z', kind: 'card.added', actor: 'human:local', cards: {} }, view: [then] }
      expect(pastFrame(live, at)?.place).toEqual(live.place)
      expect(pastFrame(frame('C-b', 1), at)).toBeNull()
    })

    it('rebuilds a frame node when its callouts change', () => {
      const cache = new Map<string, FrameNode>()
      const a = frame('C-a', 1)
      const [first] = toFrameNodes([a], {}, false, undefined, cache)
      const [same] = toFrameNodes([{ ...a }], {}, false, undefined, cache)
      expect(same).toBe(first)
      const [next] = toFrameNodes([{ ...a, callouts: [{ kind: 'why', target: 'C-a', text: 'x', source: 'Q-1' }] }], {}, false, undefined, cache)
      expect(next).not.toBe(first)
      expect(next.position).toEqual(first.position)
    })
  })

  describe('text-source frames (FR-163–177)', () => {
    const parsed: FrameSourceView = {
      path: 'docs/plan.puml', status: 'ok', revision: 'a'.repeat(40), locked: false, understood: 80, unknown_lines: 1,
      cards: [
        { key: 'P', title: 'Parser', keyword: 'component', state: 'To Do', done: false, links: ['W'], line: 2, diagram: 0 },
        { key: 'W', title: 'Writer', keyword: 'component', parent: 'P', done: true, state: 'Done', line: 3, diagram: 0 },
        { key: 'M', title: 'Macro', keyword: 'component', done: false, line: 4, diagram: 0, read_only: '由宏生成' },
      ],
      callouts: [{ kind: 'adjust', target: 'P', text: '先拆开', full: '先拆开', line: 5 }, { kind: 'why', text: '整体', full: '整体', line: 6 }],
    }
    const sourced = (extra: Partial<FrameEntry> = {}): FrameEntry =>
      ({ ...frame('C-plan', 1), states: ['To Do', 'In Progress', 'Done'], done_states: ['Done'], source: { path: 'docs/plan.puml' }, ...extra })

    it('takes the parsed file from the frames view and shows its notes as callouts', () => {
      const [entry] = framesFrom({ Frame: { 'C-plan': sourced() } }, [{ ...sourced(), bound: [], frozen: false, source: parsed }])
      expect(entry.source?.understood).toBe(80)
      expect(entry.callouts).toEqual([
        { kind: 'adjustment', target: 'P', text: '先拆开', source: 'docs/plan.puml:5' },
        { kind: 'why', target: 'C-plan', text: '整体', source: 'docs/plan.puml:6' },
      ])
      const quiet = sourced({ rules: { ...sourced().rules, callouts_off: true } })
      expect(framesFrom({ Frame: { 'C-plan': quiet } }, [{ ...quiet, source: parsed }])[0].callouts).toBeUndefined()
      // A frame without a file never takes one from the view.
      const plain = frame('C-a', 2)
      expect(framesFrom({ Frame: { 'C-a': plain } }, [{ ...plain, source: parsed }])[0].source).toBeUndefined()
    })

    it('shows the file elements as cards, keyed by alias, with queued writes beside them', () => {
      const pending = [
        { id: 7, why: 'write' as const, target: 'C-plan', op: { op: 'source' as const, change: { do: 'state' as const, element: 'P', state: 'Done' } }, from: 'claude-code/2', by: { at: '2026-10-24T00:00:00Z', eid: 'e7', seq: 7, actor: 'claude-code/2' } },
        { id: 8, why: 'write' as const, target: 'C-plan', op: { op: 'source' as const, change: { do: 'add' as const, title: 'Checker' } }, by: { at: '2026-10-24T00:00:00Z', eid: 'e8', seq: 8, actor: 'human:local' }, note: '文件里已有同名元素' },
      ]
      const cards = shownCards(sourced({ source: parsed, pending, cards: [{ id: 'C-plan.1', title: '不会显示', added: { at: '', eid: 'x', seq: 1, actor: 'human:local' }, changed: { at: '', eid: 'x', seq: 1, actor: 'human:local' } }] }))
      expect(cards.map((card) => [card.id, card.kind, card.state ?? null, card.done])).toEqual([
        ['P', 'source', 'To Do', false], ['W', 'source', 'Done', true], ['M', 'source', null, false], ['pending:8', 'proposed', 'To Do', false],
      ])
      expect(cards[0].links).toEqual(['W'])
      expect(cards[0].waiting.map((item) => item.id)).toEqual([7])
      expect(cards[1].parent).toBe('P')
      expect(cards[2].readOnly).toBe('由宏生成')
      expect(cards[3].waiting[0].note).toBe('文件里已有同名元素')
      expect(shownCards(sourced())).toEqual([])
    })

    it('words the file state and each kind of edit', () => {
      expect(sourceStatus(parsed)).toBeNull()
      expect(sourceStatus({ ...parsed, status: 'unparsable' })).toContain('最后一次能解析')
      expect(sourceStatus({ ...parsed, status: 'missing' })).toContain('main 上没有')
      expect(sourceStatus({ path: 'docs/plan.puml', error: 'git 不可用' })).toContain('git 不可用')
      expect(sourceChangeText({ do: 'state', element: 'P', state: 'Done' })).toBe('把 P 改为「Done」')
      expect(sourceChangeText({ do: 'link', from: 'P', to: 'W' })).toBe('连 P → W')
    })

    it('asks for a relative repository path of the template file types', () => {
      const plantuml: FrameTemplate = { id: 'plantuml', origin: 'starter', path: 'starter:plantuml', problems: [], file_types: ['.puml', '.plantuml'] }
      expect(sourcePathError(plantuml, 'docs/plan.puml')).toBeNull()
      expect(sourcePathError(plantuml, 'docs/plan.PlantUML')).toBeNull()
      expect(sourcePathError(plantuml, '')).toBeTruthy()
      expect(sourcePathError(plantuml, '../plan.puml')).toBeTruthy()
      expect(sourcePathError(plantuml, '/tmp/plan.puml')).toBeTruthy()
      expect(sourcePathError(plantuml, 'docs/plan.md')).toContain('.puml')
    })

    it('rebuilds the frame node when the file changes, without moving the frame', () => {
      const cache = new Map<string, FrameNode>()
      const a = sourced({ source: parsed })
      const [first] = toFrameNodes([a], {}, false, undefined, cache)
      const [next] = toFrameNodes([{ ...a, source: { ...parsed, understood: 90 } }], {}, false, undefined, cache)
      expect(next).not.toBe(first)
      expect(next.position).toEqual(first.position)
    })
  })

  it('offers the cumulative flow only when the frozen template declares it and the frame has states', () => {
    const entry = (definition: unknown, states = ['待办', '完成']) => ({ template: { id: 'kanban', version: '1.1.0', definition }, states }) as unknown as FrameEntry
    expect(offersFlow(entry({ views: ['cumulative_flow'] }))).toBe(true)
    expect(offersFlow(entry({}))).toBe(false)
    expect(offersFlow(entry({ views: 'cumulative_flow' }))).toBe(false)
    expect(offersFlow(entry({ views: ['cumulative_flow'] }, []))).toBe(false)
    expect(offersFlow(entry(null))).toBe(false)
  })

  it('stacks the cumulative flow with the last state at the bottom and folds states past the slots', () => {
    const flow = { frame: 'C-a', states: ['待办', '完成'], done_states: ['完成'], days: [
      { date: '2026-10-22', counts: [2, 0] }, { date: '2026-10-23', counts: [1, 1] }] }
    const chart = flowChart(flow, 100, 40)
    expect(chart.max).toBe(2)
    expect(chart.bands.map((band) => [band.name, band.slot, band.latest])).toEqual([['待办', 1, 1], ['完成', 2, 1]])
    // 完成 rises from the baseline; 待办 sits on top of it.
    expect(chart.bands[1].path).toBe('M0,40L100,20L100,40L0,40Z')
    expect(chart.bands[0].path).toBe('M0,0L100,0L100,20L0,40Z')
    const one = flowChart({ ...flow, days: [flow.days[0]] }, 100, 40)
    expect(one.bands[0].path).toBe('M0,0L100,0L100,40L0,40Z')
    const many = Array.from({ length: FLOW_SLOTS + 2 }, (_, index) => `S${index}`)
    const folded = flowChart({ frame: 'C-a', states: many, done_states: [], days: [{ date: '2026-10-23', counts: many.map(() => 1) }] }, 100, 40)
    expect(folded.bands.map((band) => band.name)).toEqual([...many.slice(0, FLOW_SLOTS - 1), '其他'])
    expect(folded.days[0].counts.at(-1)).toBe(3)
    expect(flowChart({ ...flow, days: [] }, 100, 40).bands[0].path).toBe('')
  })
})
