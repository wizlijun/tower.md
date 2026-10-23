import { cleanup, render, screen, within } from '@testing-library/svelte'
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
      '✕ 检查未通过 crap（eng.crap） ci/nightly：CRAP 偏高',
      '– 未见阻止 style（eng.style） · 只提醒',
    ])
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
