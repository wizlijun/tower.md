import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { FrameDeviationRead, FrameEvolutionRead } from '../lib/api'
import FrameTimeView from './FrameTimeView.svelte'

afterEach(cleanup)

const deviation: FrameDeviationRead = { view: 'plan_deviation', deviation: { frame: 'C-plan', field: 'date', done_states: ['已完成'], today: '2026-10-26', cards: [
  { card: 'C-plan.1', title: '冻结', planned: '2026-10-10', plan: '2026-10-10', done: '2026-10-14', slip_days: 0, deviation_days: 4, overdue_days: null },
  { card: 'C-plan.2', title: '发布', planned: '2026-10-20', plan: '2026-10-24', replans: [{ date: '2026-10-05', from: '2026-10-20', to: '2026-10-24' }],
    done: null, slip_days: 4, deviation_days: null, overdue_days: 2 },
] } }
const evolution: FrameEvolutionRead = { view: 'argument_evolution', evolution: { frame: 'C-map', states: [], done_states: [], omitted: 3, steps: [
  { at: '2026-10-01T01:00:00Z', date: '2026-10-01', card: 'C-map.1', title: '主张', change: 'proposed', by: 'human:local' },
  { at: '2026-10-02T01:00:00Z', date: '2026-10-02', card: 'C-map.2', title: '证据', change: 'linked', other: 'C-map.1', by: 'claude-code/2.1' },
] } }

describe('the views a template declares beside the flow (FR-159)', () => {
  it('lists each card planned against actual, saying late and overdue in words', async () => {
    const reader = { steps: vi.fn(), at: vi.fn(), deviation: vi.fn().mockResolvedValue(deviation) }
    const onClose = vi.fn()
    render(FrameTimeView, { props: { frame: 'C-plan', view: 'plan_deviation', reader, onClose } })
    expect(await screen.findByText('✓ 晚 4 天')).toBeTruthy()
    expect(screen.getByText('▲ 逾期 2 天')).toBeTruthy()
    expect(screen.getByText('2026-10-24（改期 +4 天）')).toBeTruthy()
    expect(reader.deviation).toHaveBeenCalledWith('C-plan')
    await fireEvent.click(screen.getByRole('button', { name: '关闭' }))
    expect(onClose).toHaveBeenCalled()
  })

  it('lists the newest change first and says how many older ones were left out', async () => {
    const reader = { steps: vi.fn(), at: vi.fn(), evolution: vi.fn().mockResolvedValue(evolution) }
    render(FrameTimeView, { props: { frame: 'C-map', view: 'argument_evolution', reader, onClose: vi.fn() } })
    expect(await screen.findByText('只列最近的 2 步，更早的 3 步略去。')).toBeTruthy()
    const rows = screen.getAllByRole('row').slice(1).map((row) => row.textContent)
    expect(rows).toEqual(['2026-10-02证据连线↔ C-map.1claude-code/2.1', '2026-10-01主张提出human:local'])
  })

  it('shows the error when the read fails', async () => {
    const reader = { steps: vi.fn(), at: vi.fn(), evolution: vi.fn().mockRejectedValue(new Error('boom')) }
    render(FrameTimeView, { props: { frame: 'C-map', view: 'argument_evolution', reader, onClose: vi.fn() } })
    expect((await screen.findByRole('alert')).textContent).toContain('读取演变失败')
  })
})
