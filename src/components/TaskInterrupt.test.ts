import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import TaskInterrupt from './TaskInterrupt.svelte'
import type { DelegationControls } from '../lib/delegation'
import type { RuntimeInterrupt, RuntimeReview, RuntimeTask } from '../lib/runtime'
import { delegationView } from '../test/delegation'

afterEach(cleanup)

function controls(): DelegationControls {
  return { view: delegationView(), disabled: false, busy: false, pending: null, error: null, result: null, ruleOffer: null,
    onAction: vi.fn(), onRetry: vi.fn(), onDismissRule: vi.fn() }
}
function held(interrupt: RuntimeInterrupt): RuntimeTask {
  return { input: { id: 'T-1', title: 'T-1', module: 'A', touches: [], allowed_paths: ['src/**'], depends_on: [], acceptance: ['ok'] },
    state: 'held', baseline: 'towermd/B0', needs_review: false, active_run: 'R-1', interrupt }
}

describe('task interrupts and reviews (spec 12.1, FR-58)', () => {
  it('offers retry, rewrite and reassign naming the interrupt, with shape and text, and never a pass', async () => {
    const panel = controls()
    render(TaskInterrupt, { task: held({ kind: 'gate_failed', reason: '检查未通过：CRAP 偏高', opened: 'e-9', gate: 'crap' }), controls: panel })
    const section = screen.getByRole('region', { name: '任务中断' })
    expect(section.textContent).toContain('✕ 中断 · 验证 3 次未过（crap）')
    expect(section.textContent).toContain('豁免 crap')
    const options = within(screen.getByRole('group', { name: '处理选项' })).getAllByRole('button').map((button) => button.textContent)
    expect(options).toEqual(['让 Agent 再修', '改写任务', '重派'])
    await fireEvent.input(screen.getByLabelText('处理说明（可选）'), { target: { value: ' 先修 CRAP ' } })
    await fireEvent.click(screen.getByRole('button', { name: '改写任务' }))
    expect(vi.mocked(panel.onAction).mock.calls[0][0]).toEqual({ kind: 'interrupt', object: 'T-1', data: { choice: 'rewrite', interrupt: 'e-9', note: '先修 CRAP' } })
  })

  it('cannot waive the builtin check or a path-guard stop, and a live interrupt has no choices', () => {
    render(TaskInterrupt, { task: held({ kind: 'gate_failed', reason: 'x', opened: 'e-1', gate: 'core.verify' }), controls: controls() })
    expect(screen.getByRole('region', { name: '任务中断' }).textContent).toContain('不能豁免')
    cleanup()
    render(TaskInterrupt, { task: held({ kind: 'forbidden', reason: 'G2', opened: 'e-2' }), controls: controls() })
    expect(screen.getByRole('region', { name: '任务中断' }).textContent).toContain('⊘ 中断 · 改动被路径守卫拦截')
    cleanup()
    render(TaskInterrupt, { task: held({ kind: 'review', reason: '评审中', opened: 'e-3' }), controls: controls() })
    expect(screen.queryByRole('group', { name: '处理选项' })).toBeNull()
    expect(screen.getByRole('region', { name: '任务中断' }).textContent).toContain('不会超时放行')
  })

  it('takes a verdict on an open review; changes or rejection need a comment', async () => {
    const panel = controls()
    const review: RuntimeReview = { id: 'V-1', object: 'T-1', stage: 'plan', summary: '先拆解析器', artifacts: ['docs/plan.md'], at: 0 }
    render(TaskInterrupt, { task: held({ kind: 'review', reason: '评审中', opened: 'e-3' }), reviews: [review], controls: panel })
    const section = screen.getByRole('region', { name: '评审 V-1' })
    expect(section.textContent).toContain('计划评审（G1）')
    expect(section.textContent).toContain('docs/plan.md')
    const changes = within(section).getByRole('button', { name: '要求修改' }) as HTMLButtonElement
    expect(changes.disabled).toBe(true)
    expect((within(section).getByRole('button', { name: '驳回' }) as HTMLButtonElement).disabled).toBe(true)
    await fireEvent.input(within(section).getByLabelText(/意见/), { target: { value: '把词法也拆出来' } })
    expect(changes.disabled).toBe(false)
    await fireEvent.click(changes)
    expect(vi.mocked(panel.onAction).mock.calls[0][0]).toEqual({ kind: 'verdict', object: 'V-1', data: { verdict: 'changes_requested', comment: '把词法也拆出来' } })
  })

  it('is read-only without the App write channel and shows nothing for a task that waits on no one', () => {
    render(TaskInterrupt, { task: held({ kind: 'budget', reason: '预算用完', opened: 'e-4' }) })
    for (const button of within(screen.getByRole('group', { name: '处理选项' })).getAllByRole('button')) {
      expect((button as HTMLButtonElement).disabled).toBe(true)
    }
    cleanup()
    const { container } = render(TaskInterrupt, { task: { ...held({ kind: 'budget', reason: '', opened: 'e' }), interrupt: null, state: 'running' } })
    expect(container.textContent?.trim()).toBe('')
  })
})
