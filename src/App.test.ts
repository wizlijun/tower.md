import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { tick } from 'svelte'
import type { BaselineDraft, DraftSummary, ProjectClient, ProjectSelection, ProjectView, ScanReport, ScanJob, VerificationStatus } from './lib/api'
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
    cancelScan: vi.fn().mockResolvedValue({ id: 'scan-1', status: 'running', phase: 'verifying', report: scanReport() }),
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
    tree: 'c'.repeat(40), view: { ...view('候选模块'), revision: 'b'.repeat(40) },
    modules: [{ id: '候选模块', title: '候选模块', paths: ['src'], files: ['src/a.ts'], confidence: 0.8, rationale: '源码边界', command_ids: ['test'], suggested_task: '补充模块测试' }],
    commands: [{ id: 'test', directory: '.', command: 'pnpm test', modules: ['候选模块'] }],
    unassigned: [], source_file_count: 2, assigned_file_count: 1, coverage: 0.5,
    diagnostics: ['仅分析已提交文件'], hotspots: [{ path: 'src/a.ts', commits: 2 }],
    couplings: [{ from: 'A', to: 'B', commits: 1 }],
  }
}

async function scanAction(name = '扫描并验证') {
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
    expect(api.cancelScan).not.toHaveBeenCalled()
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
    expect(api.cancelScan).not.toHaveBeenCalled()
  })

  it('offers scan directly on an initialized empty canvas', async () => {
    const api = client()
    vi.mocked(api.loadProject).mockResolvedValue({ ...view('Empty'), nodes: [] })
    render(App, { client: api })
    await fireEvent.click(screen.getByRole('button', { name: '打开项目' }))
    await fireEvent.click(await screen.findByRole('button', { name: '扫描并验证' }))
    expect(api.startScan).toHaveBeenCalledWith('A')
  })
})

function verifiedReport(status: VerificationStatus, cleanup = true): ScanReport {
  const report = scanReport()
  report.verification = {
    actor: 'process:towermd', revision: report.revision, tree: report.tree,
    started_at: '2026-10-21T00:00:00Z', finished_at: cleanup ? '2026-10-21T00:00:01Z' : '',
    commands: [{ id: 'test', directory: 'src', argv: ['node', 'test.js'], modules: ['候选模块', '共享模块'], status,
      exit_code: status === 'passed' ? 0 : status === 'failed' ? 1 : null, duration_ms: 42,
      log: '/tmp/owned/log.txt', log_sha256: 'd'.repeat(64), log_excerpt: '<script>test output</script>', log_truncated: true,
      error: status === 'failed' ? '真实失败' : null }],
    modules: [{ module: '候选模块', status, command_ids: ['test'], shared: true }], cleanup_complete: cleanup,
  }
  return report
}

function baselineDraft(): BaselineDraft {
  const scan = verifiedReport('passed')
  return {
    id: `d-${'a'.repeat(64)}`, producer: 'codex/1', self_reported: true,
    created_at: '2026-10-21T01:00:00Z',
    submission: {
      scan_id: scan.id, project_id: scan.project_id, source_revision: scan.revision,
      intents: [{ id: 'intent-1', path: 'intents/intent-1.md', text: '# 综合意图\n<script>不是 HTML</script>' }],
      modules: [], interfaces: [], acceptances: [], questions: ['怎样判断可用？'], risks: ['只验证了局部。'],
    },
    scan,
    view: { ...scan.view, nodes: [
      ...scan.view.nodes,
      { id: 'intent-1', kind: 'file', object_type: 'Intent', title: '综合意图', content: '# 综合意图\n<script>不是 HTML</script>', x: -300, y: 40, width: 260, height: 180 },
    ] },
  }
}

function baselineClient(value = baselineDraft()) {
  const api = client()
  const summary: DraftSummary = {
    id: value.id, producer: value.producer, created_at: value.created_at,
    source_revision: value.submission.source_revision, title: '综合草稿一', stale: !!value.stale,
  }
  return {
    ...api,
    listBaselines: vi.fn().mockResolvedValue([summary]),
    loadBaseline: vi.fn().mockResolvedValue(value),
  }
}

async function openBaseline() {
  await scanAction('查看综合草稿')
  await fireEvent.click(await screen.findByRole('button', { name: /综合草稿一/ }))
  await screen.findByRole('button', { name: '查看 综合意图' })
}

