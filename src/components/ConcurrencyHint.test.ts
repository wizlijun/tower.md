import { cleanup, render, screen } from '@testing-library/svelte'
import { afterEach, describe, expect, it } from 'vitest'
import ConcurrencyHint from './ConcurrencyHint.svelte'

afterEach(cleanup)

describe('ConcurrencyHint', () => {
  it('shows the reminder only when the daemon suggests a lower limit', () => {
    const reason = '并行的 4 项任务共有 9 次人工中断（每任务 2.2）：可以把 config.yaml 的 runner.concurrency 从 5 降到 2；Tower.md 只提示，不改配置'
    render(ConcurrencyHint, { hint: { limit: 5, active: 4, interrupts: 9, longest_wait_minutes: null, suggest: 2, reason } })
    expect(screen.getByRole('note', { name: '并发提示' }).textContent).toContain('从 5 降到 2')
  })

  it('stays silent without a suggestion or from an older daemon', () => {
    render(ConcurrencyHint, { hint: { limit: 3, active: 3, interrupts: 0, longest_wait_minutes: null, suggest: null, reason: null } })
    expect(screen.queryByRole('note')).toBeNull()
    cleanup()
    render(ConcurrencyHint, { hint: undefined })
    expect(screen.queryByRole('note')).toBeNull()
  })
})
