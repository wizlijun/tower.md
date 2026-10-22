import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { tick } from 'svelte'
import type { BaselineDraft, DraftSummary, ProjectClient, ProjectSelection, ProjectView, ScanReport, ScanJob, VerificationStatus, PublicationPreview, ProjectState, ProjectStreamMessage, MainResolutionPreview } from './lib/api'
import App from './App.svelte'
import type { RuntimeTask } from './lib/runtime'
import type { AnswerRequest, HumanReceipt, PromptRequest, HumanTaskDraft, ProjectControlRequest } from './lib/api'
import { selectValue } from './test/select'
import { delegationView } from './test/delegation'
import type { DelegationReceipt, DelegationRequest, RuleInput, SendBackInput } from './lib/api'

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
    await fireEvent.click(screen.getByRole('menuitem', { name: '项目态势' }))
    const description = screen.getByRole('complementary', { name: '项目态势' }).textContent
    expect(description).toContain('当前基线只读')
    expect(description).toContain('修改进入草稿')
    expect(description).toContain('人工批准后才会出版')
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

function runtimeState(state: RuntimeTask['state'] = 'running'): ProjectState {
  return {
    status: 'ready', seq: state === 'running' ? 21 : 22,
    projection: {
      Task: {
        'T-1': {
          input: { id: 'T-1', title: '真实任务', module: 'A', touches: [], allowed_paths: ['src/**'], depends_on: [], acceptance: ['通过测试'] },
          state, baseline: 'towermd/B0', needs_review: false, active_run: state === 'running' ? 'R-1' : null,
        },
      },
      Run: {
        'R-1': { binding: { id: 'R-1', host: 'local', worktree: '/repo/task', branch: 't/T-1' }, task: 'T-1',
          state: state === 'running' ? 'active' : 'released', cost_usd: null },
      },
    },
  }
}

describe('real task details on the canvas', () => {
  it('shows real task/run facts, unknown cost and dependencies without moving the module', async () => {
    const module = view('A')
    module.nodes[0].object_id = 'A'; module.nodes[0].object_type = 'Module'
    const api = { ...client(), projectState: vi.fn().mockResolvedValue(runtimeState()) }
    vi.mocked(api.loadProject).mockResolvedValue(module)
    render(App, { client: api }); await open()
    await screen.findByText('▶ 1 个任务 · 1 运行')
    const node = screen.getByRole('button', { name: '查看 A' })
    const position = [node.getAttribute('data-x'), node.getAttribute('data-y')]
    const fit = screen.getByRole('region', { name: '项目画布' }).getAttribute('data-fit-version')
    expect(screen.queryByRole('complementary')).toBeNull()
    await fireEvent.click(node)
    await fireEvent.click(screen.getByRole('button', { name: '查看模块任务（1）' }))
    await fireEvent.click(screen.getByRole('button', { name: /真实任务 T-1/ }))
    expect(screen.getByText('未知（未报告）')).toBeTruthy()
    expect(screen.queryByText('$0.0000')).toBeNull()
    expect(screen.getByText('/repo/task')).toBeTruthy()
    await fireEvent.click(screen.getByRole('button', { name: '任务依赖图' }))
    expect(screen.getByRole('button', { name: '查看 真实任务' })).toBeTruthy()
    await fireEvent.keyDown(window, { key: 'Escape' })
    api.projectState.mockResolvedValue(runtimeState('ready'))
    await scanAction('项目态势')
    await screen.findByText('○ 1 个任务')
    expect([node.getAttribute('data-x'), node.getAttribute('data-y')]).toEqual(position)
    expect(screen.getByRole('region', { name: '项目画布' }).getAttribute('data-fit-version')).toBe(fit)
    expect(api.loadProject).toHaveBeenCalledTimes(1)
  })

  it('discards late reads in sample mode and does not poll in the background', async () => {
    const late = deferred<ProjectState>()
    const api = { ...client(), projectState: vi.fn().mockResolvedValueOnce(runtimeState()).mockReturnValueOnce(late.promise).mockResolvedValue(runtimeState('ready')) }
    render(App, { client: api }); await open()
    await waitFor(() => expect(api.projectState).toHaveBeenCalledTimes(1))
    await scanAction('项目态势')
    expect(api.projectState).toHaveBeenCalledTimes(2)
    vi.useFakeTimers()
    await scanAction('打开功能示例')
    late.resolve({ status: 'needs_recovery', error: '迟到真实状态' })
    await vi.advanceTimersByTimeAsync(6000)
    expect(api.projectState).toHaveBeenCalledTimes(2)
    expect(screen.queryByText('迟到真实状态')).toBeNull()
    await scanAction('关闭示例，返回项目')
    await tick()
    expect(api.projectState).toHaveBeenCalledTimes(3)
    await vi.advanceTimersByTimeAsync(2000)
    expect(api.projectState).toHaveBeenCalledTimes(3)
  })

  it('keeps the real canvas visible when a state request fails', async () => {
    const api = { ...client(), projectState: vi.fn().mockRejectedValue('Hub 暂不可读') }
    render(App, { client: api }); await open()
    await waitFor(() => expect(api.projectState).toHaveBeenCalled())
    expect(screen.getByRole('button', { name: '查看 A' })).toBeTruthy()
    expect(screen.queryByRole('complementary')).toBeNull()
    expect(screen.queryByRole('alert')).toBeNull()
    await scanAction('任务与运行')
    expect((await screen.findByRole('alert')).textContent).toContain('Hub 暂不可读')
    expect(screen.getByRole('button', { name: '查看 A' })).toBeTruthy()
  })
})

function humanReceiptFor(input: { request_id: string }, proposal: string | null = null): HumanReceipt {
  return { request_id: input.request_id, event: input.request_id, seq: 32, status: 'accepted', proposal }
}

describe('App delegation actions', () => {
  const delegationState = (): ProjectState => ({ ...runtimeState('merged'), seq: 40, delegation_view: delegationView({
    backlog: { count: 4, threshold: 3 },
    spot_checks: [{ object: 'T-1', attempt: 'A1', config: 'cfg-1', work_kind: 'A', done: { at: '2026-10-20T10:00:00Z', eid: 'D', seq: 3 }, rate: 0.3, draw: 0.1, resolved: false }],
  }) })
  const receiptFor = (input: { request_id: string }, obj: string): DelegationReceipt =>
    ({ request_id: input.request_id, event: input.request_id, seq: 40, status: 'accepted', obj })

  it('opens a spot check onto its acceptance card, retries an uncertain send-back unchanged and offers the correction as a rule', async () => {
    const api = { ...client(), projectState: vi.fn().mockResolvedValue(delegationState()),
      sendBackAcceptance: vi.fn().mockRejectedValueOnce('回执未知')
        .mockImplementation(async (_token: string, input: DelegationRequest<SendBackInput>) => receiptFor(input, input.object)),
      recordRule: vi.fn().mockImplementation(async (_token: string, input: DelegationRequest<RuleInput>) => receiptFor(input, input.object)) }
    render(App, { client: api }); await open()
    await scanAction('任务与运行')
    expect((await screen.findByText(/4 件等你验收/)).textContent).toContain('≡')
    await fireEvent.click(screen.getByRole('button', { name: '抽查 · 抽查：真实任务' }))
    const card = await screen.findByRole('region', { name: '验收卡' })
    expect(card.textContent).toContain('? 未证实')
    await fireEvent.click(screen.getByRole('button', { name: '退回' }))
    await fireEvent.input(screen.getByLabelText('退回理由'), { target: { value: '接口变了' } })
    await fireEvent.click(screen.getByRole('button', { name: '确认退回' }))
    await screen.findByText('回执未知')
    const original = api.sendBackAcceptance.mock.calls[0]
    expect(original[1]).toMatchObject({ object: 'T-1', data: { reason: '接口变了' } })
    expect(Number.isSafeInteger(original[1].data.review_ms)).toBe(true)
    expect((screen.getByRole('button', { name: '确认退回' }) as HTMLButtonElement).disabled).toBe(true)
    await fireEvent.click(screen.getByRole('button', { name: '重试原提交' }))
    await screen.findByText(/已退回/)
    expect(api.sendBackAcceptance.mock.calls[1]).toEqual(original)
    const offer = screen.getByRole('form', { name: '记为规则' })
    expect((within(offer).getByRole('textbox') as HTMLTextAreaElement).value).toBe('接口变了')
    await fireEvent.click(within(offer).getByRole('button', { name: '记为规则' }))
    await waitFor(() => expect(api.recordRule).toHaveBeenCalledTimes(1))
    const rule = api.recordRule.mock.calls[0][1]
    expect(rule.object).toMatch(/^rule-[0-9a-f-]{36}$/)
    expect(rule.data).toEqual({ kind: 'A', text: '接口变了', source: original[1].request_id })
    await waitFor(() => expect(screen.queryByRole('form', { name: '记为规则' })).toBeNull())
  })

  it('opens the trust ledger from the menu without changing any level by itself', async () => {
    const api = { ...client(), projectState: vi.fn().mockResolvedValue(delegationState()), setDelegationLevel: vi.fn() }
    render(App, { client: api }); await open()
    await waitFor(() => expect(api.projectState).toHaveBeenCalled())
    await scanAction('信任账本')
    expect(await screen.findByText('配置已变化，建议提高抽查比例', { exact: false })).toBeTruthy()
    expect(api.setDelegationLevel).not.toHaveBeenCalled()
  })
})

