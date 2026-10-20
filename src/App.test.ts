import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { tick } from 'svelte'
import type { ProjectClient, ProjectSelection, ProjectView, ScanReport } from './lib/api'
import App from './App.svelte'

vi.mock('./components/ProjectCanvas.svelte', async () => ({
  default: (await import('./test/ProjectCanvasStub.svelte')).default,
}))

afterEach(() => { cleanup(); vi.useRealTimers() })

function view(title: string): ProjectView {
  return {
    title, reference: 'main', revision: 'a'.repeat(40), docs_dir: 'docs/tower',
    diagnostics: [], unplaced: [], edges: [],
    nodes: [{ id: title, kind: 'text', title, content: `<script>原文 ${title}</script>`, x: 120, y: 40, width: 260, height: 180 }],
  }
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}

function client(): ProjectClient {
  return {
    chooseProject: vi.fn().mockResolvedValue({ token: 'A', path: '/repo/a' }),
    loadProject: vi.fn().mockResolvedValue(view('A')),
    startScan: vi.fn().mockResolvedValue({ id: 'scan-1', status: 'running' }),
    pollScan: vi.fn().mockResolvedValue({ id: 'scan-1', status: 'ready', report: scanReport() }),
  }
}

async function open() {
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }))
  await screen.findByRole('button', { name: '查看 A' })
}