describe('persistent synthesis drafts', () => {
  it('opens a persisted draft on demand without executing commands and shows literal provenance', async () => {
    const api = baselineClient()
    render(App, { client: api })
    await open()
    expect(api.listBaselines).not.toHaveBeenCalled()
    await openBaseline()
    expect(api.listBaselines).toHaveBeenCalledWith('A')
    expect(api.loadBaseline).toHaveBeenCalledWith('A', baselineDraft().id)
    expect(api.startScan).not.toHaveBeenCalled()
    expect(api.pollScan).not.toHaveBeenCalled()
    expect(screen.queryByRole('complementary')).toBeNull()
    expect(screen.queryByRole('toolbar')).toBeNull()
    expect(screen.getByLabelText('候选模块 验证状态').textContent).toBe('通过')
    await fireEvent.click(screen.getByRole('button', { name: '查看 综合意图' }))
    const node = screen.getByRole('complementary', { name: '节点详情' })
    expect(node.textContent).toContain('<script>不是 HTML</script>')
    expect(node.querySelector('script')).toBeNull()
    expect(node.textContent).toContain('b'.repeat(40))
    await fireEvent.click(screen.getByRole('button', { name: '综合草稿 · 未批准' }))
    const description = screen.getByRole('complementary', { name: '综合说明' })
    expect(description.textContent).toContain('codex/1')
    expect(description.textContent).toContain('2026-10-21T01:00:00Z')
    expect(description.textContent).toContain('怎样判断可用？')
    expect(description.textContent).toContain('只验证了局部。')
    expect(description.textContent).toContain('提交者自报')
    expect(screen.queryByRole('button', { name: '批准' })).toBeNull()
    await scanAction('返回当前画布')
    expect(screen.getByRole('button', { name: '查看 A' })).toBeTruthy()
  })

  it('marks stale drafts and suppresses all success claims when evidence cannot be checked', async () => {
    const value = baselineDraft()
    value.stale = true
    value.evidence_error = '日志已删除，摘要无法复核。'
    const api = baselineClient(value)
    render(App, { client: api })
    await open()
    await openBaseline()
    expect(screen.getByRole('button', { name: '综合草稿 · 未批准 · 已过期 · 证据不可复核' })).toBeTruthy()
    expect(screen.getByLabelText('候选模块 验证状态').textContent).toBe('证据不可复核')
    await fireEvent.click(screen.getByRole('button', { name: '查看 候选模块' }))
    const details = screen.getByRole('complementary')
    expect(details.textContent).toContain('日志已删除，摘要无法复核。')
    expect(details.textContent).toContain('来源提交已过期')
    expect(details.querySelector('.verification-passed')).toBeNull()
    await fireEvent.click(screen.getByRole('button', { name: '综合草稿 · 未批准 · 已过期 · 证据不可复核' }))
    expect(screen.getByRole('complementary').querySelector('.verification-passed')).toBeNull()
    expect(screen.getByRole('complementary').textContent).toContain('不能批准')
  })

  it('allows the initialized empty canvas to discover drafts and explains an empty list', async () => {
    const api = baselineClient()
    api.listBaselines.mockResolvedValue([])
    vi.mocked(api.loadProject).mockResolvedValue({ ...view('空项目'), nodes: [] })
    render(App, { client: api })
    await fireEvent.click(screen.getByRole('button', { name: '打开项目' }))
    await fireEvent.click(await screen.findByRole('button', { name: '查看综合草稿' }))
    await screen.findByText('当前项目还没有综合草稿。')
    expect(screen.getByRole('complementary').textContent).toContain('baseline submit')
    expect(api.startScan).not.toHaveBeenCalled()
  })

  it('preserves draft coordinates and viewport when reloading evidence for the same draft', async () => {
    const api = baselineClient()
    render(App, { client: api })
    await open()
    await openBaseline()
    const fit = screen.getByRole('region', { name: '项目画布' }).getAttribute('data-fit-version')
    const changed = baselineDraft()
    changed.evidence_error = '本机证据缺失'
    changed.view.nodes[0].x = 9000
    changed.view.nodes[0].y = 8000
    api.loadBaseline.mockResolvedValue(changed)
    await openBaseline()
    expect(screen.getByRole('button', { name: '查看 候选模块' }).getAttribute('data-x')).toBe('120')
    expect(screen.getByRole('button', { name: '查看 候选模块' }).getAttribute('data-y')).toBe('40')
    expect(screen.getByRole('region', { name: '项目画布' }).getAttribute('data-fit-version')).toBe(fit)
    expect(screen.getByLabelText('候选模块 验证状态').textContent).toBe('证据不可复核')
  })

  it.each(['refresh', 'project', 'scan'] as const)('ignores a draft response superseded by %s', async (operation) => {
    const pending = deferred<BaselineDraft>()
    const api = baselineClient()
    api.loadBaseline.mockReturnValue(pending.promise)
    vi.mocked(api.chooseProject).mockResolvedValueOnce({ token: 'A', path: '/repo/a' }).mockResolvedValueOnce({ token: 'B', path: '/repo/b' })
    vi.mocked(api.loadProject).mockResolvedValueOnce(view('A')).mockResolvedValue(view('B'))
    vi.mocked(api.startScan).mockResolvedValue({ id: 'scan-1', status: 'ready', report: scanReport() })
    render(App, { client: api })
    await open()
    await scanAction('查看综合草稿')
    await fireEvent.click(await screen.findByRole('button', { name: /综合草稿一/ }))
    await waitFor(() => expect(api.loadBaseline).toHaveBeenCalled())
    if (operation === 'scan') {
      await scanAction()
      await screen.findByRole('button', { name: '查看 候选模块' })
    } else {
      await fireEvent.keyDown(window, { key: operation === 'refresh' ? 'r' : 'o', metaKey: true })
      await screen.findByRole('button', { name: '查看 B' })
    }
    pending.resolve(baselineDraft())
    await tick()
    expect(screen.queryByRole('button', { name: '查看 综合意图' })).toBeNull()
    expect(screen.queryByRole('button', { name: '综合草稿 · 未批准' })).toBeNull()
  })

  it('discards list results after closing and exposes retry after a read failure', async () => {
    const pending = deferred<DraftSummary[]>()
    const api = baselineClient()
    api.listBaselines.mockReturnValueOnce(pending.promise).mockRejectedValueOnce(new Error('草稿读取失败')).mockResolvedValueOnce([])
    render(App, { client: api })
    await open()
    await scanAction('查看综合草稿')
    await fireEvent.keyDown(window, { key: 'Escape' })
    pending.resolve([])
    await tick()
    expect(screen.queryByRole('complementary')).toBeNull()
    await scanAction('查看综合草稿')
    expect((await screen.findByRole('alert')).textContent).toContain('草稿读取失败')
    await fireEvent.click(screen.getByRole('button', { name: '重新读取草稿' }))
    await screen.findByText('当前项目还没有综合草稿。')
  })
})

