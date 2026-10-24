import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { FrameCard, FrameEntry, FrameFormat, FrameHistory, FrameHistoryAt } from '../lib/api'
import { toFrameNodes, type FrameReader } from '../lib/frames'
import { selectValue } from '../test/select'
import Harness from '../test/FrameNodeHarness.svelte'

afterEach(cleanup)

const mark = (seq: number, at = '2026-10-23T00:00:00Z') => ({ at, eid: `e${seq}`, seq, actor: 'human:local' })
function card(id: string, title: string, state: string, seq: number, at?: string): FrameCard {
  return { id, title, state, added: mark(seq, at), changed: mark(seq, at) }
}
function frame(cards: FrameCard[], extra: Partial<FrameEntry> = {}): FrameEntry {
  return {
    id: 'C-a', title: '读论文', template: { id: 'kanban', version: '1.0.0', definition: {} },
    layout: { x: { category: { field: 'state', values: ['待办', '完成'] } }, y: 'order', structure: [] },
    states: ['待办', '完成'], done_states: ['完成'], limits: { title_max: 30, body_max: 100 }, set_by: 'human', mode: 'free',
    rules: { text: '', add_card: 'anyone', add_link: 'anyone', edit_text: 'anyone', change_state: 'anyone', fields: [], transitions: {}, require_source: false, done_needs_human: true },
    place: { x: 0, y: 0, width: 900, height: 420 }, created: mark(1), cards, ...extra,
  }
}
function cardBox(title: string): HTMLElement {
  return screen.getByRole('group', { name: `卡片 ${title}` })
}

