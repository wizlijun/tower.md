import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import HumanMessage from './HumanMessage.svelte'
import { humanRequestId } from '../lib/request-id'
import { selectValue } from '../test/select'

afterEach(cleanup)

describe('explicit human messages', () => {
  it('fills the recommended answer without sending and submits only after an explicit click', async () => {
    const submit = vi.fn()
    render(HumanMessage, {
      scopeLabel: '模块 A',
      question: {
        draft: { id: 'H-1', text: '选择实现方案？', options: ['复用现有实现', '替换实现'], recommended: 0, context: [] },
        binding: { run: 'R-1', task: 'T-1', module: 'A' }, targets: [],
      },
      destinations: [{ value: 'temporary', label: '仅本次' }, { value: 'module_decision', label: '模块决策' }],
      onSubmit: submit, onRetry: vi.fn(),
    })
    await fireEvent.click(screen.getByRole('button', { name: '复用现有实现（推荐）' }))
    expect(submit).not.toHaveBeenCalled()
    await selectValue(screen.getByLabelText('回答去向'), 'module_decision')
    await fireEvent.click(screen.getByRole('button', { name: '提交回答' }))
    expect(submit).toHaveBeenCalledWith('复用现有实现', 'module_decision')
  })

  it('keeps an uncertain write immutable and offers only its original retry', async () => {
    const retry = vi.fn(), submit = vi.fn()
    render(HumanMessage, {
      scopeLabel: '全局插话', frozen: { text: '先完成原有测试', destination: 'temporary' },
      error: '回执未知', onSubmit: submit, onRetry: retry,
    })
    expect(screen.queryByRole('textbox')).toBeNull()
    expect(screen.getByText('先完成原有测试')).toBeTruthy()
    await fireEvent.click(screen.getByRole('button', { name: '重试原提交' }))
    expect(retry).toHaveBeenCalledTimes(1)
    expect(submit).not.toHaveBeenCalled()
    expect(humanRequestId()).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/)
  })
})
