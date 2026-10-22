import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import ClarificationPanel from './ClarificationPanel.svelte'
import { delegated } from '../test/delegation'

afterEach(cleanup)

const clarifying = () => delegated({
  phase: 'clarifying', brief: { done_when: [] }, brief_set_by: null, attempts: [], card: null,
  clarification: {
    event: { at: '2026-10-20T10:00:00Z', eid: 'C-1', seq: 4, actor: 'process:towermd-cli' }, answers: [],
    draft: { restated: '把导出改成流式', done_when: ['大文件不爆内存'], questions: ['要兼容旧格式吗？', '上限多少？', '要进度条吗？', '第四问'],
      self_reported: { name: 'claude-code', version: '2.0' } },
  },
})

describe('clarification panel (FR-201)', () => {
  it('shows the restatement and at most three questions, prefilled from the draft, and finalizes with answers', async () => {
    const onFinalize = vi.fn()
    render(ClarificationPanel, { object: clarifying(), onFinalize })
    expect(screen.getByText('把导出改成流式')).toBeTruthy()
    expect(screen.getByText(/claude-code\/2\.0（自报）/)).toBeTruthy()
    expect(screen.queryByText(/第四问/)).toBeNull()
    expect((screen.getByLabelText('完成标准') as HTMLTextAreaElement).value).toBe('大文件不爆内存')
    await fireEvent.input(screen.getByLabelText(/问题 2/), { target: { value: '100MB' } })
    await fireEvent.input(screen.getByLabelText('目的'), { target: { value: '客户导出超时' } })
    await fireEvent.click(screen.getByRole('button', { name: '定稿并委托' }))
    expect(onFinalize).toHaveBeenCalledWith({ brief: { why: '客户导出超时', done_when: ['大文件不爆内存'] }, answers: ['', '100MB', ''] })
  })

  it('cannot finalize without done-when and omits answers nobody gave', async () => {
    const onFinalize = vi.fn()
    render(ClarificationPanel, { object: delegated({ phase: 'clarifying', brief: { done_when: [] }, clarification: null, card: null }), onFinalize })
    expect(screen.getByText(/等待受托者复述与提问/)).toBeTruthy()
    const button = screen.getByRole('button', { name: '定稿并委托' }) as HTMLButtonElement
    expect(button.disabled).toBe(true)
    await fireEvent.input(screen.getByLabelText('完成标准'), { target: { value: '能跑' } })
    await fireEvent.click(button)
    expect(onFinalize).toHaveBeenCalledWith({ brief: { done_when: ['能跑'] } })
  })
})
