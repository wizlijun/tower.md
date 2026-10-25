import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import ControllerPlans from './ControllerPlans.svelte'

afterEach(cleanup)

const previewed = {
  id: 'plan-h1', scope: { kind: 'module', module: 'A' }, text: '先停 A 模块', requested_by: 'human:local', requested_at: '2026-10-26T01:00:00Z',
  status: 'previewed', summary: '暂停 T-1，提高 T-2', agent: 'claude-code/2.1', cost_usd: null,
  steps: [{ action: 'pause', task: 'T-1' }, { action: 'priority', task: 'T-2', priority: 5 }],
  drafts: [{ target: 'A', text: '把 A 的边界改成只读' }], notes: [],
}
const failed = {
  id: 'plan-h0', scope: { kind: 'global' }, text: '全部重排', requested_by: 'human:local', requested_at: '2026-10-25T01:00:00Z',
  status: 'failed', reason: '总控超时', steps: [],
}
const done = { ...previewed, id: 'plan-old', status: 'confirmed', requested_at: '2026-10-24T01:00:00Z', resolved_by: 'human:local' }

describe('ControllerPlans', () => {
  it('runs a plan only on the person\'s confirmation and shows slow-layer drafts as never run', async () => {
    const onResolve = vi.fn()
    render(ControllerPlans, { projection: { ControllerPlan: { [previewed.id]: previewed, [done.id]: done } }, onRequest: vi.fn(), onResolve })
    const plan = screen.getByRole('article', { name: '计划 plan-h1' })
    expect(plan.textContent).toContain('待你确认')
    expect(plan.textContent).toContain('涉及 2 项任务')
    expect(plan.textContent).toContain('暂停 T-1')
    expect(plan.textContent).toContain('确认也不会执行')
    expect(plan.textContent).toContain('费用 未知')
    await fireEvent.click(screen.getByRole('button', { name: '确认执行' }))
    expect(onResolve).toHaveBeenCalledWith('plan-h1', 'confirm')
    // A resolved plan offers nothing more.
    const resolved = screen.getByRole('article', { name: '计划 plan-old' })
    expect(resolved.querySelector('button')).toBeNull()
  })

  it('lets the person take over a failed plan, which cannot be confirmed', async () => {
    const onResolve = vi.fn()
    render(ControllerPlans, { projection: { ControllerPlan: { [failed.id]: failed } }, onRequest: vi.fn(), onResolve })
    expect(screen.getByRole('alert').textContent).toContain('总控超时')
    expect(screen.queryByRole('button', { name: '确认执行' })).toBeNull()
    await fireEvent.click(screen.getByRole('button', { name: '我来接管' }))
    expect(onResolve).toHaveBeenCalledWith('plan-h0', 'take_over')
  })

  it('hands module or project input to the controller', async () => {
    const onRequest = vi.fn()
    render(ControllerPlans, { projection: {}, module: 'A', onRequest, onResolve: vi.fn() })
    expect(screen.getByLabelText('对模块 A 说')).toBeTruthy()
    await fireEvent.input(screen.getByRole('textbox'), { target: { value: '  先停 A 模块 ' } })
    await fireEvent.click(screen.getByRole('button', { name: '交给总控' }))
    expect(onRequest).toHaveBeenCalledWith('先停 A 模块')
  })
})