describe('real verification lifecycle', () => {
  it('shows the running candidate then updates evidence without moving nodes or refitting', async () => {
    const api = client()
    const completed = verifiedReport('passed')
    completed.view.nodes[0] = { ...completed.view.nodes[0], x: 9999, y: 8888, content: '新验证原文' }
    vi.mocked(api.startScan).mockResolvedValue({ id: 'scan-1', status: 'running', phase: 'verifying', report: scanReport() })
    vi.mocked(api.pollScan).mockResolvedValue({ id: 'scan-1', status: 'ready', report: completed })
    render(App, { client: api })
    await open()
    vi.useFakeTimers()
    await scanAction()
    const card = () => screen.getByRole('button', { name: '查看 候选模块' })
    const fit = screen.getByRole('region', { name: '项目画布' }).getAttribute('data-fit-version')
    expect(screen.getByLabelText('候选模块 验证状态').textContent).toBe('验证中')
    expect(card().getAttribute('data-x')).toBe('120')
    await fireEvent.click(card())
    await vi.advanceTimersByTimeAsync(2000)
    await tick()
    expect(screen.getByLabelText('候选模块 验证状态').textContent).toBe('通过')
    expect(card().getAttribute('data-x')).toBe('120')
    expect(card().getAttribute('data-y')).toBe('40')
    expect(screen.getByRole('region', { name: '项目画布' }).getAttribute('data-fit-version')).toBe(fit)
    const details = screen.getByRole('complementary', { name: '节点详情' })
    expect(details.textContent).toContain('新验证原文')
    expect(details.textContent).toContain('共享命令结果')
    expect(details.textContent).toContain('["node","test.js"]')
    expect(details.textContent).toContain('42 ms')
    expect(details.textContent).toContain('/tmp/owned/log.txt')
    expect(details.textContent).toContain('d'.repeat(64))
    expect(details.querySelector('script')).toBeNull()
    expect(screen.queryByText('已批准')).toBeNull()
  })

  it.each([
    ['failed', '失败'], ['no_tests', '无测试'], ['not_run', '未运行'], ['timed_out', '超时'],
  ] as const)('displays actual %s without promoting it to passed', async (status, label) => {
    const api = client()
    vi.mocked(api.startScan).mockResolvedValue({ id: 'scan-1', status: 'ready', report: verifiedReport(status) })
    render(App, { client: api })
    await open()
    await scanAction()
    await screen.findByRole('button', { name: '查看 候选模块' })
    expect(screen.getByLabelText('候选模块 验证状态').textContent).toBe(label)
    await fireEvent.click(screen.getByRole('button', { name: '查看 候选模块' }))
    const text = screen.getByRole('complementary').textContent
    expect(text).not.toContain('✓ 通过')
    if (status === 'no_tests') expect(text).toContain('候选任务建议')
    if (status === 'not_run') expect(text).not.toContain('缺少测试')
  })

  it('waits for cancellation cleanup and ignores an earlier in-flight poll', async () => {
    const stale = deferred<ScanJob>()
    const api = client()
    vi.mocked(api.startScan).mockResolvedValue({ id: 'scan-1', status: 'running', phase: 'verifying', report: scanReport() })
    vi.mocked(api.cancelScan).mockResolvedValue({ id: 'scan-1', status: 'running', phase: 'cancelling', cancel_requested: true, report: verifiedReport('cancelled', false) })
    vi.mocked(api.pollScan).mockReturnValueOnce(stale.promise).mockResolvedValueOnce({ id: 'scan-1', status: 'cancelled', report: verifiedReport('cancelled') })
    render(App, { client: api })
    await open()
    vi.useFakeTimers()
    await scanAction()
    await vi.advanceTimersByTimeAsync(2000)
    await scanAction('取消本次验证')
    expect(api.cancelScan).toHaveBeenCalledWith('A', 'scan-1')
    expect(screen.getByRole('status').textContent).toContain('正在取消并清理')
    stale.resolve({ id: 'scan-1', status: 'ready', report: verifiedReport('passed') })
    await vi.advanceTimersByTimeAsync(2000)
    await tick()
    expect(screen.getByLabelText('候选模块 验证状态').textContent).toBe('已取消')
    expect(screen.queryByRole('status')).toBeNull()
    await vi.advanceTimersByTimeAsync(6000)
    expect(api.pollScan).toHaveBeenCalledTimes(2)
  })

  it('stops polling on terminal cleanup failure and never accepts a passed command as success', async () => {
    const api = client()
    vi.mocked(api.startScan).mockResolvedValue({ id: 'scan-1', status: 'failed', phase: 'complete', report: verifiedReport('passed', false), error: '工作树回收失败' })
    render(App, { client: api })
    await open()
    vi.useFakeTimers()
    await scanAction()
    expect(screen.getByRole('alert').textContent).toContain('清理失败，不能接受通过结果')
    expect(screen.getByLabelText('候选模块 验证状态').textContent).toBe('清理失败')
    expect(screen.queryByRole('status')).toBeNull()
    await fireEvent.click(screen.getByRole('button', { name: '查看 候选模块' }))
    expect(screen.getByRole('complementary').textContent).toContain('清理失败，不能接受通过结果')
    expect(screen.getByRole('complementary').textContent).not.toContain('✓ 通过')
    await vi.advanceTimersByTimeAsync(6000)
    expect(api.pollScan).not.toHaveBeenCalled()
  })

  it('refreshes main without cancelling or starting a duplicate running verification', async () => {
    const api = client()
    vi.mocked(api.startScan).mockResolvedValue({ id: 'scan-1', status: 'running', phase: 'verifying', report: scanReport() })
    vi.mocked(api.pollScan).mockResolvedValue({ id: 'scan-1', status: 'ready', report: verifiedReport('passed') })
    render(App, { client: api })
    await open()
    vi.useFakeTimers()
    await scanAction()
    await fireEvent.keyDown(window, { key: 'r', metaKey: true })
    await tick()
    expect(screen.getByRole('button', { name: '查看 A' })).toBeTruthy()
    await fireEvent.keyDown(window, { key: 'F10', shiftKey: true })
    expect((screen.getByRole('menuitem', { name: '扫描并验证' }) as HTMLButtonElement).disabled).toBe(true)
    await vi.advanceTimersByTimeAsync(6000)
    expect(api.pollScan).not.toHaveBeenCalled()
    expect(api.cancelScan).not.toHaveBeenCalled()
    await fireEvent.click(screen.getByRole('menuitem', { name: '查看验证进度' }))
    await tick()
    expect(api.pollScan).toHaveBeenCalledWith('A', 'scan-1')
    expect(api.startScan).toHaveBeenCalledTimes(1)
    expect(screen.getByRole('button', { name: '查看 A' })).toBeTruthy()
    await scanAction('查看候选')
    expect(screen.getByLabelText('候选模块 验证状态').textContent).toBe('通过')
  })

  it('retains the job ID when refresh happens before the start acknowledgement', async () => {
    const pending = deferred<ScanJob>()
    const api = client()
    vi.mocked(api.startScan).mockReturnValue(pending.promise)
    render(App, { client: api })
    await open()
    await scanAction()
    await fireEvent.keyDown(window, { key: 'r', metaKey: true })
    await tick()
    await fireEvent.keyDown(window, { key: 'F10', shiftKey: true })
    expect((screen.getByRole('menuitem', { name: '扫描并验证' }) as HTMLButtonElement).disabled).toBe(true)
    pending.resolve({ id: 'scan-1', status: 'running', phase: 'verifying', report: scanReport() })
    await tick()
    await tick()
    expect(screen.getByRole('menuitem', { name: '查看验证进度' })).toBeTruthy()
    expect((screen.getByRole('menuitem', { name: '扫描并验证' }) as HTMLButtonElement).disabled).toBe(true)
    expect(api.startScan).toHaveBeenCalledTimes(1)
  })

  it('discards a start acknowledgement even while the replacement project is still loading', async () => {
    const pending = deferred<ScanJob>()
    const replacement = deferred<ProjectView>()
    const api = client()
    vi.mocked(api.startScan).mockReturnValue(pending.promise)
    vi.mocked(api.chooseProject).mockResolvedValueOnce({ token: 'A', path: '/repo/a' }).mockResolvedValueOnce({ token: 'B', path: '/repo/b' })
    vi.mocked(api.loadProject).mockResolvedValueOnce(view('A')).mockReturnValueOnce(replacement.promise)
    render(App, { client: api })
    await open()
    await scanAction()
    await fireEvent.keyDown(window, { key: 'o', metaKey: true })
    await waitFor(() => expect(api.loadProject).toHaveBeenCalledWith('B'))
    pending.resolve({ id: 'scan-1', status: 'running', phase: 'verifying', report: scanReport() })
    await tick()
    replacement.resolve(view('B'))
    await screen.findByRole('button', { name: '查看 B' })
    await fireEvent.keyDown(window, { key: 'F10', shiftKey: true })
    expect(screen.queryByRole('menuitem', { name: '取消本次验证' })).toBeNull()
    expect(screen.queryByRole('button', { name: '查看 候选模块' })).toBeNull()
    expect(api.cancelScan).not.toHaveBeenCalled()
  })

  it('keeps a legacy report unverified and exposes execution behavior without an approval prompt', async () => {
    const api = client()
    vi.mocked(api.startScan).mockResolvedValue({ id: 'scan-1', status: 'ready', report: { ...scanReport(), verification: null } })
    render(App, { client: api })
    await open()
    await scanAction('扫描与执行说明')
    expect(screen.getByRole('complementary').textContent).toContain('允许网络')
    expect(screen.getByRole('complementary').textContent).toContain('不自动安装依赖')
    expect(screen.queryByRole('dialog')).toBeNull()
    await fireEvent.keyDown(window, { key: 'Escape' })
    await scanAction()
    await screen.findByRole('button', { name: '查看 候选模块' })
    expect(screen.getByLabelText('候选模块 验证状态').textContent).toBe('未验证')
  })
})
