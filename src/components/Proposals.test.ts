import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import { afterEach, expect, it, vi } from 'vitest'
import Proposals from './Proposals.svelte'
import type { ProjectClient, ProposalEntry, PublicationPreview } from '../lib/api'

afterEach(cleanup)
const id = 'P-019a0000-0000-7000-8000-000000000001'
const entry: ProposalEntry = { type: 'Proposal', id, status: 'submitted', event: 'submitted',
  proposal: { id, project_id: 'project', source_revision: 'a'.repeat(40), baseline_tag: 'towermd/B0',
    draft_id: 'd-reviewed', source_proposal_id: null, source_commit: null, kind: 'module',
    reason: '分离存储职责', alternatives: ['保留现状'], recommendation: '拆分模块',
    impact: { intents: [], modules: ['A', 'B'], interfaces: ['IF-1'], tasks: ['T-1'], paths: ['src/storage/'] },
    changes: [{ path: 'docs/arch/A.md', before_oid: 'old', after_oid: 'new', before: '原职责', after: '新职责' }],
    content_hash: 'b'.repeat(64) } }
const preview: PublicationPreview = { request_id: '019a0000-0000-7000-8000-000000000002',
  draft_id: 'd-reviewed', project_id: 'project', source_revision: 'a'.repeat(40), target: 'B1',
  prepared_at: '2026-10-21T00:00:00Z', content_hash: 'c'.repeat(64) }
function client(): ProjectClient {
  return { chooseProject: vi.fn(), loadProject: vi.fn(), startScan: vi.fn(), pollScan: vi.fn(), cancelScan: vi.fn(),
    listProposals: vi.fn().mockResolvedValue({ proposals: [entry] }),
    prepareProposal: vi.fn().mockResolvedValue(preview), approveProposal: vi.fn(), rejectProposal: vi.fn() }
}

it('uses the nested proposal original and requires a separate publication confirmation', async () => {
  const api = client()
  render(Proposals, { client: api, token: 'token', visible: true,
    onBlocked: vi.fn(), onClose: vi.fn(), onComplete: vi.fn(), onViewDraft: vi.fn() })
  await fireEvent.click(await screen.findByRole('button', { name: /分离存储职责/ }))
  expect(screen.getByText('原职责')).toBeTruthy()
  expect(screen.getByText('新职责')).toBeTruthy()
  expect(screen.getByText('A、B')).toBeTruthy()
  await fireEvent.click(screen.getByRole('button', { name: '准备批准' }))
  expect(await screen.findByRole('button', { name: '确认批准并出版 B1' })).toBeTruthy()
  expect(api.approveProposal).not.toHaveBeenCalled()
})

it('shows the proposer impact notes only when the proposal carries them', async () => {
  const noted: ProposalEntry = { ...entry, proposal: { ...entry.proposal!, impact_notes: { B: '存储调用改走 A' } } }
  const api = client()
  vi.mocked(api.listProposals!).mockResolvedValue({ proposals: [noted] })
  render(Proposals, { client: api, token: 'token', visible: true,
    onBlocked: vi.fn(), onClose: vi.fn(), onComplete: vi.fn(), onViewDraft: vi.fn() })
  await fireEvent.click(await screen.findByRole('button', { name: /分离存储职责/ }))
  expect(screen.getByRole('heading', { name: '影响说明' })).toBeTruthy()
  expect(screen.getByText('存储调用改走 A')).toBeTruthy()
  cleanup()
  render(Proposals, { client: client(), token: 'token', visible: true,
    onBlocked: vi.fn(), onClose: vi.fn(), onComplete: vi.fn(), onViewDraft: vi.fn() })
  await fireEvent.click(await screen.findByRole('button', { name: /分离存储职责/ }))
  expect(screen.queryByRole('heading', { name: '影响说明' })).toBeNull()
})

it('retains the same proposal approval when the publication reply is unknown', async () => {
  const api = client(), blocked = vi.fn()
  vi.mocked(api.approveProposal!).mockRejectedValue(new Error('结果未知'))
  render(Proposals, { client: api, token: 'token', visible: true,
    onBlocked: blocked, onClose: vi.fn(), onComplete: vi.fn(), onViewDraft: vi.fn() })
  await fireEvent.click(await screen.findByRole('button', { name: /分离存储职责/ }))
  await fireEvent.click(screen.getByRole('button', { name: '准备批准' }))
  await fireEvent.click(await screen.findByRole('button', { name: '确认批准并出版 B1' }))
  await screen.findByText('Error: 结果未知')
  await fireEvent.click(screen.getByRole('button', { name: '重试原提案确认' }))
  await screen.findByText('Error: 结果未知')
  const expected = ['token', id, { request_id: preview.request_id, target: 'B1', prepared_at: preview.prepared_at, content_hash: preview.content_hash }]
  expect(vi.mocked(api.approveProposal!).mock.calls).toEqual([expected, expected])
  expect(blocked).toHaveBeenLastCalledWith(true)
  expect(screen.getByRole('button', { name: /分离存储职责.*待审阅/ })).toBeTruthy()
  expect(screen.getByRole<HTMLButtonElement>('button', { name: '刷新提案' }).disabled).toBe(true)
})


