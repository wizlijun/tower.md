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
  await fireEvent.input(screen.getByLabelText('验收条件'), { target: { value: '拒绝非法输入\n合法输入可保存' } })
}

describe('minimal task creation', () => {
  it('requires explicit submit and preserves the selected module exact paths', async () => {
    const submit = vi.fn()
    render(TaskCreate, { modules, onSubmit: submit, onRetry: vi.fn() })
    expect((screen.getByRole('button', { name: '创建任务' }) as HTMLButtonElement).disabled).toBe(true)
    await fillRequired()
    expect(submit).not.toHaveBeenCalled()
    await fireEvent.click(screen.getByRole('button', { name: '创建任务' }))
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
    await fireEvent.click(screen.getByRole('button', { name: '创建任务' }))
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
    await fireEvent.click(screen.getByRole('button', { name: '创建任务' }))
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