describe('a frame on the canvas (FR-157/158, FR-160/161)', () => {
  it('draws callouts beside their card without moving it, and hides them by kind', async () => {
    const plain = frame([card('C-a.1', '甲', '待办', 2)])
    const [before] = toFrameNodes([plain], {}, false, undefined, new Map())
    render(Harness, { props: { node: before } })
    const place = cardBox('甲').getAttribute('style')
    cleanup()
    const noted = frame([card('C-a.1', '甲', '待办', 2)], { callouts: [
      { kind: 'why', target: 'C-a.1', text: '决策 D-1：先读综述', source: 'Q-1' },
      { kind: 'question', target: 'C-a.1', text: '要不要拆开？', source: 'Q-2' },
    ] })
    const [after] = toFrameNodes([noted], {}, false, undefined, new Map())
    render(Harness, { props: { node: after } })
    expect(cardBox('甲').getAttribute('style')).toBe(place)
    const notes = screen.getByLabelText('卡片 甲 的旁注')
    expect(notes.textContent).toContain('为什么')
    expect(notes.textContent).toContain('决策 D-1：先读综述')
    expect(notes.textContent).toContain('问题')
    await fireEvent.click(screen.getByRole('button', { name: '旁注' }))
    await fireEvent.click(within(screen.getByRole('group', { name: '按种类显示旁注' })).getByLabelText(/问题/))
    expect(screen.getByLabelText('卡片 甲 的旁注').textContent).not.toContain('要不要拆开？')
    expect(cardBox('甲').getAttribute('style')).toBe(place)
  })

  it('badges what changed in a range, fades the rest, and replays a past step read-only', async () => {
    const live = frame([card('C-a.1', '甲', '完成', 2, '2026-10-01T00:00:00Z'), card('C-a.2', '乙', '待办', 3, '2026-10-01T00:00:00Z')])
    const history: FrameHistory = { frame: 'C-a', seq: 9, steps: [
      { seq: 2, at: '2026-10-01T00:00:00Z', kind: 'card.added', actor: 'human:local', cards: { 'C-a.1': ['new'] } },
      { seq: 3, at: '2026-10-01T00:00:00Z', kind: 'card.added', actor: 'human:local', cards: { 'C-a.2': ['new'] } },
      { seq: 9, at: new Date().toISOString(), kind: 'card.moved', actor: 'claude-code/2', cards: { 'C-a.1': ['state'] } },
    ] }
    const then: FrameHistoryAt = { frame: 'C-a', seq: 2, step: history.steps[0], view: [frame([card('C-a.1', '甲', '待办', 2)], { place: { x: 5, y: 5, width: 10, height: 10 } })] }
    const reader: FrameReader = { steps: vi.fn().mockResolvedValue(history), at: vi.fn().mockResolvedValue(then) }
    const actions = { card: vi.fn(), pin: vi.fn() }
    const [node] = toFrameNodes([live], {}, true, undefined, new Map(), actions, { history: reader })
    render(Harness, { props: { node } })
    const placeOf = (title: string) => /left: [^;]+; top: [^;]+;/.exec(cardBox(title).getAttribute('style') ?? '')?.[0]
    const before = { a: placeOf('甲'), b: placeOf('乙') }
    await selectValue(screen.getByLabelText('高亮变化的范围'), 'today')
    await waitFor(() => expect(cardBox('甲').textContent).toContain('状态'))
    expect(cardBox('乙').classList.contains('is-faded')).toBe(true)
    expect(cardBox('甲').classList.contains('is-faded')).toBe(false)
    expect({ a: placeOf('甲'), b: placeOf('乙') }).toEqual(before)

    await fireEvent.click(screen.getByRole('button', { name: '历史' }))
    const slider = await screen.findByLabelText('历史上的一步') as HTMLInputElement
    expect(slider.max).toBe('3')
    slider.value = '0'
    await fireEvent.change(slider)
    await screen.findByText(/第 1\/3 步/)
    expect(reader.at).toHaveBeenCalledWith('C-a', 2)
    expect(screen.queryByRole('group', { name: '卡片 乙' })).toBeNull()
    expect(screen.getByText(/历史 · 只读/)).toBeTruthy()
    expect(screen.queryByRole('button', { name: '+ 卡片' })).toBeNull()
    await fireEvent.click(screen.getByRole('button', { name: '回到现在' }))
    expect(cardBox('乙')).toBeTruthy()
    expect(actions.card).not.toHaveBeenCalled()
    expect(actions.pin).not.toHaveBeenCalled()
  })

  it('reads the frame as text on request, switches format, and moves nothing', async () => {
    const live = frame([card('C-a.1', '甲', '待办', 2)])
    const text = vi.fn(async (_frame: string, format?: FrameFormat) => format === 'markdown'
      ? { frame: 'C-a', format: 'markdown' as const, formats: ['mermaid', 'markdown'] as FrameFormat[], text: '# 读论文\n\n## 待办\n\n- 甲\n' }
      : { frame: 'C-a', format: 'mermaid' as const, formats: ['mermaid', 'markdown'] as FrameFormat[], text: 'kanban\n  lane1[待办]\n    n1[甲]\n' })
    const reader: FrameReader = { steps: vi.fn(), at: vi.fn(), text }
    const actions = { card: vi.fn(), pin: vi.fn() }
    const [node] = toFrameNodes([live], {}, true, undefined, new Map(), actions, { history: reader })
    render(Harness, { props: { node } })
    const before = cardBox('甲').getAttribute('style')
    await fireEvent.click(screen.getByRole('button', { name: '导出' }))
    const area = await screen.findByLabelText('导出的文本') as HTMLTextAreaElement
    expect(area.readOnly).toBe(true)
    expect(area.value).toContain('kanban')
    expect(text).toHaveBeenCalledWith('C-a', undefined)
    await selectValue(screen.getByLabelText('导出格式'), 'markdown')
    await waitFor(() => expect((screen.getByLabelText('导出的文本') as HTMLTextAreaElement).value).toContain('## 待办'))
    expect(text).toHaveBeenLastCalledWith('C-a', 'markdown')
    expect(cardBox('甲').getAttribute('style')).toBe(before)
    await fireEvent.click(screen.getByRole('button', { name: '关闭' }))
    expect(screen.queryByLabelText('导出的文本')).toBeNull()
    expect(actions.card).not.toHaveBeenCalled()
    expect(reader.steps).not.toHaveBeenCalled()
  })

  it('opens the cumulative flow in its corner on request, names every band in text, and moves nothing', async () => {
    const live = frame([card('C-a.1', '甲', '待办', 2)], { template: { id: 'kanban', version: '1.1.0', definition: { views: ['cumulative_flow'] } } })
    const flow = vi.fn(async () => ({ view: 'cumulative_flow' as const, flow: { frame: 'C-a', states: ['待办', '完成'], done_states: ['完成'],
      days: [{ date: '2026-10-22', counts: [2, 0] }, { date: '2026-10-23', counts: [1, 1] }] } }))
    const reader: FrameReader = { steps: vi.fn(), at: vi.fn(), flow }
    const actions = { card: vi.fn(), pin: vi.fn() }
    const [node] = toFrameNodes([live], {}, true, undefined, new Map(), actions, { history: reader })
    render(Harness, { props: { node } })
    const before = cardBox('甲').getAttribute('style')
    await fireEvent.click(screen.getByRole('button', { name: '累积流' }))
    expect(await screen.findByRole('img', { name: /累积流图：2026-10-22 至 2026-10-23，共 2 天/ })).toBeTruthy()
    expect(flow).toHaveBeenCalledWith('C-a')
    expect(screen.getByText('待办 1')).toBeTruthy()
    expect(screen.getByText('完成 1')).toBeTruthy()
    expect(cardBox('甲').getAttribute('style')).toBe(before)
    await fireEvent.click(screen.getByRole('button', { name: '关闭' }))
    expect(screen.queryByRole('img', { name: /累积流图/ })).toBeNull()
    expect(actions.card).not.toHaveBeenCalled()
    expect(reader.steps).not.toHaveBeenCalled()
  })

  it('offers no cumulative flow when the template does not declare it', () => {
    const reader: FrameReader = { steps: vi.fn(), at: vi.fn(), flow: vi.fn() }
    const [node] = toFrameNodes([frame([])], {}, true, undefined, new Map(), {}, { history: reader })
    render(Harness, { props: { node } })
    expect(screen.queryByRole('button', { name: '累积流' })).toBeNull()
  })
})