describe('App task creation and project controls', () => {
  it('creates from actual approved paths and retries an uncertain task with the complete original input', async () => {
    const api = { ...client(), projectState: vi.fn().mockResolvedValue(runtimeState()),
      taskOptions: vi.fn().mockResolvedValue({ baseline: 'towermd/B0', modules: [{ id: 'A', title: 'A', paths: ['src/a.ts'] }] }),
      createHumanTask: vi.fn().mockRejectedValueOnce('回执未知').mockImplementation(async (_token: string, input: { request_id: string; task: HumanTaskDraft }) => ({
        request_id: input.request_id, event: input.request_id, seq: 30, status: 'accepted', task: { input: input.task, origin: 'app_human' },
      })) }
    render(App, { client: api }); await open()
    await scanAction('新建委托')
    await fireEvent.input(await screen.findByLabelText('任务标题'), { target: { value: '修正边界' } })
    await fireEvent.input(screen.getByLabelText('完成标准'), { target: { value: '已有测试通过' } })
    await fireEvent.click(screen.getByRole('button', { name: '委托' }))
    await screen.findByText('回执未知')
    const original = api.createHumanTask.mock.calls[0]
    expect(original[1].task.allowed_paths).toEqual(['src/a.ts'])
    expect(original[1].task.acceptance).toEqual(['已有测试通过'])
    await fireEvent.keyDown(window, { key: 'Escape' })
    await fireEvent.keyDown(window, { key: 'o', metaKey: true })
    expect(api.chooseProject).toHaveBeenCalledTimes(1)
    await scanAction('继续确认委托')
    await fireEvent.click(screen.getByRole('button', { name: '重试原提交' }))
    await screen.findByText(`已委托任务 ${original[1].task.id}`)
    expect(api.createHumanTask.mock.calls[1]).toEqual(original)
    expect(screen.queryByLabelText('任务标题')).toBeNull()
  })

  it('requires explicit App confirmation and preserves the old pause ID when a resume result is unknown', async () => {
    const paused = { ...runtimeState(), projection: { ...runtimeState().projection as object,
      ProjectControl: { paused: true, pause_event: 'pause-1', scope: 'all', resume_requests: [{ event: 'request', pause_event: 'pause-1', actor: 'process:towermd-cli', at: 'now' }] } } }
    const api = { ...client(), projectState: vi.fn().mockResolvedValue(paused),
      controlProject: vi.fn().mockRejectedValueOnce('恢复回执未知').mockImplementation(async (_token: string, input: ProjectControlRequest) => ({
        request_id: input.request_id, event: input.request_id, seq: 35, status: 'accepted',
        control: { paused: true, pause_event: 'pause-2', scope: 'all', resume_requests: [] },
      })) }
    render(App, { client: api }); await open()
    await scanAction('确认恢复领取')
    expect(api.controlProject).not.toHaveBeenCalled()
    await fireEvent.click(screen.getByRole('button', { name: '确认恢复项目' }))
    await screen.findByText('恢复回执未知')
    const original = api.controlProject.mock.calls[0]
    expect(original[1]).toMatchObject({ kind: 'resume', pause_event: 'pause-1' })
    await fireEvent.keyDown(window, { key: 'Escape' })
    await scanAction('继续确认暂停或恢复')
    await fireEvent.click(screen.getByRole('button', { name: '重试原操作' }))
    await screen.findByText('恢复确认已记录；以当前项目状态为准。')
    expect(api.controlProject.mock.calls[1]).toEqual(original)
    expect(screen.queryByText('项目允许领取新任务。')).toBeNull()
  })
})

