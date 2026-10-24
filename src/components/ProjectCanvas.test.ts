import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import { tick } from 'svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { CanvasNode, ProjectView } from '../lib/api'
import type { ActivityBadge } from '../lib/runtime'
import ProjectCanvas from './ProjectCanvas.svelte'

afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

function card(id: string, x: number, extra: Partial<CanvasNode> = {}): CanvasNode {
  return { id: `n-${id}`, object_id: id, kind: 'text', title: `模块 ${id}`, content: '', x, y: 40, width: 220, height: 120, ...extra }
}
function view(revision: string, nodes: CanvasNode[]): ProjectView {
  return { title: '项目', reference: 'main', revision, docs_dir: null, diagnostics: [], nodes, edges: [], unplaced: [] }
}
/** Where SvelteFlow put each card: the transform on its wrapper. */
function places(container: HTMLElement): Record<string, string> {
  return Object.fromEntries([...container.querySelectorAll<HTMLElement>('.svelte-flow__node')]
    .map((node) => [node.dataset.id ?? '', node.style.transform]))
}
const running: ActivityBadge = { label: '1 运行', symbol: '▶', tone: 'verifying' }

describe('the project canvas fast layer (FR-50, FR-45, FR-36)', () => {
  it('shows several progress changes within a second as one, without moving any card', async () => {
    vi.useFakeTimers()
    const shown = view('r1', [card('a', 0), card('b', 300)])
    const { container, rerender } = render(ProjectCanvas, { props: { view: shown, onOpen: vi.fn() } })
    await tick()
    const before = places(container)
    expect(before).toEqual({ 'n-a': 'translate(0px, 40px)', 'n-b': 'translate(300px, 40px)' })
    await rerender({ view: shown, onOpen: vi.fn(), activity: { a: running } })
    await rerender({ view: shown, onOpen: vi.fn(), activity: { a: { ...running, label: '2 运行' } } })
    vi.advanceTimersByTime(999)
    await tick()
    expect(screen.queryByText('2 运行')).toBeNull()
    expect(screen.queryByText('1 运行')).toBeNull()
    vi.advanceTimersByTime(1)
    await tick()
    expect(screen.getByText('2 运行')).toBeTruthy()
    expect(screen.queryByText('1 运行')).toBeNull()
    expect(places(container)).toEqual(before)
  })

  it('labels proposed cards and relation ends in words and symbols, in place', async () => {
    const shown = view('r1', [card('a', 0), card('b', 300)])
    const { container, rerender } = render(ProjectCanvas, { props: { view: shown, onOpen: vi.fn() } })
    await tick()
    const before = places(container)
    await rerender({ view: shown, onOpen: vi.fn(), marks: { a: { proposed: '提案待审阅', relation: ['模块 b'] }, b: { relation: ['模块 a'] } } })
    expect(screen.getByText(/拟议 · 提案待审阅/)).toBeTruthy()
    expect(screen.getByText(/关系一端 · 另一端 模块 b/)).toBeTruthy()
    expect(container.querySelectorAll('.canvas-card.is-proposed')).toHaveLength(1)
    expect(container.querySelectorAll('.canvas-card.is-relation-end')).toHaveLength(2)
    expect(places(container)).toEqual(before)
  })
})

describe('the ghost layer (FR-46, FR-35)', () => {
  it('shows the approved version behind a draft by default, and hides it on request', async () => {
    const approved = [card('a', 0), card('b', 300), card('gone', 600)]
    render(ProjectCanvas, { props: { view: view('r1', [card('a', 0), card('b', 500)]), onOpen: vi.fn(), before: approved } })
    await tick()
    const toggle = screen.getByRole('button', { name: /旧版本幽灵层（2）/ })
    expect(toggle.getAttribute('aria-pressed')).toBe('true')
    const layer = screen.getByRole('group', { name: '旧版本幽灵层' })
    expect(layer.textContent).toContain('旧版本 · 原位置')
    expect(layer.textContent).toContain('模块 b')
    expect(layer.textContent).toContain('旧版本 · 已删除')
    expect(layer.textContent).toContain('模块 gone')
    await fireEvent.click(toggle)
    expect(screen.queryByRole('group', { name: '旧版本幽灵层' })).toBeNull()
    expect(toggle.getAttribute('aria-pressed')).toBe('false')
  })

  it('offers the previous version after a new one changes the layout; progress and refits never do', async () => {
    vi.useFakeTimers()
    const first = view('r1', [card('a', 0), card('b', 300)])
    const { container, rerender } = render(ProjectCanvas, { props: { view: first, onOpen: vi.fn() } })
    await tick()
    await rerender({ view: view('r1', first.nodes), onOpen: vi.fn(), activity: { a: running } })
    expect(screen.queryByRole('button', { name: /旧版本幽灵层/ })).toBeNull()
    await rerender({ view: view('r2', [card('a', 0), card('b', 300)]), onOpen: vi.fn() })
    expect(screen.queryByRole('button', { name: /旧版本幽灵层/ })).toBeNull()
    await rerender({ view: view('r3', [card('a', 0), card('b', 700)]), onOpen: vi.fn() })
    // The moved card glides from its old place to the new one; the other stays put.
    vi.advanceTimersByTime(250)
    await tick()
    const halfway = places(container)
    expect(halfway['n-a']).toBe('translate(0px, 40px)')
    expect(halfway['n-b']).not.toBe('translate(300px, 40px)')
    expect(halfway['n-b']).not.toBe('translate(700px, 40px)')
    vi.advanceTimersByTime(400)
    await tick()
    expect(places(container)).toEqual({ 'n-a': 'translate(0px, 40px)', 'n-b': 'translate(700px, 40px)' })
    const toggle = screen.getByRole('button', { name: /旧版本幽灵层（1）/ })
    expect(toggle.getAttribute('aria-pressed')).toBe('false')
    expect(screen.queryByRole('group', { name: '旧版本幽灵层' })).toBeNull()
    await fireEvent.click(toggle)
    expect(screen.getByRole('group', { name: '旧版本幽灵层' }).textContent).toContain('模块 b')
    await rerender({ view: view('r3', [card('a', 0), card('b', 700)]), onOpen: vi.fn(), fitVersion: 1 })
    expect(screen.queryByRole('button', { name: /旧版本幽灵层/ })).toBeNull()
  })
})