describe('a text-source frame (FR-163–178)', () => {
  const source = {
    path: 'docs/plan.puml', status: 'ok' as const, locked: false, understood: 75, unknown_lines: 1, sha256: 'f'.repeat(64),
    unknown: [{ line: 9, text: 'Alice -> Bob', reason: '时序图的消息不在画布模型里' }],
    cards: [
      { key: 'P', title: 'Parser', keyword: 'component', state: 'To Do', done: false, line: 2, diagram: 0 },
      { key: 'M', title: 'Macro', keyword: 'component', done: false, line: 3, diagram: 0, read_only: '由宏生成' },
    ],
  }
  const sourced = (extra: Partial<FrameEntry> = {}) => frame([], {
    id: 'C-plan', layout: { x: 'none', y: 'none', structure: ['nesting', 'relations'] }, states: ['To Do', 'Done'], done_states: ['Done'], source, ...extra,
  })

  it('writes state changes, names and new elements to the file, and keeps read-only elements closed', async () => {
    const actions = { card: vi.fn(), pin: vi.fn() }
    const [node] = toFrameNodes([sourced()], {}, true, undefined, new Map(), actions)
    render(Harness, { props: { node } })
    expect(screen.getByText(/文件 docs\/plan.puml · 理解 75%/)).toBeTruthy()
    await fireEvent.click(within(cardBox('Parser')).getByRole('button', { name: 'Parser' }))
    await selectValue(within(cardBox('Parser')).getByLabelText('状态'), 'Done')
    expect(actions.card).toHaveBeenLastCalledWith('C-plan', 'C-plan', { op: 'source', change: { do: 'state', element: 'P', state: 'Done' } })
    await fireEvent.click(within(cardBox('Parser')).getByRole('button', { name: 'Parser' }))
    await fireEvent.input(within(cardBox('Parser')).getByLabelText('新名称'), { target: { value: 'Reader' } })
    await fireEvent.click(within(cardBox('Parser')).getByRole('button', { name: '改名' }))
    expect(actions.card).toHaveBeenLastCalledWith('C-plan', 'C-plan', { op: 'source', change: { do: 'title', element: 'P', title: 'Reader' } })
    expect(cardBox('Macro').textContent).toContain('只读：由宏生成')
    await fireEvent.click(within(cardBox('Macro')).getByRole('button', { name: 'Macro' }))
    expect(within(cardBox('Macro')).queryByLabelText('状态')).toBeNull()
    await fireEvent.click(screen.getByRole('button', { name: '+ 卡片' }))
    await fireEvent.input(screen.getByLabelText('卡片标题'), { target: { value: 'Checker' } })
    await fireEvent.click(screen.getByRole('button', { name: '加入' }))
    expect(actions.card).toHaveBeenLastCalledWith('C-plan', 'C-plan', { op: 'source', change: { do: 'add', title: 'Checker' } })
    await fireEvent.click(screen.getByRole('button', { name: /看不懂的 1 行/ }))
    expect(screen.getByText('Alice -> Bob')).toBeTruthy()
  })

  it('locks editing while the file does not parse and applies a fix only after the person reads it', async () => {
    const actions = { card: vi.fn(), pin: vi.fn(), relink: vi.fn() }
    const broken = { ...source, status: 'unparsable' as const, locked: true, shown_from: 'b'.repeat(40), renamed_to: 'docs/new.puml',
      problems: [{ line: 4, text: 'component "X', reason: '引号没有闭合' }], fixes: [{ id: 'close-quote-4', title: '补上第 4 行的引号', diff: ['- component "X', '+ component "X"'] }] }
    const [node] = toFrameNodes([sourced({ source: broken })], {}, true, undefined, new Map(), actions)
    render(Harness, { props: { node } })
    expect(screen.getByText(/编辑已锁定/)).toBeTruthy()
    expect(screen.getByText(/第 4 行：引号没有闭合/)).toBeTruthy()
    expect(screen.queryByRole('button', { name: '+ 卡片' })).toBeNull()
    await fireEvent.click(within(cardBox('Parser')).getByRole('button', { name: 'Parser' }))
    expect(within(cardBox('Parser')).queryByLabelText('状态')).toBeNull()
    expect(screen.queryByRole('button', { name: 'Fix' })).toBeNull()
    await fireEvent.click(screen.getByRole('button', { name: /修复建议/ }))
    expect(screen.getByText(/\+ component "X"/)).toBeTruthy()
    expect(actions.card).not.toHaveBeenCalled()
    await fireEvent.click(screen.getByRole('button', { name: 'Fix' }))
    expect(actions.card).toHaveBeenCalledWith('C-plan', 'C-plan', { op: 'source', change: { do: 'fix', fix: 'close-quote-4', sha256: 'f'.repeat(64) } })
    await fireEvent.click(screen.getByRole('button', { name: '改为跟随它' }))
    expect(actions.relink).toHaveBeenCalledWith('C-plan', 'docs/new.puml')
  })
})