describe('real human questions and prompts', () => {
  it('freezes an uncertain prompt, blocks switching, and retries the exact original UUIDv7 input', async () => {
    const api = { ...client(), projectState: vi.fn().mockResolvedValue(runtimeState()),
      sendPrompt: vi.fn().mockRejectedValueOnce('连接断开，回执未知').mockImplementation(async (_token: string, input: PromptRequest) => humanReceiptFor(input)) }
    render(App, { client: api }); await open()
    await scanAction('向项目插话')
    await fireEvent.input(screen.getByLabelText('插话内容'), { target: { value: '先运行已有测试' } })
    expect(api.sendPrompt).not.toHaveBeenCalled()
    await fireEvent.click(screen.getByRole('button', { name: '发送插话' }))
    await screen.findByRole('alert')
    const first = api.sendPrompt.mock.calls[0]
    expect(first[0]).toBe('A')
    expect(first[1]).toMatchObject({ scope: { kind: 'global' }, text: '先运行已有测试', targets: [] })
    expect(first[1].request_id).toMatch(/^[0-9a-f-]{14}7[0-9a-f-]{21}$/)
    expect(screen.queryByRole('textbox')).toBeNull()
    await fireEvent.keyDown(window, { key: 'o', metaKey: true })
    expect(api.chooseProject).toHaveBeenCalledTimes(1)
    await fireEvent.keyDown(window, { key: 'Escape' })
    await scanAction('继续确认人工提交')
    await fireEvent.click(screen.getByRole('button', { name: '重试原提交' }))
    await screen.findByText('插话已记录，等待目标运行读取。')
    expect(api.sendPrompt.mock.calls[1]).toEqual(first)
  })

  it('sends module and task prompts with their explicit scope through the same project client', async () => {
    const module = view('A'); module.nodes[0].object_type = 'Module'; module.nodes[0].object_id = 'A'
    const api = { ...client(), projectState: vi.fn().mockResolvedValue(runtimeState()),
      sendPrompt: vi.fn().mockImplementation(async (_token: string, input: PromptRequest) => {
        const receipt = humanReceiptFor(input)
        api.projectState.mockResolvedValue({ ...runtimeState(), seq: receipt.seq })
        return receipt
      }) }
    vi.mocked(api.loadProject).mockResolvedValue(module)
    render(App, { client: api }); await open()
    await fireEvent.click(screen.getByRole('button', { name: '查看 A' }))
    await fireEvent.click(screen.getByRole('button', { name: '向模块插话' }))
    await fireEvent.input(screen.getByLabelText('插话内容'), { target: { value: '检查模块测试' } })
    await fireEvent.click(screen.getByRole('button', { name: '发送插话' }))
    await screen.findByText('插话已记录，等待目标运行读取。')
    expect(api.sendPrompt.mock.calls[0][1].scope).toEqual({ kind: 'module', module: 'A' })
    await fireEvent.keyDown(window, { key: 'Escape' })
    await fireEvent.click(screen.getByRole('button', { name: '查看 A' }))
    await fireEvent.click(screen.getByRole('button', { name: '查看模块任务（1）' }))
    await fireEvent.click(screen.getByRole('button', { name: /真实任务 T-1/ }))
    await fireEvent.click(screen.getByRole('button', { name: '向任务插话' }))
    await fireEvent.input(screen.getByLabelText('插话内容'), { target: { value: '补充边界测试' } })
    await fireEvent.click(screen.getByRole('button', { name: '发送插话' }))
    await screen.findByText('插话已记录，等待目标运行读取。')
    expect(api.sendPrompt.mock.calls[1][1].scope).toEqual({ kind: 'task', task: 'T-1' })
    expect(api.sendPrompt.mock.calls[1][1].request_id).not.toBe(api.sendPrompt.mock.calls[0][1].request_id)
  })

  it('answers a real hold explicitly and reports a slow-layer response as a pending proposal', async () => {
    const state = runtimeState()
    state.projection = { ...(state.projection as object), Hold: {
      'H-1': {
        draft: { id: 'H-1', text: '是否调整模块边界？', options: ['保留原边界', '调整边界'], recommended: 0, context: [], mode: 'live' },
        binding: { module: 'A', task: 'T-1', run: 'R-1' }, targets: [{ id: 'A', layer: 'L1' }],
      },
    }, Answer: {} }
    const api = { ...client(), projectState: vi.fn().mockResolvedValue(state),
      answerQuestion: vi.fn().mockImplementation(async (_token: string, input: AnswerRequest) => humanReceiptFor(input, `P-${input.request_id}`)) }
    render(App, { client: api }); await open()
    await scanAction('任务与运行')
    await fireEvent.click(screen.getByRole('button', { name: '提问 · 是否调整模块边界？' }))
    expect(screen.getByText(/涉及慢层结构/)).toBeTruthy()
    await fireEvent.click(screen.getByRole('button', { name: '保留原边界（推荐）' }))
    expect(api.answerQuestion).not.toHaveBeenCalled()
    await selectValue(screen.getByLabelText('回答去向'), 'module_decision')
    expect(screen.queryByRole('option', { name: '登记规则' })).toBeNull()
    await fireEvent.click(screen.getByRole('button', { name: '提交回答' }))
    await screen.findByText(/已保存为待批准提案/)
    expect(api.answerQuestion.mock.calls[0][0]).toBe('A')
    expect(api.answerQuestion.mock.calls[0][1]).toMatchObject({ hold: 'H-1', text: '保留原边界', remember_as: 'module_decision', targets: [] })
    expect(screen.queryByText('回答已记录，等待 Agent 读取。')).toBeNull()
  })

  it('labels CLI relays as temporary conversation answers while retaining App answer attribution', async () => {
    const state = runtimeState()
    const holds = Object.fromEntries(['relay', 'human'].map((id) => [`H-${id}`, {
      draft: { id: `H-${id}`, text: `问题 ${id}`, options: [], context: [], mode: 'live' },
      binding: { module: 'A', task: 'T-1', run: 'R-1' }, targets: [],
    }]))
    const answers = Object.fromEntries(['relay', 'human'].map((id) => [`H-${id}`, {
      input: { hold: `H-${id}`, text: `回答 ${id}`, remember_as: { kind: 'temporary' }, targets: [] },
      event: `answer-${id}`, actor: id === 'relay' ? 'process:towermd-cli' : 'human:local',
      run: 'R-1', answered_at: '2026-10-21T12:00:00Z', proposal: null, decision: null,
    }]))
    state.projection = { ...(state.projection as object), Hold: holds, Answer: answers }
    render(App, { client: { ...client(), projectState: vi.fn().mockResolvedValue(state) } }); await open()
    await scanAction('任务与运行')
    expect(screen.getByText(/会话转述（仅本次）.*process:towermd-cli/)).toBeTruthy()
    expect(screen.getByText(/已记录本次回答.*human:local/)).toBeTruthy()
    expect(screen.queryByText(/已记录模块决策/)).toBeNull()
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
    expect(screen.getByRole('complementary').textContent).toContain('dev draft submit')
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


describe('human draft review', () => {
  it('keeps a write pending across closing the panel and retries the exact action before navigating', async () => {
    const original = baselineDraft()
    const changed = { ...original, id: `d-${'c'.repeat(64)}`, view: { ...original.view, nodes: original.view.nodes.map((node) => node.id === '候选模块' ? { ...node, title: '人工名称' } : node) } }
    const pending = deferred<BaselineDraft>()
    const api = { ...baselineClient(original), reviewBaseline: vi.fn().mockReturnValueOnce(pending.promise).mockResolvedValueOnce(changed) }
    render(App, { client: api })
    await open(); await openBaseline()
    await fireEvent.click(screen.getByRole('button', { name: '查看 候选模块' }))
    await fireEvent.click(screen.getByRole('button', { name: '编辑草稿' }))
    await fireEvent.input(screen.getByLabelText('名称'), { target: { value: '人工名称' } })
    await fireEvent.click(screen.getByRole('button', { name: '保存名称' }))
    await fireEvent.keyDown(window, { key: 'Escape' })
    await fireEvent.keyDown(window, { key: 'o', metaKey: true })
    await fireEvent.keyDown(window, { key: 'r', metaKey: true })
    expect(api.chooseProject).toHaveBeenCalledTimes(1)
    expect(api.loadProject).toHaveBeenCalledTimes(1)
    pending.reject('连接中断，保存回执未知')
    expect((await screen.findByRole('alert')).textContent).toContain('保存结果尚未确认')
    await fireEvent.click(screen.getByRole('button', { name: '重试原操作' }))
    await screen.findByRole('button', { name: '查看 人工名称' })
    expect(api.reviewBaseline).toHaveBeenNthCalledWith(1, 'A', original.id, { kind: 'rename', node_id: '候选模块', title: '人工名称' })
    expect(api.reviewBaseline.mock.calls[1]).toEqual(api.reviewBaseline.mock.calls[0])
    expect(screen.queryByRole('complementary')).toBeNull()
    expect(api.startScan).not.toHaveBeenCalled()
  })

  it('retains an earlier uncertain write even if its retry gets a definite rejection', async () => {
    const original = baselineDraft()
    const api = { ...baselineClient(original), reviewBaseline: vi.fn().mockRejectedValueOnce('保存回执未知').mockRejectedValueOnce('操作已拒绝：暂时无法读取已有草稿') }
    render(App, { client: api }); await open(); await openBaseline()
    await fireEvent.click(screen.getByRole('button', { name: '移动 候选模块' }))
    await screen.findByRole('alert')
    await fireEvent.click(screen.getByRole('button', { name: '重试原操作' }))
    await waitFor(() => expect(screen.getByRole('alert').textContent).toContain('暂时无法读取已有草稿'))
    expect(screen.getByRole('alert').textContent).toContain('保存结果尚未确认')
    expect(screen.getByRole('button', { name: '重试原操作' })).toBeTruthy()
    await fireEvent.keyDown(window, { key: 'o', metaKey: true })
    expect(api.chooseProject).toHaveBeenCalledTimes(1)
    expect(api.reviewBaseline.mock.calls[1]).toEqual(api.reviewBaseline.mock.calls[0])
  })

  it('allows correcting a rejected operation without treating it as an uncertain write', async () => {
    const api = { ...baselineClient(), reviewBaseline: vi.fn().mockRejectedValue('操作已拒绝：名称无效') }
    render(App, { client: api }); await open(); await openBaseline()
    await fireEvent.click(screen.getByRole('button', { name: '查看 候选模块' }))
    await fireEvent.click(screen.getByRole('button', { name: '编辑草稿' }))
    await fireEvent.click(screen.getByRole('button', { name: '保存名称' }))
    expect((await screen.findByRole('alert')).textContent).toContain('无法保存草稿更改')
    expect(screen.queryByRole('button', { name: '重试原操作' })).toBeNull()
    await fireEvent.click(screen.getByRole('button', { name: '返回编辑' }))
    expect((screen.getByRole('button', { name: '保存名称' }) as HTMLButtonElement).disabled).toBe(false)
  })

  it('rounds fractional drag coordinates without refitting and restores confirmed positions on an uncertain response', async () => {
    const original = baselineDraft()
    const moved = { ...original, id: `d-${'e'.repeat(64)}`, view: { ...original.view, nodes: original.view.nodes.map((node) => node.id === '候选模块' ? { ...node, x: 220, y: 90 } : node) } }
    const api = { ...baselineClient(original), reviewBaseline: vi.fn().mockRejectedValueOnce('回执丢失').mockResolvedValueOnce(moved) }
    render(App, { client: api }); await open(); await openBaseline()
    const fit = screen.getByRole('region', { name: '项目画布' }).getAttribute('data-fit-version')
    await fireEvent.click(screen.getByRole('button', { name: '移动 候选模块' }))
    await screen.findByRole('alert')
    expect(screen.getByRole('button', { name: '查看 候选模块' }).getAttribute('data-x')).toBe('120')
    await fireEvent.click(screen.getByRole('button', { name: '重试原操作' }))
    await waitFor(() => expect(screen.getByRole('button', { name: '查看 候选模块' }).getAttribute('data-x')).toBe('220'))
    expect(api.reviewBaseline).toHaveBeenCalledWith('A', original.id, { kind: 'move', node_id: '候选模块', x: 220, y: 90 })
    expect(screen.getByRole('region', { name: '项目画布' }).getAttribute('data-fit-version')).toBe(fit)
  })

  it('marks edited module evidence pending and clearly separates its old passing scan', async () => {
    const original = baselineDraft()
    const reviewed: BaselineDraft = { ...original, review: { base_id: original.id, actor: 'human:local', actions: [], confirmed_modules: ['候选模块'], pending_modules: ['候选模块'], modules: [{ id: '候选模块', title: '候选模块', files: ['src/a.ts'], commands: [{ directory: '.', command: 'pnpm test' }], verification_pending: true, commands_confirmed: true }] } }
    const api = { ...baselineClient(reviewed), reviewBaseline: vi.fn().mockResolvedValue(reviewed) }
    render(App, { client: api }); await open(); await openBaseline()
    expect(screen.getByLabelText('候选模块 验证状态').textContent).toBe('待重新验证')
    await fireEvent.click(screen.getByRole('button', { name: '查看 候选模块' }))
    expect(screen.getByRole('complementary').textContent).toContain('原扫描历史结果')
    const details = screen.getByRole('complementary')
    expect(details.textContent).toContain(`综合来源（自报）：${original.producer}`)
    expect(details.textContent).toContain(`本次审阅：human:local · ${original.created_at} · 未批准`)
    expect(details.textContent).not.toContain(`提交者（自报）：${original.producer} · ${original.created_at}`)
    expect(screen.getByRole('complementary').querySelector('.verification-passed')).toBeNull()
    await fireEvent.click(screen.getByRole('button', { name: '编辑草稿' }))
    await fireEvent.click(screen.getByText('验证命令 · 已确认'))
    await fireEvent.click(screen.getByRole('button', { name: '确认验证命令' }))
    await waitFor(() => expect(api.reviewBaseline).toHaveBeenCalledWith('A', original.id, { kind: 'commands', module_id: '候选模块', commands: [{ directory: '.', command: 'pnpm test' }] }))
    expect(api.startScan).not.toHaveBeenCalled()
    expect(screen.getByLabelText('候选模块 验证状态').textContent).toBe('待重新验证')
    await fireEvent.click(screen.getByRole('button', { name: '综合草稿 · 未批准' }))
    const provenance = screen.getByRole('complementary', { name: '综合说明' })
    expect(provenance.textContent).toContain('综合来源（自报）')
    expect(provenance.textContent).toContain('本次审阅时间')
    expect(provenance.textContent).toContain('human:local')
    expect(provenance.textContent).not.toContain('提交时间')
  })

  it('adds a source Markdown reference only from its on-demand form', async () => {
    const original = baselineDraft()
    const api = { ...baselineClient(original), reviewBaseline: vi.fn().mockResolvedValue(original) }
    render(App, { client: api }); await open(); await openBaseline()
    expect(screen.queryByLabelText('项目相对路径')).toBeNull()
    await scanAction('引用 Markdown 文档')
    await fireEvent.input(screen.getByLabelText('项目相对路径'), { target: { value: 'docs/guide.md' } })
    await fireEvent.click(screen.getByRole('button', { name: '保存引用' }))
    await waitFor(() => expect(api.reviewBaseline).toHaveBeenCalledWith('A', original.id, { kind: 'reference', path: 'docs/guide.md', x: 0, y: 300 }))
    expect(screen.queryByLabelText('项目相对路径')).toBeNull()
  })

  it('does not offer editable dragging or permit editing a stale draft', async () => {
    const api = { ...baselineClient({ ...baselineDraft(), stale: true }), reviewBaseline: vi.fn() }
    render(App, { client: api }); await open(); await openBaseline()
    expect(screen.queryByRole('button', { name: '移动 候选模块' })).toBeNull()
    await fireEvent.click(screen.getByRole('button', { name: '查看 候选模块' }))
    expect((screen.getByRole('button', { name: '编辑草稿' }) as HTMLButtonElement).disabled).toBe(true)
    expect(api.reviewBaseline).not.toHaveBeenCalled()
  })
})


describe('interactive feature sample', () => {
  it('runs the sample flow with failure gates, retained answers and explicit release without calling the real client', async () => {
    const api = { ...baselineClient(), reviewBaseline: vi.fn() }
    render(App, { client: api })
    await fireEvent.click(screen.getByRole('button', { name: '打开功能示例' }))
    expect(screen.getByRole('button', { name: '功能示例 · 示例数据' })).toBeTruthy()
    expect(screen.getAllByRole('button', { name: /^查看 / })).toHaveLength(8)
    expect(screen.queryByRole('complementary')).toBeNull()
    const canvas = screen.getByRole('region', { name: '项目画布' })
    const originalCoordinates = Array.from(canvas.querySelectorAll('button')).map((node) => [node.getAttribute('data-x'), node.getAttribute('data-y')])
    await fireEvent.click(screen.getByRole('button', { name: '查看 转换任务' }))
    expect((screen.getByRole('button', { name: '领取示例任务' }) as HTMLButtonElement).disabled).toBe(true)
    await fireEvent.click(screen.getByRole('button', { name: '查看 基线审阅' }))
    await fireEvent.click(screen.getByRole('button', { name: '确认示例基线' }))
    await fireEvent.click(screen.getByRole('button', { name: '查看 转换任务' }))
    await fireEvent.click(screen.getByRole('button', { name: '领取示例任务' }))
    await fireEvent.click(screen.getByRole('button', { name: '查看 待答问题' }))
    await fireEvent.input(screen.getByLabelText('示例回答'), { target: { value: '保留空行，转换其余内容。' } })
    await fireEvent.click(screen.getByRole('button', { name: '记录示例回答' }))
    for (const result of ['模拟失败', '模拟无测试']) {
      await fireEvent.click(screen.getByRole('button', { name: '查看 验证结果' }))
      await fireEvent.click(screen.getByRole('button', { name: result }))
      await fireEvent.click(screen.getByRole('button', { name: '查看 人工放行' }))
      expect((screen.getByRole('button', { name: '放行示例结果' }) as HTMLButtonElement).disabled).toBe(true)
    }
    await fireEvent.click(screen.getByRole('button', { name: '查看 验证结果' }))
    await fireEvent.click(screen.getByRole('button', { name: '模拟通过' }))
    await fireEvent.click(screen.getByRole('button', { name: '查看 人工放行' }))
    await fireEvent.click(screen.getByRole('button', { name: '退回示例结果' }))
    expect((screen.getByRole('button', { name: '放行示例结果' }) as HTMLButtonElement).disabled).toBe(true)
    await fireEvent.click(screen.getByRole('button', { name: '查看 待答问题' }))
    expect(screen.getByRole('complementary').textContent).toContain('保留空行，转换其余内容。')
    expect(screen.queryByLabelText('示例回答')).toBeNull()
    await fireEvent.click(screen.getByRole('button', { name: '查看 验证结果' }))
    await fireEvent.click(screen.getByRole('button', { name: '模拟通过' }))
    await fireEvent.click(screen.getByRole('button', { name: '查看 人工放行' }))
    await fireEvent.click(screen.getByRole('button', { name: '放行示例结果' }))
    await fireEvent.click(screen.getByRole('button', { name: '查看 转换任务' }))
    expect(screen.getByRole('complementary').textContent).toContain('示例流程已完成。真实任务只有通过验证并被认证合并后才算完成。')
    expect(Array.from(canvas.querySelectorAll('button')).map((node) => [node.getAttribute('data-x'), node.getAttribute('data-y')])).toEqual(originalCoordinates)
    for (const method of Object.values(api)) expect(method).not.toHaveBeenCalled()
    await fireEvent.click(screen.getByRole('button', { name: '关闭功能示例' }))
    expect(screen.getByRole('button', { name: '打开项目' })).toBeTruthy()
    expect(screen.queryByRole('button', { name: '功能示例 · 示例数据' })).toBeNull()
  })

  it('returns from the sample to the exact existing draft and never uses real project actions inside the sample', async () => {
    const api = { ...baselineClient(), reviewBaseline: vi.fn(), referencePath: vi.fn() }
    render(App, { client: api }); await open(); await openBaseline()
    const originalX = screen.getByRole('button', { name: '查看 候选模块' }).getAttribute('data-x')
    await scanAction('打开功能示例')
    await fireEvent.keyDown(window, { key: 'o', metaKey: true })
    await fireEvent.keyDown(window, { key: 'r', metaKey: true })
    await fireEvent.click(screen.getByRole('button', { name: '查看 基线审阅' }))
    await fireEvent.click(screen.getByRole('button', { name: '确认示例基线' }))
    await fireEvent.click(screen.getByRole('button', { name: '关闭示例，返回项目' }))
    expect(screen.getByRole('button', { name: '查看 候选模块' }).getAttribute('data-x')).toBe(originalX)
    expect(screen.getByRole('button', { name: '综合草稿 · 未批准' })).toBeTruthy()
    expect(api.chooseProject).toHaveBeenCalledTimes(1)
    expect(api.loadProject).toHaveBeenCalledTimes(1)
    expect(api.listBaselines).toHaveBeenCalledTimes(1)
    expect(api.loadBaseline).toHaveBeenCalledTimes(1)
    expect(api.reviewBaseline).not.toHaveBeenCalled()
    expect(api.startScan).not.toHaveBeenCalled()
    expect(api.cancelScan).not.toHaveBeenCalled()
    expect(api.referencePath).not.toHaveBeenCalled()
  })

  it('cannot enter the sample while a real write is unconfirmed', async () => {
    const original = baselineDraft()
    const api = { ...baselineClient(original), reviewBaseline: vi.fn().mockRejectedValueOnce('保存回执未知').mockResolvedValueOnce(original) }
    render(App, { client: api }); await open(); await openBaseline()
    await fireEvent.click(screen.getByRole('button', { name: '移动 候选模块' }))
    await screen.findByRole('alert')
    await fireEvent.keyDown(window, { key: 'F10', shiftKey: true })
    const sampleButton = screen.getByRole('menuitem', { name: '打开功能示例' }) as HTMLButtonElement
    expect(sampleButton.disabled).toBe(true)
    await fireEvent.click(sampleButton)
    expect(screen.queryByRole('button', { name: '功能示例 · 示例数据' })).toBeNull()
    await fireEvent.click(screen.getByRole('button', { name: '重试原操作' }))
    await waitFor(() => expect(api.reviewBaseline).toHaveBeenCalledTimes(2))
    expect(api.reviewBaseline.mock.calls[1]).toEqual(api.reviewBaseline.mock.calls[0])
  })
})

function reviewedDraft(): BaselineDraft {
  const original = baselineDraft()
  return { ...original, review: { base_id: original.id, actor: 'human:local', actions: [], confirmed_modules: ['候选模块'], pending_modules: ['候选模块'], modules: [{ id: '候选模块', title: '候选模块', files: ['src/a.ts'], commands: [{ directory: 'src', command: 'node test.js' }], verification_pending: true, commands_confirmed: true }] } }
}

describe('real reviewed draft verification', () => {
  it('retries one request after an unknown acknowledgement and displays its real result without moving cards', async () => {
    const original = reviewedDraft()
    const report = verifiedReport('failed').verification!
    const done: BaselineDraft = { ...original, id: `d-${'b'.repeat(64)}`, review: { ...original.review!, pending_modules: [], modules: original.review!.modules.map((module) => ({ ...module, verification_pending: false })), verification: { parent_id: original.id, run_id: 'run', input_sha256: 'd'.repeat(64), limits: { command_timeout_ms: 120000, total_timeout_ms: 600000, output_limit_bytes: 1048576 }, report } } }
    const api = { ...baselineClient(original), verifyBaseline: vi.fn().mockRejectedValueOnce('回执丢失').mockImplementation(async (_token, parent, id) => ({ id, parent_id: parent, input_sha256: 'd'.repeat(64), status: 'ready', phase: 'complete', cancel_requested: false, result_id: done.id, error: null })) }
    api.loadBaseline.mockImplementation(async (_token, id) => id === done.id ? done : original)
    render(App, { client: api }); await open(); await openBaseline()
    const fit = screen.getByRole('region', { name: '项目画布' }).getAttribute('data-fit-version')
    const node = screen.getByRole('button', { name: '查看 候选模块' })
    const coordinates = [node.getAttribute('data-x'), node.getAttribute('data-y')]
    await fireEvent.click(screen.getByRole('button', { name: '综合草稿 · 未批准' }))
    await fireEvent.click(screen.getByRole('button', { name: '重新验证当前草稿' }))
    expect((await screen.findByRole('alert')).textContent).toContain('回执丢失')
    await fireEvent.keyDown(window, { key: 'o', metaKey: true })
    expect(api.chooseProject).toHaveBeenCalledTimes(1)
    await fireEvent.click(screen.getByRole('button', { name: '重试原验证请求' }))
    await waitFor(() => expect(screen.getByLabelText('候选模块 验证状态').textContent).toBe('失败'))
    expect(api.verifyBaseline.mock.calls[1]).toEqual(api.verifyBaseline.mock.calls[0])
    expect(screen.getByRole('region', { name: '项目画布' }).getAttribute('data-fit-version')).toBe(fit)
    const after = screen.getByRole('button', { name: '查看 候选模块' })
    expect([after.getAttribute('data-x'), after.getAttribute('data-y')]).toEqual(coordinates)
    expect(api.startScan).not.toHaveBeenCalled()
  })

  it('cancels the existing reviewed verification without invoking scan or approving the draft', async () => {
    const original = reviewedDraft()
    const api = { ...baselineClient(original),
      verifyBaseline: vi.fn().mockImplementation(async (_token, parent, id) => ({ id, parent_id: parent, input_sha256: 'd'.repeat(64), status: 'running', phase: 'verifying', cancel_requested: false, result_id: null, error: null })),
      cancelBaselineVerification: vi.fn().mockImplementation(async (_token, id) => ({ id, parent_id: original.id, input_sha256: 'd'.repeat(64), status: 'cancelled', phase: 'complete', cancel_requested: true, result_id: null, error: null })),
    }
    render(App, { client: api }); await open(); await openBaseline()
    await fireEvent.click(screen.getByRole('button', { name: '综合草稿 · 未批准' }))
    await fireEvent.click(screen.getByRole('button', { name: '重新验证当前草稿' }))
    await fireEvent.click(await screen.findByRole('button', { name: '取消审阅验证' }))
    await screen.findByText('本次审阅验证已取消；请检查各模块结果。')
    expect(api.cancelBaselineVerification).toHaveBeenCalledWith('A', api.verifyBaseline.mock.calls[0][2])
    expect(screen.getByLabelText('候选模块 验证状态').textContent).toBe('待重新验证')
    expect(api.startScan).not.toHaveBeenCalled()
    expect(screen.queryByRole('button', { name: '批准' })).toBeNull()
  })
})

function publicationPreview(draft = baselineDraft()): PublicationPreview {
  return {
    request_id: '0199ff44-1234-7123-8123-0123456789ab',
    draft_id: draft.id, project_id: draft.submission.project_id,
    source_revision: draft.submission.source_revision, target: 'B0',
    prepared_at: '2026-10-21T12:00:00Z', content_hash: 'c'.repeat(64),
  }
}
function publicationReceipt(preview = publicationPreview()) {
  return {
    target: preview.target, content_hash: preview.content_hash,
    commit: 'd'.repeat(40), tag: 'towermd/B0', snapshot_commit: 'e'.repeat(40),
  }
}

describe('real baseline publication and recovery', () => {
  it('blocks an already prepared publication and draft verification while a human message is uncertain', async () => {
    const reviewed = reviewedDraft()
    const preview = publicationPreview(reviewed)
    const api = { ...baselineClient(reviewed), projectState: vi.fn().mockResolvedValue(runtimeState()),
      preparePublication: vi.fn().mockResolvedValue(preview), approvePublication: vi.fn(),
      verifyBaseline: vi.fn(), sendPrompt: vi.fn().mockRejectedValue('人工插话回执未知') }
    render(App, { client: api }); await open(); await openBaseline()
    await fireEvent.click(screen.getByRole('button', { name: '综合草稿 · 未批准' }))
    await fireEvent.click(screen.getByRole('button', { name: '准备批准' }))
    await screen.findByRole('button', { name: '批准 B0' })
    await fireEvent.keyDown(window, { key: 'Escape' })
    await scanAction('向项目插话')
    await fireEvent.input(screen.getByLabelText('插话内容'), { target: { value: '先核对边界' } })
    await fireEvent.click(screen.getByRole('button', { name: '发送插话' }))
    await screen.findByText('人工插话回执未知')
    await fireEvent.keyDown(window, { key: 'Escape' })
    await fireEvent.click(screen.getByRole('button', { name: '综合草稿 · 未批准' }))
    const approve = screen.getByRole<HTMLButtonElement>('button', { name: '批准 B0' })
    const verify = screen.getByRole<HTMLButtonElement>('button', { name: '重新验证当前草稿' })
    expect(approve.disabled).toBe(true)
    expect(verify.disabled).toBe(true)
    await fireEvent.click(approve); await fireEvent.click(verify)
    expect(api.approvePublication).not.toHaveBeenCalled()
    expect(api.verifyBaseline).not.toHaveBeenCalled()
  })

  it('prepares a frozen preview and requires an explicit second click before publishing and refreshing main', async () => {
    const preview = publicationPreview()
    const api = { ...baselineClient(),
      preparePublication: vi.fn().mockResolvedValue({ ...preview, files: { 'private.md': [115, 101, 99, 114, 101, 116] } }),
      approvePublication: vi.fn().mockResolvedValue(publicationReceipt(preview)),
    }
    vi.mocked(api.loadProject).mockResolvedValueOnce(view('A')).mockResolvedValueOnce(view('Published'))
    render(App, { client: api }); await open(); await openBaseline()
    expect(api.preparePublication).not.toHaveBeenCalled()
    await fireEvent.click(screen.getByRole('button', { name: '综合草稿 · 未批准' }))
    await fireEvent.click(screen.getByRole('button', { name: '准备批准' }))
    await screen.findByRole('button', { name: '批准 B0' })
    expect(api.approvePublication).not.toHaveBeenCalled()
    expect(screen.getByLabelText('批准预览').textContent).toContain(preview.content_hash)
    expect(screen.getByLabelText('批准预览').textContent).toContain(preview.source_revision)
    expect(screen.queryByText('private.md')).toBeNull()
    await fireEvent.click(screen.getByRole('button', { name: '批准 B0' }))
    await screen.findByRole('button', { name: '查看 Published' })
    expect(api.approvePublication).toHaveBeenCalledExactlyOnceWith('A', preview.draft_id, {
      request_id: preview.request_id, target: 'B0', prepared_at: preview.prepared_at, content_hash: preview.content_hash,
    })
    expect(screen.getByRole('complementary', { name: '项目态势' }).textContent).toContain('B0 已出版')
    expect(screen.queryByRole('button', { name: '综合草稿 · 未批准' })).toBeNull()
    await fireEvent.keyDown(window, { key: 'Escape' })
    expect(screen.queryByRole('complementary')).toBeNull()
    expect(screen.queryByRole('toolbar')).toBeNull()
  })

  it('preserves an unknown approval and retries exactly that request while blocking edits and project switches', async () => {
    const preview = publicationPreview()
    const api = { ...baselineClient(),
      preparePublication: vi.fn().mockResolvedValue(preview),
      approvePublication: vi.fn().mockRejectedValueOnce('出版回执丢失').mockResolvedValueOnce(publicationReceipt()),
    }
    render(App, { client: api }); await open(); await openBaseline()
    await fireEvent.click(screen.getByRole('button', { name: '综合草稿 · 未批准' }))
    await fireEvent.click(screen.getByRole('button', { name: '准备批准' }))
    await fireEvent.click(await screen.findByRole('button', { name: '批准 B0' }))
    expect((await screen.findByRole('alert')).textContent).toContain('出版回执丢失')
    await fireEvent.keyDown(window, { key: 'o', metaKey: true })
    expect(api.chooseProject).toHaveBeenCalledTimes(1)
    await fireEvent.keyDown(window, { key: 'Escape' })
    await fireEvent.keyDown(window, { key: 'F10', shiftKey: true })
    expect((screen.getByRole('menuitem', { name: '打开功能示例' }) as HTMLButtonElement).disabled).toBe(true)
    await fireEvent.click(screen.getByRole('button', { name: '综合草稿 · 未批准' }))
    await fireEvent.click(screen.getByRole('button', { name: '重试原批准 B0' }))
    await screen.findByText('B0 已出版')
    expect(api.approvePublication.mock.calls[1]).toEqual(api.approvePublication.mock.calls[0])
    expect(api.preparePublication).toHaveBeenCalledTimes(1)
  })

  it('rejects a preview for another source without creating an approval button', async () => {
    const api = { ...baselineClient(),
      preparePublication: vi.fn().mockResolvedValue({ ...publicationPreview(), source_revision: 'f'.repeat(40) }),
      approvePublication: vi.fn(),
    }
    render(App, { client: api }); await open(); await openBaseline()
    await fireEvent.click(screen.getByRole('button', { name: '综合草稿 · 未批准' }))
    await fireEvent.click(screen.getByRole('button', { name: '准备批准' }))
    expect((await screen.findByRole('alert')).textContent).toContain('与当前草稿不一致')
    expect(screen.queryByRole('button', { name: '批准 B0' })).toBeNull()
    expect(api.approvePublication).not.toHaveBeenCalled()
  })

  it('discards an unapproved preview when editing produces a new draft', async () => {
    const draft = baselineDraft()
    const api = { ...baselineClient(draft),
      preparePublication: vi.fn().mockResolvedValue(publicationPreview(draft)),
      approvePublication: vi.fn(),
      reviewBaseline: vi.fn().mockResolvedValue({ ...draft, id: `d-${'e'.repeat(64)}` }),
    }
    render(App, { client: api }); await open(); await openBaseline()
    await fireEvent.click(screen.getByRole('button', { name: '综合草稿 · 未批准' }))
    await fireEvent.click(screen.getByRole('button', { name: '准备批准' }))
    await screen.findByRole('button', { name: '批准 B0' })
    await fireEvent.keyDown(window, { key: 'Escape' })
    await fireEvent.click(screen.getByRole('button', { name: '移动 候选模块' }))
    await waitFor(() => expect(api.reviewBaseline).toHaveBeenCalledTimes(1))
    await fireEvent.click(screen.getByRole('button', { name: '综合草稿 · 未批准' }))
    await screen.findByRole('button', { name: '准备批准' })
    expect(screen.queryByRole('button', { name: '批准 B0' })).toBeNull()
    expect(api.approvePublication).not.toHaveBeenCalled()
  })

  it('polls project state without opening a panel and ignores a late response from the previous project', async () => {
    const old = deferred<ProjectState>()
    const api = { ...client(), projectState: vi.fn().mockImplementation((token: string) => token === 'A' ? old.promise : Promise.resolve({ status: 'ready', seq: 18 })) }
    vi.mocked(api.chooseProject).mockResolvedValueOnce({ token: 'A', path: '/repo/a' }).mockResolvedValueOnce({ token: 'B', path: '/repo/b' })
    vi.mocked(api.loadProject).mockImplementation(async (token) => view(token))
    render(App, { client: api }); await open()
    expect(api.projectState).toHaveBeenCalledWith('A')
    expect(screen.queryByRole('complementary')).toBeNull()
    await scanAction('项目态势')
    await fireEvent.keyDown(window, { key: 'o', metaKey: true })
    await screen.findByRole('button', { name: '查看 B' })
    old.resolve({ status: 'needs_recovery', error: 'A-only error' })
    await tick()
    await scanAction('项目态势')
    await screen.findByText('实时状态可用 · 事件 18')
    expect(screen.queryByText('A-only error')).toBeNull()
    expect(screen.queryByRole('button', { name: '准备恢复' })).toBeNull()
  })

  it('requires explicit recovery confirmation and retries the original request after an unknown result', async () => {
    const requestId = '0199ff44-1234-7123-8123-0123456789ab'
    const api = { ...client(),
      projectState: vi.fn().mockResolvedValue({ status: 'needs_recovery' }),
      requestRecovery: vi.fn().mockResolvedValue({ request_id: requestId }),
      confirmRecovery: vi.fn().mockRejectedValueOnce('恢复回执未知').mockResolvedValueOnce({ status: 'ready', snapshot_commit: 'e'.repeat(40), seq: 4294967300 }),
    }
    render(App, { client: api }); await open(); await scanAction('项目态势')
    await fireEvent.click(await screen.findByRole('button', { name: '准备恢复' }))
    await screen.findByRole('button', { name: '确认恢复项目状态' })
    expect(api.confirmRecovery).not.toHaveBeenCalled()
    await fireEvent.click(screen.getByRole('button', { name: '确认恢复项目状态' }))
    expect((await screen.findByRole('alert')).textContent).toContain('恢复回执未知')
    await fireEvent.keyDown(window, { key: 'o', metaKey: true })
    expect(api.chooseProject).toHaveBeenCalledTimes(1)
    await fireEvent.click(screen.getByRole('button', { name: '重试原恢复确认' }))
    await screen.findByText('恢复已完成。')
    expect(api.confirmRecovery.mock.calls).toEqual([['A', requestId], ['A', requestId]])
    expect(api.requestRecovery).toHaveBeenCalledTimes(1)
    await fireEvent.keyDown(window, { key: 'Escape' })
    expect(screen.queryByRole('complementary')).toBeNull()
  })
})


function streamingClient() {
  const streams: { token: string; subscription: string; send: (message: ProjectStreamMessage) => void }[] = []
  const api = { ...client(), projectState: vi.fn().mockResolvedValue(runtimeState()),
    subscribeProject: vi.fn(async (token: string, subscription: string, send: (message: ProjectStreamMessage) => void) => {
      streams.push({ token, subscription, send })
    }), unsubscribeProject: vi.fn().mockResolvedValue(undefined) }
  const emit = (index: number, state: ProjectState) => {
    const stream = streams[index]
    stream.send({ token: stream.token, subscription: stream.subscription, status: 'connected', state })
  }
  return { api, streams, emit }
}

describe('native live project subscription', () => {
  it('updates only live facts, rejects older frames, and preserves the canvas through disconnect/reconnect', async () => {
    const { api, streams, emit } = streamingClient()
    const module = view('A'); module.nodes[0].object_id = 'A'; module.nodes[0].object_type = 'Module'
    vi.mocked(api.loadProject).mockResolvedValue(module)
    render(App, { client: api }); await open()
    await screen.findByText('▶ 1 个任务 · 1 运行')
    const node = screen.getByRole('button', { name: '查看 A' })
    const position = [node.getAttribute('data-x'), node.getAttribute('data-y')]
    const fit = screen.getByRole('region', { name: '项目画布' }).getAttribute('data-fit-version')
    emit(0, runtimeState('ready')); await tick()
    expect(screen.getByText('○ 1 个任务')).toBeTruthy()
    emit(0, runtimeState()); await tick()
    expect(screen.queryByText('▶ 1 个任务 · 1 运行')).toBeNull()
    streams[0].send({ ...streams[0], status: 'disconnected' }); await tick()
    expect(screen.queryByRole('complementary')).toBeNull()
    await scanAction('任务与运行')
    expect((await screen.findByRole('alert')).textContent).toContain('正在重连')
    emit(0, { ...runtimeState(), seq: 23 }); await tick()
    expect(screen.queryByRole('alert')).toBeNull()
    expect(screen.getByText('▶ 1 个任务 · 1 运行')).toBeTruthy()
    expect([node.getAttribute('data-x'), node.getAttribute('data-y')]).toEqual(position)
    expect(screen.getByRole('region', { name: '项目画布' }).getAttribute('data-fit-version')).toBe(fit)
    expect(api.loadProject).toHaveBeenCalledTimes(1)
    expect(api.subscribeProject).toHaveBeenCalledTimes(1)
    const reads = api.projectState.mock.calls.length
    vi.useFakeTimers(); await vi.advanceTimersByTimeAsync(10000)
    expect(api.projectState).toHaveBeenCalledTimes(reads)
  })

  it('keeps a cancelled chooser subscription, cancels switched projects and sample subscriptions, and ignores late registration', async () => {
    const { api, streams, emit } = streamingClient()
    const pending = deferred<void>()
    api.subscribeProject.mockImplementationOnce((token, subscription, send) => {
      streams.push({ token, subscription, send }); return pending.promise
    })
    vi.mocked(api.chooseProject).mockResolvedValueOnce({ token: 'A', path: '/repo/a' })
      .mockResolvedValueOnce(null).mockResolvedValueOnce({ token: 'B', path: '/repo/b' })
    vi.mocked(api.loadProject).mockImplementation(async (token) => view(token))
    const mounted = render(App, { client: api }); await open()
    await fireEvent.keyDown(window, { key: 'o', metaKey: true }); await tick()
    expect(api.unsubscribeProject).not.toHaveBeenCalled()
    expect(api.subscribeProject).toHaveBeenCalledTimes(1)
    await fireEvent.keyDown(window, { key: 'o', metaKey: true })
    await screen.findByRole('button', { name: '查看 B' })
    expect(api.unsubscribeProject).toHaveBeenCalledWith(streams[0].subscription)
    emit(0, { status: 'needs_recovery', error: 'old project frame' })
    pending.resolve(); await tick()
    expect(api.unsubscribeProject).not.toHaveBeenCalledWith(streams[1].subscription)
    await scanAction('打开功能示例')
    expect(api.unsubscribeProject).toHaveBeenCalledWith(streams[1].subscription)
    emit(1, { status: 'needs_recovery', error: 'sample late frame' })
    await scanAction('关闭示例，返回项目')
    expect(api.subscribeProject).toHaveBeenCalledTimes(3)
    await scanAction('项目态势')
    expect(screen.queryByText('old project frame')).toBeNull()
    expect(screen.queryByText('sample late frame')).toBeNull()
    mounted.unmount()
    expect(api.unsubscribeProject).toHaveBeenCalledWith(streams[2].subscription)
  })

  it('fences queued frames and stale reads behind an actual write receipt without truncating its sequence', async () => {
    const { api, emit } = streamingClient()
    const seq = 4294967300
    const control = { paused: true, pause_event: 'current-pause', scope: 'all' as const, resume_requests: [] }
    const connected = { ...api, controlProject: vi.fn(async (_token: string, input: ProjectControlRequest) => ({
      request_id: input.request_id, event: input.request_id, seq, status: 'accepted' as const, control,
    })) }
    render(App, { client: connected }); await open()
    await scanAction('暂停全部')
    await fireEvent.click(screen.getByRole('button', { name: '暂停项目' }))
    await screen.findByText('操作已记录，正在同步最新状态…')
    emit(0, { ...runtimeState(), seq: seq - 1 }); await tick()
    expect((screen.getByRole('button', { name: '暂停项目' }) as HTMLButtonElement).disabled).toBe(true)
    const paused = { ...runtimeState(), seq, projection: { ...runtimeState().projection as object, ProjectControl: control } }
    emit(0, paused); await tick()
    expect(screen.queryByText('操作已记录，正在同步最新状态…')).toBeNull()
    expect(screen.getByRole('button', { name: '确认恢复项目' })).toBeTruthy()
    emit(0, { ...runtimeState(), seq: seq - 1 }); await tick()
    expect(screen.getByRole('button', { name: '确认恢复项目' })).toBeTruthy()
    expect(api.subscribeProject).toHaveBeenCalledTimes(1)
  })

  it('does not let an older in-flight read overwrite a newer pushed recovery state', async () => {
    const { api, emit } = streamingClient(); const pending = deferred<ProjectState>()
    api.projectState.mockReturnValueOnce(pending.promise)
    render(App, { client: api }); await open()
    emit(0, { status: 'needs_recovery', error: '真实恢复闸门' }); await tick()
    pending.resolve(runtimeState()); await tick()
    api.projectState.mockResolvedValue({ status: 'needs_recovery', error: '真实恢复闸门' })
    await scanAction('项目态势')
    expect(screen.getByText('真实恢复闸门')).toBeTruthy()
    expect(screen.getByRole('button', { name: '准备恢复' })).toBeTruthy()
  })
})


it('isolates unsequenced recovery frames from the connection preceding an actual recovery receipt', async () => {
  const { api, streams, emit } = streamingClient()
  let current: ProjectState = { status: 'needs_recovery' }
  api.projectState.mockImplementation(async () => current)
  const seq = 4294967300
  const connected = { ...api,
    requestRecovery: vi.fn().mockResolvedValue({ request_id: 'original-recovery' }),
    confirmRecovery: vi.fn().mockImplementation(async () => {
      current = { ...runtimeState(), seq }
      return { status: 'ready', seq, snapshot_commit: 'e'.repeat(40) }
    }),
  }
  render(App, { client: connected }); await open(); await scanAction('项目态势')
  await fireEvent.click(await screen.findByRole('button', { name: '准备恢复' }))
  await fireEvent.click(await screen.findByRole('button', { name: '确认恢复项目状态' }))
  await screen.findByText('恢复已完成。')
  await screen.findByText(`实时状态可用 · 事件 ${seq}`)
  emit(0, { status: 'needs_recovery', error: '恢复前的迟到消息' }); await tick()
  expect(screen.queryByText('恢复前的迟到消息')).toBeNull()
  expect(screen.queryByRole('button', { name: '准备恢复' })).toBeNull()
  expect(api.unsubscribeProject).toHaveBeenCalledWith(streams[0].subscription)
  expect(api.subscribeProject).toHaveBeenCalledTimes(2)
})

it.each(['stream', 'read'] as const)('fences a stale %s behind the actual main-resolution receipt before accepting fresh state', async (transport) => {
  const { api, emit } = streamingClient()
  const seq = 4294967300
  const commit = 'a'.repeat(40)
  const preview: MainResolutionPreview = {
    version: 1, request_id: '019a0000-0000-7000-8000-000000000001', operation: 'claim',
    project_id: 'project', docs_dir: 'docs/tower', import_anchor: 'b'.repeat(40),
    main_revision: commit, target_commit: commit, parent_commit: 'b'.repeat(40),
    changes: [{ path: 'docs/tower/arch/A.md', status: 'M', old_mode: '100644', new_mode: '100644', old_oid: 'c'.repeat(40), new_oid: 'd'.repeat(40) }],
    diff: '-原责任\n+新责任', content_hash: 'e'.repeat(64),
  }
  const original: ProjectState = { ...runtimeState(), projection: {
    ...runtimeState().projection as object,
    Alert: { protected: { resolved: false, data: { reason: 'unapproved_protected_change', detail: { commit } } } },
  } }
  const pending = deferred<ProjectState>()
  api.projectState.mockResolvedValue(original)
  const connected = { ...api,
    prepareMainResolution: vi.fn().mockResolvedValue(preview),
    confirmMainResolution: vi.fn(async () => {
      api.projectState.mockReturnValue(pending.promise)
      return { request_id: preview.request_id, operation: preview.operation, target_commit: commit,
        event: 'resolution-original', seq, resolution_commit: commit, snapshot_commit: 'f'.repeat(40) }
    }),
  }
  render(App, { client: connected }); await open(); await scanAction('任务与运行')
  await fireEvent.click(await screen.findByRole('button', { name: '禁止 · main 含未经批准的受保护变更' }))
  await fireEvent.click(screen.getByRole('button', { name: '认领此提交' }))
  await fireEvent.click(await screen.findByRole('button', { name: '确认认领该提交' }))
  await screen.findByText('已认领此提交。')
  await waitFor(() => expect(api.subscribeProject).toHaveBeenCalledTimes(2))
  expect(connected.confirmMainResolution).toHaveBeenCalledWith('A', preview)
  const stale: ProjectState = { ...original, seq: seq - 1 }
  if (transport === 'stream') emit(1, stale)
  else pending.resolve(stale)
  await tick(); await scanAction('任务与运行')
  expect(screen.getByText(`来源 · Hub 已接受事件 ${original.seq}`)).toBeTruthy()
  expect(screen.queryByText(`来源 · Hub 已接受事件 ${seq - 1}`)).toBeNull()
  emit(1, { ...runtimeState(), seq }); await tick()
  expect(screen.getByText(`来源 · Hub 已接受事件 ${seq}`)).toBeTruthy()
  expect(screen.queryByRole('button', { name: '禁止 · main 含未经批准的受保护变更' })).toBeNull()
  emit(1, stale); pending.resolve(stale); await tick()
  expect(screen.getByText(`来源 · Hub 已接受事件 ${seq}`)).toBeTruthy()
  expect(screen.queryByRole('button', { name: '禁止 · main 含未经批准的受保护变更' })).toBeNull()
})


it('opens the existing attention details from an approved project region without adding a permanent panel', async () => {
  const { api } = streamingClient()
  const canvas = view('A')
  canvas.nodes.push({ id: 'project', kind: 'group', region: 'project', title: '项目', content: '', x: -380, y: 0, width: 320, height: 240 })
  vi.mocked(api.loadProject).mockResolvedValue(canvas)
  render(App, { client: api }); await open()
  expect(screen.queryByRole('complementary')).toBeNull()
  expect(screen.getByText('选择查看项目态势')).toBeTruthy()
  await fireEvent.click(screen.getByRole('button', { name: '查看 项目' }))
  expect(await screen.findByRole('complementary', { name: '任务与运行' })).toBeTruthy()
  expect(screen.getByRole('button', { name: /真实任务 T-1/ })).toBeTruthy()
  await fireEvent.keyDown(window, { key: 'Escape' })
  expect(screen.queryByRole('complementary')).toBeNull()
  expect(screen.getByRole('button', { name: '查看 项目' }).getAttribute('data-x')).toBe('-380')
})


it.each(['scan', 'draft'] as const)('keeps a %s project region in review without implying a live attention entry', async (mode) => {
  const value = baselineDraft()
  const project = { id: 'project', kind: 'group' as const, region: 'project', title: '草稿项目', content: '', x: -380, y: 0, width: 320, height: 240 }
  value.scan.view.nodes.push(project)
  value.view.nodes.push(project)
  const api = { ...baselineClient(value), projectState: vi.fn().mockResolvedValue(runtimeState()) }
  vi.mocked(api.startScan).mockResolvedValue({ id: value.scan.id, status: 'ready', report: value.scan })
  render(App, { client: api }); await open()
  if (mode === 'scan') await scanAction()
  else await openBaseline()
  const card = await screen.findByRole('button', { name: '查看 草稿项目' })
  expect(screen.queryByText('选择查看项目态势')).toBeNull()
  expect(screen.queryByLabelText('项目关注摘要')).toBeNull()
  await fireEvent.click(card)
  expect(screen.getByRole('complementary', { name: '节点详情' })).toBeTruthy()
  expect(screen.queryByRole('complementary', { name: '任务与运行' })).toBeNull()
  expect(card.getAttribute('data-x')).toBe('-380')
})
