import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import AcceptanceCard from './AcceptanceCard.svelte'
import { delegated } from '../test/delegation'

afterEach(cleanup)

function timer(ms = 4200) {
  return { elapsed: vi.fn(() => ms), dispose: vi.fn() }
}

describe('acceptance card (FR-202, FR-208, FR-209)', () => {
  it('labels each evidence source by shape and text, and marks rows with no evidence as unproven', () => {
    const object = delegated()
    render(AcceptanceCard, { object, card: object.card!, onAction: vi.fn(), timer: timer() })
    const rows = screen.getAllByRole('listitem').filter((item) => item.classList.contains('acceptance-row'))
    expect(rows).toHaveLength(4)
    expect(rows[0].textContent).toContain('■ 检查')
    expect(rows[0].textContent).toContain('通过')
    expect(rows[1].textContent).toContain('○ 受托者自述')
    expect(rows[2].textContent).toContain('? 未证实')
    expect(rows[3].textContent).toContain('整体证据')
    expect(screen.getByText('验收时长只计 App 内')).toBeTruthy()
  })

  it('confirms with the in-App review time, the card\'s unproven list and an optional takeaway', async () => {
    const object = delegated(), onAction = vi.fn(), clock = timer()
    render(AcceptanceCard, { object, card: object.card!, onAction, timer: clock })
    await fireEvent.input(screen.getByLabelText('这次改变了什么看法'), { target: { value: ' 自述也要核 ' } })
    await fireEvent.click(screen.getByRole('button', { name: '已验收' }))
    expect(onAction).toHaveBeenCalledWith({ kind: 'confirm', object: 'T-1', data: { review_ms: 4200, unproven: [2], takeaway: '自述也要核' } })
  })

  it('sends back only with a reason, and records "I used it" against chosen criteria', async () => {
    const object = delegated(), onAction = vi.fn()
    render(AcceptanceCard, { object, card: object.card!, onAction, timer: timer(10) })
    await fireEvent.click(screen.getByRole('button', { name: '退回' }))
    const confirm = screen.getByRole('button', { name: '确认退回' }) as HTMLButtonElement
    expect(confirm.disabled).toBe(true)
    await fireEvent.input(screen.getByLabelText('退回理由'), { target: { value: '接口变了' } })
    await fireEvent.click(confirm)
    expect(onAction).toHaveBeenLastCalledWith({ kind: 'send_back', object: 'T-1', data: { reason: '接口变了', review_ms: 10 } })
    const used = screen.getByText('我用过', { selector: 'summary' }).closest('details')!
    await fireEvent.click(within(used).getByLabelText('3. 不改接口'))
    await fireEvent.click(within(used).getByLabelText('1. 测试通过'))
    await fireEvent.click(within(used).getByRole('button', { name: '我用过' }))
    expect(onAction).toHaveBeenLastCalledWith({ kind: 'used', object: 'T-1', data: { criteria: [0, 2] } })
  })

  it('offers no judgement while only claimed done, and says so', () => {
    const object = delegated({ phase: 'claimed_done' })
    const card = { ...object.card!, can_judge: false }
    render(AcceptanceCard, { object, card, onAction: vi.fn(), timer: timer() })
    expect(screen.getByText(/已声称、未确认/)).toBeTruthy()
    expect(screen.queryByRole('button', { name: '已验收' })).toBeNull()
    expect(screen.queryByRole('button', { name: '退回' })).toBeNull()
  })

  it('names the action "确认完成" where a person completes the object', () => {
    const object = delegated({ phase: 'claimed_done', human_can_complete: true })
    render(AcceptanceCard, { object, card: object.card!, onAction: vi.fn(), timer: timer() })
    expect(screen.getByRole('button', { name: '确认完成' })).toBeTruthy()
  })
})