describe('canvas-only app', () => {
  it('starts with an open action and no permanent tools or details', () => {
    render(App, { client: client() })
    expect(screen.getByRole('button', { name: '打开项目' })).toBeTruthy()
    expect(screen.queryByRole('complementary')).toBeNull()
    expect(screen.queryByRole('menu')).toBeNull()
    expect(screen.queryByRole('toolbar')).toBeNull()
  })

  it('keeps the current project when the folder chooser is cancelled', async () => {
    const api = client()
    vi.mocked(api.chooseProject).mockResolvedValueOnce({ token: 'A', path: '/repo/a' }).mockResolvedValueOnce(null)
    render(App, { client: api })
    await open()
    const fitVersion = screen.getByRole('region', { name: '项目画布' }).getAttribute('data-fit-version')
    await fireEvent.keyDown(window, { key: 'o', metaKey: true })
    await waitFor(() => expect(api.chooseProject).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(screen.queryByRole('status')).toBeNull())
    expect(screen.getByRole('button', { name: '查看 A' })).toBeTruthy()
    expect(screen.getByRole('region', { name: '项目画布' }).getAttribute('data-fit-version')).toBe(fitVersion)
    expect(api.loadProject).toHaveBeenCalledTimes(1)
  })

  it('preserves the current viewport when refreshing, refitting only on request or project switch', async () => {
    const api = client()
    vi.mocked(api.chooseProject)
      .mockResolvedValueOnce({ token: 'A', path: '/repo/a' })
      .mockResolvedValueOnce({ token: 'B', path: '/repo/b' })
    vi.mocked(api.loadProject)
      .mockResolvedValueOnce(view('A'))
      .mockResolvedValueOnce(view('A-refreshed'))
      .mockResolvedValueOnce(view('B'))
    render(App, { client: api })
    await open()
    const canvas = () => screen.getByRole('region', { name: '项目画布' })
    const initialFit = Number(canvas().getAttribute('data-fit-version'))
    await fireEvent.keyDown(window, { key: 'r', metaKey: true })
    await screen.findByRole('button', { name: '查看 A-refreshed' })
    expect(Number(canvas().getAttribute('data-fit-version'))).toBe(initialFit)
    await fireEvent.keyDown(window, { key: 'F10', shiftKey: true })
    await fireEvent.click(screen.getByRole('menuitem', { name: '适应画布' }))
    expect(Number(canvas().getAttribute('data-fit-version'))).toBe(initialFit + 1)
    await fireEvent.keyDown(window, { key: 'o', metaKey: true })
    await screen.findByRole('button', { name: '查看 B' })
    expect(Number(canvas().getAttribute('data-fit-version'))).toBe(initialFit + 2)
  })

  it('keeps the successful canvas and fit state through a refresh failure and retry', async () => {
    const api = client()
    vi.mocked(api.loadProject)
      .mockResolvedValueOnce(view('A'))
      .mockRejectedValueOnce(new Error('读取暂不可用'))
      .mockResolvedValueOnce(view('A-recovered'))
    render(App, { client: api })
    await open()
    const fitVersion = screen.getByRole('region', { name: '项目画布' }).getAttribute('data-fit-version')
    await fireEvent.keyDown(window, { key: 'r', ctrlKey: true })
    expect((await screen.findByRole('alert')).textContent).toContain('读取暂不可用')
    expect(screen.getByRole('button', { name: '查看 A' })).toBeTruthy()
    expect(screen.getByRole('region', { name: '项目画布' }).getAttribute('data-fit-version')).toBe(fitVersion)
    await fireEvent.click(screen.getByRole('button', { name: '重试' }))
    await screen.findByRole('button', { name: '查看 A-recovered' })
    expect(screen.queryByRole('alert')).toBeNull()
    expect(screen.getByRole('region', { name: '项目画布' }).getAttribute('data-fit-version')).toBe(fitVersion)
    expect(api.chooseProject).toHaveBeenCalledTimes(1)
    expect(api.loadProject).toHaveBeenNthCalledWith(3, 'A')
  })

  it('does not replace the new project with a late old project response', async () => {
    const old = deferred<ProjectView>()
    const api = client()
    vi.mocked(api.chooseProject)
      .mockResolvedValueOnce({ token: 'A', path: '/repo/a' })
      .mockResolvedValueOnce({ token: 'B', path: '/repo/b' })
    vi.mocked(api.loadProject).mockImplementation((token) => token === 'A' ? old.promise : Promise.resolve(view('B')))
    render(App, { client: api })
    await fireEvent.click(screen.getByRole('button', { name: '打开项目' }))
    await waitFor(() => expect(api.loadProject).toHaveBeenCalledWith('A'))
    await fireEvent.keyDown(window, { key: 'o', ctrlKey: true })
    await screen.findByRole('button', { name: '查看 B' })
    old.resolve(view('A'))
    await tick()
    expect(screen.queryByRole('button', { name: '查看 A' })).toBeNull()
    expect(screen.getByRole('button', { name: '查看 B' })).toBeTruthy()
  })

  it('ignores a late cancelled chooser after a newer selection', async () => {
    const old = deferred<ProjectSelection | null>()
    const api = client()
    vi.mocked(api.chooseProject).mockReturnValueOnce(old.promise).mockResolvedValueOnce({ token: 'B', path: '/repo/b' })
    vi.mocked(api.loadProject).mockResolvedValue(view('B'))
    render(App, { client: api })
    await fireEvent.click(screen.getByRole('button', { name: '打开项目' }))
    await fireEvent.keyDown(window, { key: 'o', metaKey: true })
    await screen.findByRole('button', { name: '查看 B' })
    old.resolve(null)
    await tick()
    expect(screen.getByRole('button', { name: '查看 B' })).toBeTruthy()
    expect(api.loadProject).toHaveBeenCalledTimes(1)
  })

  it('retries the failed selection without asking for a new folder', async () => {
    const api = client()
    vi.mocked(api.loadProject).mockRejectedValueOnce(new Error('main 不存在')).mockResolvedValueOnce(view('A'))
    render(App, { client: api })
    await fireEvent.click(screen.getByRole('button', { name: '打开项目' }))
    expect((await screen.findByRole('alert')).textContent).toContain('main 不存在')
    await fireEvent.click(screen.getByRole('button', { name: '重试' }))
    await screen.findByRole('button', { name: '查看 A' })
    expect(screen.queryByRole('alert')).toBeNull()
    expect(api.chooseProject).toHaveBeenCalledTimes(1)
    expect(api.loadProject).toHaveBeenNthCalledWith(2, 'A')
  })

  it('retries the failed new project while preserving the last successful canvas', async () => {
    const api = client()
    vi.mocked(api.chooseProject).mockResolvedValueOnce({ token: 'A', path: '/repo/a' }).mockResolvedValueOnce({ token: 'B', path: '/repo/b' })
    vi.mocked(api.loadProject).mockResolvedValueOnce(view('A')).mockRejectedValueOnce(new Error('B 读取失败')).mockResolvedValueOnce(view('B'))
    render(App, { client: api })
    await open()
    await fireEvent.keyDown(window, { key: 'o', metaKey: true })
    await screen.findByRole('alert')
    expect(screen.getByRole('button', { name: '查看 A' })).toBeTruthy()
    await fireEvent.click(screen.getByRole('button', { name: '重试' }))
    await screen.findByRole('button', { name: '查看 B' })
    expect(api.loadProject).toHaveBeenNthCalledWith(3, 'B')
  })

  it('shows literal original text and source only on selection, then closes with Escape', async () => {
    render(App, { client: client() })
    await open()
    expect(screen.queryByRole('complementary')).toBeNull()
    await fireEvent.click(screen.getByRole('button', { name: '查看 A' }))
    const panel = screen.getByRole('complementary', { name: '节点详情' })
    expect(panel.textContent).toContain('<script>原文 A</script>')
    expect(panel.querySelector('script')).toBeNull()
    expect(panel.textContent).toContain('a'.repeat(40))
    await fireEvent.keyDown(window, { key: 'Escape' })
    expect(screen.queryByRole('complementary')).toBeNull()
    expect(screen.getByRole('button', { name: '查看 A' })).toBeTruthy()
  })

  it('opens project tools only on demand and Escape closes the menu', async () => {
    render(App, { client: client() })
    await open()
    await fireEvent.contextMenu(screen.getByRole('main'), { clientX: 100, clientY: 100 })
    expect(screen.getByRole('menu')).toBeTruthy()
    await fireEvent.keyDown(window, { key: 'Escape' })
    expect(screen.queryByRole('menu')).toBeNull()
    await fireEvent.keyDown(window, { key: 'F10', shiftKey: true })
    await fireEvent.click(screen.getByRole('menuitem', { name: '项目说明' }))
    expect(screen.getByRole('complementary', { name: '项目说明' }).textContent).toContain('只读预览')
    await fireEvent.click(screen.getByRole('button', { name: '关闭详情' }))
    expect(screen.queryByRole('complementary')).toBeNull()
  })

  it('re-reads on Cmd+R and ignores an older refresh completion', async () => {
    const old = deferred<ProjectView>()
    const api = client()
    vi.mocked(api.loadProject).mockResolvedValueOnce(view('A')).mockReturnValueOnce(old.promise).mockResolvedValueOnce(view('A-new'))
    render(App, { client: api })
    await open()
    await fireEvent.keyDown(window, { key: 'r', metaKey: true })
    await waitFor(() => expect(api.loadProject).toHaveBeenCalledTimes(2))
    const event = new KeyboardEvent('keydown', { key: 'r', ctrlKey: true, cancelable: true })
    window.dispatchEvent(event)
    expect(event.defaultPrevented).toBe(true)
    await screen.findByRole('button', { name: '查看 A-new' })
    old.resolve(view('A-old'))
    await tick()
    expect(screen.queryByRole('button', { name: '查看 A-old' })).toBeNull()
  })

  it('exposes unplaced modules without inventing canvas positions', async () => {
    const api = client()
    const content = view('Unplaced')
    content.unplaced = content.nodes
    content.nodes = []
    vi.mocked(api.loadProject).mockResolvedValue(content)
    render(App, { client: api })
    await fireEvent.click(screen.getByRole('button', { name: '打开项目' }))
    await fireEvent.click(await screen.findByRole('button', { name: '查看未摆放模块（1）' }))
    expect(screen.queryByRole('region', { name: '项目画布' })).toBeNull()
    await fireEvent.click(screen.getByRole('button', { name: 'Unplaced' }))
    expect(screen.getByRole('complementary', { name: '节点详情' }).textContent).toContain('原文 Unplaced')
  })
})