it.each(['approve', 'reject'] as const)('updates the accepted %s result in the list and prevents acting again after switching proposals', async (operation) => {
  const api = client(), complete = vi.fn(), blocked = vi.fn()
  const other: ProposalEntry = { ...entry, id: 'P-other', proposal: { ...entry.proposal!, id: 'P-other', reason: '另一提案' } }
  vi.mocked(api.listProposals!).mockResolvedValue({ proposals: [entry, other] })
  vi.mocked(api.approveProposal!).mockResolvedValue({
    target: preview.target, content_hash: preview.content_hash, tag: 'towermd/B1',
    commit: 'd'.repeat(40), snapshot_commit: 'e'.repeat(40),
  })
  vi.mocked(api.rejectProposal!).mockImplementation(async (_token, _id, input) => ({
    ...entry, status: 'rejected', rejection: { eid: input.request_id, reason: input.reason },
  }))
  render(Proposals, { client: api, token: 'token', visible: true,
    onBlocked: blocked, onClose: vi.fn(), onComplete: complete, onViewDraft: vi.fn() })
  await fireEvent.click(await screen.findByRole('button', { name: /分离存储职责/ }))
  if (operation === 'approve') {
    await fireEvent.click(screen.getByRole('button', { name: '准备批准' }))
    await fireEvent.click(await screen.findByRole('button', { name: '确认批准并出版 B1' }))
    await screen.findByText(/已出版 towermd\/B1/)
  } else {
    await fireEvent.click(screen.getByRole('button', { name: '驳回' }))
    await fireEvent.input(screen.getByLabelText('驳回理由'), { target: { value: '保持现有职责' } })
    await fireEvent.click(screen.getByRole('button', { name: '确认驳回提案' }))
    await screen.findByText('已驳回提案并记录理由。')
  }
  const label = operation === 'approve' ? '已批准' : '已驳回'
  expect(screen.getByRole('button', { name: new RegExp(`分离存储职责.*${label}`) })).toBeTruthy()
  expect(screen.queryByRole('button', { name: /分离存储职责.*待审阅/ })).toBeNull()
  expect(complete).toHaveBeenCalledTimes(1)
  expect(blocked).toHaveBeenLastCalledWith(false)
  await fireEvent.click(screen.getByRole('button', { name: /另一提案.*待审阅/ }))
  expect(screen.getByRole('button', { name: '准备批准' })).toBeTruthy()
  await fireEvent.click(screen.getByRole('button', { name: new RegExp(`分离存储职责.*${label}`) }))
  expect(screen.queryByRole('button', { name: '准备批准' })).toBeNull()
  expect(screen.queryByRole('button', { name: '驳回' })).toBeNull()
  expect(api.listProposals).toHaveBeenCalledTimes(1)
  expect(operation === 'approve' ? api.approveProposal : api.rejectProposal).toHaveBeenCalledTimes(1)
})

it('marks what a proposal would weaken and says nothing when it weakens nothing', async () => {
  const api = client()
  const weakened: ProposalEntry = { ...entry, weakening: [{ change: 'check_removed', text: '删除检查「cargo test」' }] }
  vi.mocked(api.listProposals!).mockResolvedValue({ proposals: [weakened] })
  render(Proposals, { client: api, token: 'token', visible: true,
    onBlocked: vi.fn(), onClose: vi.fn(), onComplete: vi.fn(), onViewDraft: vi.fn() })
  await fireEvent.click(await screen.findByRole('button', { name: /分离存储职责/ }))
  const section = screen.getByRole('region', { name: '这是弱化' })
  expect(section.textContent).toContain('删除检查「cargo test」')
  cleanup()
  vi.mocked(api.listProposals!).mockResolvedValue({ proposals: [{ ...entry, weakening: [] }] })
  render(Proposals, { client: api, token: 'token', visible: true,
    onBlocked: vi.fn(), onClose: vi.fn(), onComplete: vi.fn(), onViewDraft: vi.fn() })
  await fireEvent.click(await screen.findByRole('button', { name: /分离存储职责/ }))
  expect(screen.queryByRole('region', { name: '这是弱化' })).toBeNull()
})
