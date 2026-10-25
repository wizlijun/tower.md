import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import AskPanel from './AskPanel.svelte'
import type { GenericRequestView } from '../lib/api'
import type { DelegationControls } from '../lib/delegation'
import { delegationView } from '../test/delegation'
import { selectValue } from '../test/select'

afterEach(cleanup)

const mark = (eid: string) => ({ at: '2026-10-23T01:00:00Z', eid, seq: 1, actor: 'human:local' })
function request(id: string, extra: Partial<GenericRequestView> = {}): GenericRequestView {
  return { id, event: mark(id), data: { target: 'mod-a', ask: 'question', text: `问题 ${id}`, due: '2026-10-24T01:00:00Z', to: [] },
    open: true, recipients: [], silence: null, ...extra }
}
function controls(requests: GenericRequestView[], extra: Partial<DelegationControls> = {}): DelegationControls {
  return { view: delegationView({ requests }), disabled: false, busy: false, pending: null, error: null, result: null, ruleOffer: null,
    onAction: vi.fn(), onRetry: vi.fn(), onDismissRule: vi.fn(), ...extra }
}

describe('asking about a node (FR-116, FR-127)', () => {
  it('sends a question with a local deadline and never names the recipients', async () => {
    const c = controls([])
    render(AskPanel, { props: { target: 'mod-a', controls: c, now: () => new Date(2026, 9, 23, 9, 30) } })
    const send = screen.getByRole('button', { name: '发出' }) as HTMLButtonElement
    expect(send.disabled).toBe(true)
    await fireEvent.input(screen.getByLabelText('内容'), { target: { value: '  为什么拆成两层？ ' } })
    await selectValue(screen.getByLabelText('期限'), '4')
    await fireEvent.click(send)
    expect(c.onAction).toHaveBeenCalledTimes(1)
    const intent = vi.mocked(c.onAction).mock.calls[0][0]
    expect(intent).toMatchObject({ kind: 'ask', data: { target: 'mod-a', ask: 'question', text: '为什么拆成两层？' } })
    expect(intent.kind === 'ask' ? intent.data.due.slice(0, 19) : '').toBe('2026-10-23T13:30:00')
    expect(Object.keys(intent.kind === 'ask' ? intent.data : {})).not.toContain('to')
  })

  it('shows answers with their refs, queued and silent requests on this node only, and withdraws open ones', async () => {
    const reply = { event: mark('r'), data: { from: 'claude-code/2.1', state: 'in_progress' as const, summary: '为了隔离慢层', refs: ['docs/specs/x.md'] } }
    const c = controls([
      request('q-answered', { open: false, reply, data: { target: 'mod-a', ask: 'question', text: '为何？', due: '2026-10-24T01:00:00Z', to: ['claude-code/2.1'] } }),
      request('q-queued'),
      request('q-silent', { recipients: ['codex/1.0'], silence: mark('s'), data: { target: 'mod-a', ask: 'report', text: '进度？', due: '2026-10-24T01:00:00Z', to: ['codex/1.0'] } }),
      request('q-other', { data: { target: 'mod-b', ask: 'question', text: '别处', due: '2026-10-24T01:00:00Z', to: [] } }),
    ])
    render(AskPanel, { props: { target: 'mod-a', controls: c } })
    const rows = within(screen.getByRole('list', { name: '本节点的请求' })).getAllByRole('listitem')
    expect(rows).toHaveLength(3)
    expect(rows[0].textContent).toContain('✓ 已回答 · claude-code/2.1：为了隔离慢层')
    expect(rows[0].textContent).toContain('依据：docs/specs/x.md')
    expect(within(rows[0]).queryByRole('button')).toBeNull()
    expect(rows[1].textContent).toContain('∅ 无人可答')
    expect(rows[2].textContent).toContain('◷ 失联 · 等待 codex/1.0 回答')
    expect(rows[2].textContent).toContain('要求汇报')
    await fireEvent.click(within(rows[1]).getByRole('button', { name: '撤回请求' }))
    expect(c.onAction).toHaveBeenCalledWith({ kind: 'withdraw', object: 'q-queued', data: {} })
  })

  it('records an answered question as a decision once, only when the App offers it (FR-127)', async () => {
    const reply = { event: mark('r'), data: { from: 'claude-code/2.1', state: 'waiting' as const, summary: '统一用 JSON' } }
    const answered = (id: string) => request(id, { open: false, reply })
    const c = controls([answered('q-1'), answered('q-2'), request('q-open')])
    render(AskPanel, { props: { target: 'mod-a', controls: c } })
    expect(screen.queryByRole('button', { name: '记为决策' })).toBeNull()
    cleanup()
    render(AskPanel, { props: { target: 'mod-a', controls: c, decided: new Set(['q-2']) } })
    const rows = within(screen.getByRole('list', { name: '本节点的请求' })).getAllByRole('listitem')
    expect(rows[1].textContent).toContain('已记为决策')
    expect(within(rows[1]).queryByRole('button', { name: '记为决策' })).toBeNull()
    expect(within(rows[2]).queryByRole('button', { name: '记为决策' })).toBeNull()
    const record = within(rows[0]).getByRole('button', { name: '记为决策' }) as HTMLButtonElement
    expect(record.disabled).toBe(true)
    await fireEvent.input(within(rows[0]).getByLabelText('决策'), { target: { value: ' 导出统一用 JSON ' } })
    await fireEvent.click(record)
    expect(c.onAction).toHaveBeenCalledWith({ kind: 'decision', data: { text: '导出统一用 JSON', source: { kind: 'reply', request: 'q-1' } } })
  })

  it('locks the form while an earlier submission is unconfirmed', () => {
    render(AskPanel, { props: { target: 'mod-a', controls: controls([], { pending: { kind: 'ask', input: { request_id: 'x', data: { target: 'mod-a', ask: 'question', text: 't', due: 'd' } } } }), showStatus: true } })
    expect((screen.getByLabelText('内容') as HTMLTextAreaElement).disabled).toBe(true)
    expect(screen.getByText(/提问：提交结果尚待确认/)).toBeTruthy()
  })

  it('offers "update by its rules" only for a frame (FR-156)', async () => {
    render(AskPanel, { props: { target: 'mod-a', controls: controls([]) } })
    expect(screen.queryByRole('option', { name: '按规则更新' })).toBeNull()
    cleanup()
    const c = controls([])
    render(AskPanel, { props: { target: 'C-reading1', controls: c } })
    await selectValue(screen.getByLabelText('类别'), 'update')
    await fireEvent.input(screen.getByLabelText('内容'), { target: { value: '按规则补齐本周' } })
    await fireEvent.click(screen.getByRole('button', { name: '发出' }))
    expect(c.onAction).toHaveBeenCalledWith(expect.objectContaining({ kind: 'ask', data: expect.objectContaining({ target: 'C-reading1', ask: 'update' }) }))
  })
})
