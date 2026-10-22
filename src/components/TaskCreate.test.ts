import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import TaskCreate, { type TaskDraft } from './TaskCreate.svelte'
import { selectValue } from '../test/select'

afterEach(cleanup)

const modules = [
  { id: 'A', title: '核心', paths: ['src/core.ts', 'src/core.test.ts'] },
  { id: 'B', title: '界面', paths: ['src/view.svelte'] },
]
async function fillRequired() {
  await fireEvent.input(screen.getByLabelText('任务标题'), { target: { value: '补齐校验' } })
  await fireEvent.input(screen.getByLabelText('完成标准'), { target: { value: '拒绝非法输入\n合法输入可保存' } })
}

describe('delegation card task creation', () => {
  it('requires explicit submit and preserves the selected module exact paths', async () => {
    const submit = vi.fn()
    render(TaskCreate, { modules, onSubmit: submit, onRetry: vi.fn() })
    expect((screen.getByRole('button', { name: '委托' }) as HTMLButtonElement).disabled).toBe(true)
    await fillRequired()
    expect(submit).not.toHaveBeenCalled()
    await fireEvent.click(screen.getByRole('button', { name: '委托' }))
    expect(submit).toHaveBeenCalledWith({
      id: expect.stringMatching(/^T-[0-9a-f-]{36}$/), title: '补齐校验', module: 'A',
      allowed_paths: ['src/core.ts', 'src/core.test.ts'], touches: [], depends_on: [],
      acceptance: ['拒绝非法输入', '合法输入可保存'], decisions: [], priority: 0,
    })
  })

  it('changes module defaults without adding broad patterns or old module paths', async () => {
    const submit = vi.fn()
    render(TaskCreate, { modules, onSubmit: submit, onRetry: vi.fn() })
    await fillRequired()
    await selectValue(screen.getByLabelText('所属模块'), 'B')
    expect(submit).not.toHaveBeenCalled()
    await fireEvent.click(screen.getByRole('button', { name: '委托' }))
    expect(submit.mock.calls[0][0]).toMatchObject({
      module: 'B', allowed_paths: ['src/view.svelte'], touches: [],
    })
  })

  it('adds touches without implicitly granting their paths, and retains explicit dependencies', async () => {
    const submit = vi.fn()
    render(TaskCreate, { modules, onSubmit: submit, onRetry: vi.fn() })
    await fillRequired()
    await fireEvent.click(screen.getByLabelText('涉及 界面 · B'))
    expect((screen.getByLabelText('src/view.svelte') as HTMLInputElement).checked).toBe(false)
    await fireEvent.click(screen.getByLabelText('src/view.svelte'))
    await fireEvent.click(screen.getByLabelText('src/core.test.ts'))
    await fireEvent.input(screen.getByLabelText('依赖任务 ID'), { target: { value: 'T-previous\nT-other' } })
    await fireEvent.input(screen.getByLabelText('依据决策 ID'), { target: { value: 'D-choice' } })
    await fireEvent.click(screen.getByRole('button', { name: '委托' }))
    expect(submit.mock.calls[0][0]).toMatchObject({
      allowed_paths: ['src/core.ts', 'src/view.svelte'], touches: ['B'],
      depends_on: ['T-previous', 'T-other'], decisions: ['D-choice'],
    })
  })

  it('keeps every frozen field unchanged when modules refresh and only retries', async () => {
    const submit = vi.fn(), retry = vi.fn()
    const frozen: TaskDraft = {
      id: 'T-original', title: '原任务', module: 'A', allowed_paths: ['old.ts'], touches: ['B'],
      acceptance: ['原验收'], depends_on: ['T-before'], decisions: ['D-original'], priority: 3,
      why: '原目的', checkpoints: [{ text: '原检查点', due: '2026-10-25T18:00:00+09:00' }],
    }
    const view = render(TaskCreate, { modules, frozen, error: '回执未知', onSubmit: submit, onRetry: retry })
    await view.rerender({ modules: [{ id: 'C', title: '新模块', paths: ['new.ts'] }] })
    expect(screen.queryByRole('textbox')).toBeNull()
    expect(view.container.querySelector('pre')?.textContent).toBe(JSON.stringify(frozen, null, 2))
    await fireEvent.click(screen.getByRole('button', { name: '重试原提交' }))
    expect(retry).toHaveBeenCalledTimes(1)
    expect(submit).not.toHaveBeenCalled()
    await view.rerender({ busy: true })
    expect((screen.getByRole('button', { name: '正在确认…' }) as HTMLButtonElement).disabled).toBe(true)
    await fireEvent.click(screen.getByRole('button', { name: '正在确认…' }))
    expect(retry).toHaveBeenCalledTimes(1)
  })
})

