import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { FrameRules } from '../lib/api'
import { selectValue } from '../test/select'
import SourceStateRule from './SourceStateRule.svelte'

afterEach(cleanup)

const rules: FrameRules = {
  text: '', add_card: 'anyone', add_link: 'anyone', edit_text: 'anyone', change_state: 'anyone', fields: [], transitions: {},
  require_source: false, done_needs_human: true, status_map: { '#blue': 'To Do' },
}

describe('writing an unmapped token into the frame rules (appendix A.2)', () => {
  it('saves the chosen state beside the rules already there and keeps every other rule', async () => {
    const onSave = vi.fn()
    render(SourceStateRule, { props: { tokens: ['#green'], states: ['To Do', 'Done'], rules, onSave } })
    const button = screen.getByRole('button', { name: '写进容器规则' }) as HTMLButtonElement
    expect(button.disabled).toBe(true)
    await selectValue(screen.getByLabelText('#green 当作的状态'), 'Done')
    await fireEvent.click(button)
    expect(onSave).toHaveBeenCalledWith({ ...rules, status_map: { '#blue': 'To Do', '#green': 'Done' } })
  })

  it('shows nothing once every token is in the rules or when there are no states', () => {
    render(SourceStateRule, { props: { tokens: ['#blue'], states: ['To Do'], rules, onSave: vi.fn() } })
    expect(screen.queryByRole('group', { name: '未映射的状态写法' })).toBeNull()
    cleanup()
    render(SourceStateRule, { props: { tokens: ['#green'], states: [], rules, onSave: vi.fn() } })
    expect(screen.queryByRole('group', { name: '未映射的状态写法' })).toBeNull()
  })
})