function scanReport(): ScanReport {
  return {
    id: 'scan-1', project_id: 'repo-a', reference: 'main', revision: 'b'.repeat(40),
    tree: 'c'.repeat(40), view: view('候选模块'),
    modules: [{ id: '候选模块', title: '候选模块', paths: ['src'], files: ['src/a.ts'], confidence: 0.8, rationale: '源码边界', command_ids: ['test'], suggested_task: '补充模块测试' }],
    commands: [{ id: 'test', directory: '.', command: 'pnpm test', modules: ['候选模块'] }],
    unassigned: [], source_file_count: 2, assigned_file_count: 1, coverage: 0.5,
    diagnostics: ['仅分析已提交文件'], hotspots: [{ path: 'src/a.ts', commits: 2 }],
    couplings: [{ from: 'A', to: 'B', commits: 1 }],
  }
}

async function scanAction(name = '扫描仓库') {
  await fireEvent.keyDown(window, { key: 'F10', shiftKey: true })
  await fireEvent.click(screen.getByRole('menuitem', { name }))
}

describe('repository scan candidates', () => {
  it('polls a scan then opens a clearly marked candidate, returning and reopening without rescanning', async () => {
    const api = client()
    render(App, { client: api })
    await open()
    vi.useFakeTimers()
    await scanAction()
    expect(screen.getByRole('status').textContent).toContain('正在分析已提交源码')
    expect(api.startScan).toHaveBeenCalledWith('A')
    expect(api.pollScan).not.toHaveBeenCalled()
    await vi.advanceTimersByTimeAsync(2000)
    await tick()
    expect(api.pollScan).toHaveBeenCalledWith('A', 'scan-1')
    expect(screen.getByRole('button', { name: '候选 · 尚未验证' })).toBeTruthy()
    expect(screen.getByRole('button', { name: '查看 候选模块' })).toBeTruthy()
    expect(screen.queryByRole('status')).toBeNull()
    expect(screen.queryByRole('complementary')).toBeNull()
    await scanAction('返回当前画布')
    expect(screen.getByRole('button', { name: '查看 A' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: '候选 · 尚未验证' })).toBeNull()
    await scanAction('查看候选')
    expect(screen.getByRole('button', { name: '查看 候选模块' })).toBeTruthy()
    await vi.advanceTimersByTimeAsync(6000)
    expect(api.startScan).toHaveBeenCalledTimes(1)
    expect(api.pollScan).toHaveBeenCalledTimes(1)
  })

  it('refreshes back to the current main canvas while retaining the fixed-commit candidate for explicit review', async () => {
    const api = client()
    vi.mocked(api.startScan).mockResolvedValue({ id: 'scan-1', status: 'ready', report: scanReport() })
    vi.mocked(api.loadProject).mockResolvedValueOnce(view('A')).mockResolvedValueOnce(view('A-new'))
    render(App, { client: api })
    await open()
    await scanAction()
    await screen.findByRole('button', { name: '查看 候选模块' })
    await fireEvent.keyDown(window, { key: 'r', metaKey: true })
    await screen.findByRole('button', { name: '查看 A-new' })
    expect(screen.queryByRole('button', { name: '候选 · 尚未验证' })).toBeNull()
    expect(screen.queryByRole('button', { name: '查看 候选模块' })).toBeNull()
    await scanAction('查看候选')
    expect(screen.getByRole('button', { name: '查看 候选模块' })).toBeTruthy()
    await fireEvent.click(screen.getByRole('button', { name: '候选 · 尚未验证' }))
    const panel = screen.getByRole('complementary', { name: '扫描说明' })
    expect(panel.textContent).toContain('候选固定于下方来源提交')
    expect(panel.textContent).toContain('b'.repeat(40))
    expect(api.startScan).toHaveBeenCalledTimes(1)
    expect(api.pollScan).not.toHaveBeenCalled()
  })

  it('shows real scan provenance, unverified commands and module text only on demand', async () => {
    const api = client()
    vi.mocked(api.startScan).mockResolvedValue({ id: 'scan-1', status: 'ready', report: scanReport() })
    render(App, { client: api })
    await open()
    await scanAction()
    await fireEvent.click(await screen.findByRole('button', { name: '候选 · 尚未验证' }))
    const panel = screen.getByRole('complementary', { name: '扫描说明' })
    expect(panel.textContent).toContain('50%')
    expect(panel.textContent).toContain('1 / 2 个源码文件')
    expect(panel.textContent).toContain('尚未验证')
    expect(panel.textContent).toContain('pnpm test')
    expect(panel.textContent).toContain('补充模块测试')
    expect(panel.textContent).toContain('不代表调用依赖')
    expect(panel.textContent).toContain('b'.repeat(40))
    await fireEvent.keyDown(window, { key: 'Escape' })
    await fireEvent.click(screen.getByRole('button', { name: '查看 候选模块' }))
    expect(screen.getByRole('complementary').textContent).toContain('80%')
    expect(screen.getByRole('complementary').textContent).toContain('<script>原文 候选模块</script>')
    expect(screen.getByRole('complementary').querySelector('script')).toBeNull()
  })

  it('preserves an earlier candidate when a rescan fails, and permits retry', async () => {
    const api = client()
    vi.mocked(api.startScan)
      .mockResolvedValueOnce({ id: 'scan-1', status: 'ready', report: scanReport() })
      .mockResolvedValueOnce({ id: 'scan-2', status: 'failed', error: '无法读取源码树' })
      .mockResolvedValueOnce({ id: 'scan-3', status: 'ready', report: scanReport() })
    render(App, { client: api })
    await open()
    await scanAction()
    await screen.findByRole('button', { name: '查看 候选模块' })
    await scanAction()
    expect((await screen.findByRole('alert')).textContent).toContain('无法读取源码树')
    expect(screen.getByRole('button', { name: '查看 候选模块' })).toBeTruthy()
    await fireEvent.click(screen.getByRole('button', { name: '重新扫描' }))
    await waitFor(() => expect(screen.queryByRole('alert')).toBeNull())
    expect(api.startScan).toHaveBeenCalledTimes(3)
  })

  it('ignores an in-flight old scan response after switching projects', async () => {
    const pending = deferred<Awaited<ReturnType<ProjectClient['pollScan']>>>()
    const api = client()
    vi.mocked(api.pollScan).mockReturnValue(pending.promise)
    vi.mocked(api.chooseProject).mockResolvedValueOnce({ token: 'A', path: '/repo/a' }).mockResolvedValueOnce({ token: 'B', path: '/repo/b' })
    vi.mocked(api.loadProject).mockResolvedValueOnce(view('A')).mockResolvedValueOnce(view('B'))
    render(App, { client: api })
    await open()
    vi.useFakeTimers()
    await scanAction()
    await vi.advanceTimersByTimeAsync(2000)
    await fireEvent.keyDown(window, { key: 'o', metaKey: true })
    await tick()
    pending.resolve({ id: 'scan-1', status: 'ready', report: scanReport() })
    await vi.advanceTimersByTimeAsync(6000)
    await tick()
    expect(screen.getByRole('button', { name: '查看 B' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: '查看 候选模块' })).toBeNull()
    expect(screen.queryByRole('status')).toBeNull()
    expect(api.pollScan).toHaveBeenCalledTimes(1)
  })

  it('continues polling after a cancelled folder chooser and stops when unmounted', async () => {
    const api = client()
    vi.mocked(api.chooseProject).mockResolvedValueOnce({ token: 'A', path: '/repo/a' }).mockResolvedValueOnce(null)
    vi.mocked(api.pollScan).mockResolvedValue({ id: 'scan-1', status: 'running' })
    const app = render(App, { client: api })
    await open()
    vi.useFakeTimers()
    await scanAction()
    await fireEvent.keyDown(window, { key: 'o', metaKey: true })
    await vi.advanceTimersByTimeAsync(2000)
    expect(api.pollScan).toHaveBeenCalledTimes(1)
    expect(screen.getByRole('status').textContent).toContain('正在分析已提交源码')
    app.unmount()
    await vi.advanceTimersByTimeAsync(8000)
    expect(api.pollScan).toHaveBeenCalledTimes(1)
  })

  it('offers scan directly on an initialized empty canvas', async () => {
    const api = client()
    vi.mocked(api.loadProject).mockResolvedValue({ ...view('Empty'), nodes: [] })
    render(App, { client: api })
    await fireEvent.click(screen.getByRole('button', { name: '打开项目' }))
    await fireEvent.click(await screen.findByRole('button', { name: '扫描仓库' }))
    expect(api.startScan).toHaveBeenCalledWith('A')
  })
})
