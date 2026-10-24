import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import DelegationDetails from './DelegationDetails.svelte'
import type { DelegationControls } from '../lib/delegation'
import { delegated, delegationView } from '../test/delegation'

afterEach(cleanup)

function controls(): DelegationControls {
  return { view: delegationView(), disabled: false, busy: false, pending: null, error: null, result: null, ruleOffer: null,
    onAction: vi.fn(), onRetry: vi.fn(), onDismissRule: vi.fn() }
}

describe('delegation details: completion gates (FR-120–124)', () => {
  it('shows a failed gate as 检查未通过 with shape, text and reason', () => {
    const object = delegated({ phase: 'claimed_done', completion: { verdict: 'blocked', version: 'abc', gates: [
      { gate: 'core.verify', check: 'core.verify', hard: true, builtin: true, status: 'passed', evidence: ['E-w'] },
      { gate: 'crap', check: 'eng.crap', hard: true, builtin: false, status: 'failed', reason: 'ci/nightly：CRAP 偏高', evidence: ['S-1'] },
      { gate: 'style', check: 'eng.style', hard: false, builtin: false, status: 'clear' },
    ] } })
    render(DelegationDetails, { object, controls: controls() })
    expect(screen.getByRole('heading', { name: '检查 · 检查未通过' })).toBeTruthy()
    const rows = within(screen.getByRole('list', { name: '检查' })).getAllByRole('listitem')
    expect(rows.map((row) => row.textContent?.replace(/\s+/g, ' ').trim())).toEqual([
      '✓ 检查通过 core.verify',
      '✕ 检查未通过 crap（eng.crap） ci/nightly：CRAP 偏高 豁免…',
      '– 未见阻止 style（eng.style） · 只提醒',
    ])
  })

  it('waives only a blocking declared hard gate, with a reason and at most 30 days (FR-72)', async () => {
    const object = delegated({ phase: 'claimed_done', completion: { verdict: 'blocked', version: 'abc', gates: [
      { gate: 'core.verify', check: 'core.verify', hard: true, builtin: true, status: 'failed' },
      { gate: 'crap', check: 'eng.crap', hard: true, builtin: false, status: 'failed', reason: 'CRAP 偏高' },
      { gate: 'style', check: 'eng.style', hard: false, builtin: false, status: 'failed' },
      { gate: 'lint', check: 'eng.lint', hard: true, builtin: false, status: 'error' },
    ] } })
    const panel = controls()
    render(DelegationDetails, { object, controls: panel })
    const buttons = screen.getAllByRole('button', { name: '豁免…' })
    expect(buttons).toHaveLength(1)
    await fireEvent.click(buttons[0])
    const form = screen.getByRole('form', { name: '豁免门禁 crap' })
    const submit = within(form).getByRole('button', { name: '豁免' }) as HTMLButtonElement
    expect(submit.disabled).toBe(true)
    await fireEvent.input(within(form).getByLabelText('豁免理由（一句话）'), { target: { value: ' 上游修复前先放行 ' } })
    const days = within(form).getByLabelText(/有效天数/)
    await fireEvent.input(days, { target: { value: '31' } })
    expect(submit.disabled).toBe(true)
    await fireEvent.input(days, { target: { value: '3' } })
    const before = Date.now()
    await fireEvent.submit(form)
    const intent = vi.mocked(panel.onAction).mock.calls[0][0]
    expect(intent).toMatchObject({ kind: 'waive', object: object.id, data: { gate: 'crap', reason: '上游修复前先放行' } })
    const until = Date.parse((intent as { data: { until: string } }).data.until)
    expect(until - before).toBeGreaterThanOrEqual(3 * 24 * 3600 * 1000 - 1000)
    expect(until - before).toBeLessThanOrEqual(3 * 24 * 3600 * 1000 + 1000)
    expect(screen.queryByRole('form', { name: '豁免门禁 crap' })).toBeNull()
  })

  it('shows a waived gate with its own shape and the waiver in the reason', () => {
    const object = delegated({ phase: 'claimed_done', completion: { verdict: 'passed', version: 'abc', gates: [
      { gate: 'crap', check: 'eng.crap', hard: true, builtin: false, status: 'waived', reason: '已豁免至 2026-10-31T00:00:00Z：上游修复前', evidence: ['W-1'] },
    ] } })
    render(DelegationDetails, { object, controls: controls() })
    const row = within(screen.getByRole('list', { name: '检查' })).getByRole('listitem')
    expect(row.textContent?.replace(/\s+/g, ' ').trim()).toBe('↷ 已豁免 crap（eng.crap） 已豁免至 2026-10-31T00:00:00Z：上游修复前')
    expect(screen.queryByRole('button', { name: '豁免…' })).toBeNull()
  })

  it('blocks on a broken declaration and stays silent when nothing is declared or checked', () => {
    const broken = delegated({ completion: { verdict: 'blocked', version: null, gates: [], error: '.towermd/gates.yaml 不是合法 YAML' } })
    render(DelegationDetails, { object: broken, controls: controls() })
    expect(screen.getByRole('list', { name: '检查' }).textContent).toContain('检查未通过')
    cleanup()
    render(DelegationDetails, { object: delegated({ completion: { verdict: 'human_only', version: null, gates: [] } }), controls: controls() })
    expect(screen.queryByRole('list', { name: '检查' })).toBeNull()
  })
})
