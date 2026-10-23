import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import TrustLedger from './TrustLedger.svelte'
import type { DelegationControls } from '../lib/delegation'
import { delegationView } from '../test/delegation'
import { selectValue } from '../test/select'

afterEach(cleanup)

function controls(overrides: Partial<DelegationControls> = {}): DelegationControls {
  return { view: delegationView(), disabled: false, busy: false, pending: null, error: null, result: null, ruleOffer: null,
    onAction: vi.fn(), onRetry: vi.fn(), onDismissRule: vi.fn(), ...overrides }
}

describe('trust ledger (FR-203–206)', () => {
  it('shows one row per config and work kind with unknown metrics as unknown, no overall score, and the config change note', () => {
    render(TrustLedger, { controls: controls() })
    const row = screen.getByRole('region', { name: 'A · cfg-1' })
    const text = row.textContent!
    expect(text).toContain('配置已变化，建议提高抽查比例')
    expect(text).toContain('一次通过50%（1/2）')
    expect(text).toContain('按时—')
    expect(text).toContain('越界未接入')
    expect(text).toContain('兜圈子未接入')
    expect(text).toContain('验收时长—')
    expect(text).toContain('先问后做（来源：默认（清单第一项））')
    expect(text).toContain('↑ 可升至 做完汇报（需人设定）')
    expect(screen.getByText(/没有总分/)).toBeTruthy()
    expect(screen.getByText(/验收时长只计 App 内/)).toBeTruthy()
  })

  it('sets a level and renames a work kind only on explicit human action', async () => {
    const onAction = vi.fn()
    render(TrustLedger, { controls: controls({ onAction }) })
    expect(onAction).not.toHaveBeenCalled()
    const row = screen.getByRole('region', { name: 'A · cfg-1' })
    await selectValue(within(row).getByLabelText(/委托级别 A/), '只看结果')
    await fireEvent.click(within(row).getByRole('button', { name: '设定级别' }))
    expect(onAction).toHaveBeenLastCalledWith({ kind: 'level', data: { config: 'cfg-1', kind: 'A', level: '只看结果' } })
    await selectValue(screen.getByRole('combobox', { name: '项目级委托级别' }), '做完汇报')
    await fireEvent.click(within(screen.getByRole('region', { name: '项目级委托级别' })).getByRole('button', { name: '设定级别' }))
    expect(onAction).toHaveBeenLastCalledWith({ kind: 'level', data: { config: '*', kind: '*', level: '做完汇报' } })
    await selectValue(screen.getByLabelText('原类别'), 'A')
    const submit = screen.getByRole('button', { name: '改名' }) as HTMLButtonElement
    await fireEvent.input(screen.getByLabelText('改为'), { target: { value: 'A' } })
    expect(submit.disabled).toBe(true)
    await fireEvent.input(screen.getByLabelText('改为'), { target: { value: ' 文档 ' } })
    await fireEvent.click(submit)
    expect(onAction).toHaveBeenLastCalledWith({ kind: 'rename', data: { from: 'A', to: '文档' } })
  })

  it('locks every action while another delegation action awaits confirmation', () => {
    render(TrustLedger, { controls: controls({ pending: { kind: 'level', input: { request_id: 'r', data: { config: '*', kind: '*', level: 'x' } } } }) })
    expect(screen.getByText(/设定委托级别：提交结果尚待确认/)).toBeTruthy()
    for (const button of screen.getAllByRole('button', { name: '设定级别' })) expect((button as HTMLButtonElement).disabled).toBe(true)
  })
})