describe('delegation card fields (FR-184)', () => {
  it('keeps the five fields in fixed order and disables assign without done_when', async () => {
    const submit = vi.fn()
    const view = render(TaskCreate, { modules, onSubmit: submit, onRetry: vi.fn() })
    const labels = [...view.container.querySelectorAll('label')].map((label) => label.firstChild?.textContent?.trim())
    const order = ['目的', '完成标准', '边界', '检查点', '交付前自检'].map((name) => labels.indexOf(name))
    expect(order.every((index, position) => index >= 0 && (position === 0 || index > order[position - 1]))).toBe(true)
    await fireEvent.input(screen.getByLabelText('任务标题'), { target: { value: '补齐校验' } })
    expect((screen.getByRole('button', { name: '委托' }) as HTMLButtonElement).disabled).toBe(true)
    expect(screen.getByText(/补写完成标准后才能委托/)).toBeTruthy()
    expect((screen.getByRole('button', { name: '先澄清' }) as HTMLButtonElement).disabled).toBe(false)
  })

  it('sends only filled fields and parses @ local times into RFC 3339 dues', async () => {
    const submit = vi.fn()
    render(TaskCreate, { modules, onSubmit: submit, onRetry: vi.fn() })
    await fillRequired()
    await fireEvent.input(screen.getByLabelText('目的'), { target: { value: '  用户输入非法时不崩溃 ' } })
    await fireEvent.input(screen.getByLabelText('检查点'), { target: { value: '写完测试 @ 2026-10-25 18:00\n合并前停下' } })
    await fireEvent.click(screen.getByRole('button', { name: '委托' }))
    const draft = submit.mock.calls[0][0]
    expect(draft.why).toBe('用户输入非法时不崩溃')
    expect('limits' in draft || 'self_check' in draft || 'clarify' in draft).toBe(false)
    expect(draft.checkpoints[0].text).toBe('写完测试')
    expect(new Date(draft.checkpoints[0].due).getTime()).toBe(new Date(2026, 9, 25, 18, 0).getTime())
    expect(draft.checkpoints[0].due).toMatch(/^2026-10-25T18:00:00[+-]\d{2}:\d{2}$/)
    expect(draft.checkpoints[1]).toEqual({ text: '合并前停下' })
  })

  it('blocks an impossible checkpoint time and creates a clarify-first task without done_when', async () => {
    const submit = vi.fn()
    render(TaskCreate, { modules, onSubmit: submit, onRetry: vi.fn() })
    await fireEvent.input(screen.getByLabelText('任务标题'), { target: { value: '整理导出' } })
    await fireEvent.input(screen.getByLabelText('检查点'), { target: { value: '初稿 @ 2026-02-30 10:00' } })
    expect(screen.getByRole('alert').textContent).toContain('初稿 @ 2026-02-30 10:00')
    expect((screen.getByRole('button', { name: '先澄清' }) as HTMLButtonElement).disabled).toBe(true)
    await fireEvent.input(screen.getByLabelText('检查点'), { target: { value: '' } })
    await fireEvent.click(screen.getByRole('button', { name: '先澄清' }))
    expect(submit.mock.calls[0][0]).toMatchObject({ title: '整理导出', acceptance: [], clarify: true })
  })

  it('shows the backlog hint beside assign without blocking it', async () => {
    const submit = vi.fn()
    render(TaskCreate, { modules, onSubmit: submit, onRetry: vi.fn(), backlog: { count: 7, threshold: 5 } })
    expect(screen.getByRole('note').textContent).toContain('先验收再托付')
    await fillRequired()
    await fireEvent.click(screen.getByRole('button', { name: '委托' }))
    expect(submit).toHaveBeenCalledTimes(1)
  })
})
