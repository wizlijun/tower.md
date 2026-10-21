import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import { afterEach, expect, it, vi } from 'vitest'
import MainResolution from './MainResolution.svelte'
import type { MainResolutionPreview, ProjectClient, ProposalEntry } from '../lib/api'

afterEach(cleanup)
const commit = 'a'.repeat(40)
const preview: MainResolutionPreview = {
  version: 1, request_id: '019a0000-0000-7000-8000-000000000001', operation: 'claim',
  project_id: 'project', docs_dir: 'docs/tower', import_anchor: 'b'.repeat(40),
  main_revision: commit, target_commit: commit, parent_commit: 'b'.repeat(40),
  changes: [{ path: 'docs/tower/arch/A.md', status: 'M', old_mode: '100644', new_mode: '100644', old_oid: 'c'.repeat(40), new_oid: 'd'.repeat(40) }],
  diff: '-原责任\n+新责任', content_hash: 'e'.repeat(64),
}
function client(): ProjectClient {
  return { chooseProject: vi.fn(), loadProject: vi.fn(), startScan: vi.fn(), pollScan: vi.fn(), cancelScan: vi.fn(),
    prepareMainResolution: vi.fn().mockResolvedValue(preview), confirmMainResolution: vi.fn() }
}

it('shows the exact diff and waits for a separate explicit confirmation', async () => {
  const api = client()
  render(MainResolution, { client: api, token: 'token', commit, visible: true,
    onBlocked: vi.fn(), onClose: vi.fn(), onComplete: vi.fn() })
  await fireEvent.click(screen.getByRole('button', { name: '认领此提交' }))
  expect((await screen.findByText(/原责任/)).textContent).toBe('-原责任\n+新责任')
  expect(api.confirmMainResolution).not.toHaveBeenCalled()
  expect(screen.getByRole('button', { name: '确认认领该提交' })).toBeTruthy()
})

it('keeps the original request after an unknown result, even when a retry is rejected', async () => {
  const api = client(), blocked = vi.fn()
  vi.mocked(api.confirmMainResolution!).mockRejectedValueOnce(new Error('连接中断'))
    .mockRejectedValueOnce(new Error('操作已拒绝：暂不可用'))
  const ui = render(MainResolution, { client: api, token: 'token', commit, visible: true,
    onBlocked: blocked, onClose: vi.fn(), onComplete: vi.fn() })
  await fireEvent.click(screen.getByRole('button', { name: '认领此提交' }))
  await fireEvent.click(await screen.findByRole('button', { name: '确认认领该提交' }))
  await screen.findByText('Error: 连接中断')
  await ui.rerender({ visible: false })
  await ui.rerender({ visible: true })
  await fireEvent.click(screen.getByRole('button', { name: '重试原确认' }))
  await screen.findByText('Error: 操作已拒绝：暂不可用')
  await waitFor(() => expect(api.confirmMainResolution).toHaveBeenCalledTimes(2))
  expect(vi.mocked(api.confirmMainResolution!).mock.calls[0]).toEqual(['token', preview])
  expect(vi.mocked(api.confirmMainResolution!).mock.calls[1]).toEqual(['token', preview])
  expect(blocked).toHaveBeenLastCalledWith(true)
  expect(screen.getByRole<HTMLButtonElement>('button', { name: '生成撤销提交' }).disabled).toBe(true)
})


const captured: ProposalEntry = {
  type: 'Proposal', id: 'P-captured', status: 'draft', event: 'captured-event',
  proposal: {
    id: 'P-captured', project_id: 'project', source_revision: commit, baseline_tag: 'towermd/B0',
    draft_id: null, source_proposal_id: null, source_commit: commit, kind: 'other',
    reason: '捕获受保护变更', alternatives: ['撤销'], recommendation: '审阅实际差异',
    impact: { intents: [], modules: ['A'], interfaces: [], tasks: [], paths: ['docs/tower/arch/A.md'] },
    changes: [], content_hash: 'f'.repeat(64),
  },
}

it('keeps capture separate from resolution and permits a confirmed revert after reopening', async () => {
  const api = client(), complete = vi.fn(), blocked = vi.fn()
  api.captureMainProposal = vi.fn().mockResolvedValue(captured)
  const revert = { ...preview, operation: 'revert' as const }
  vi.mocked(api.prepareMainResolution!).mockResolvedValue(revert)
  vi.mocked(api.confirmMainResolution!).mockResolvedValue({
    request_id: revert.request_id, event: revert.request_id, seq: 45, operation: 'revert',
    target_commit: commit, resolution_commit: 'f'.repeat(40), snapshot_commit: 'e'.repeat(40),
  })
  const ui = render(MainResolution, { client: api, token: 'token', commit, visible: true,
    onBlocked: blocked, onClose: vi.fn(), onComplete: complete })
  await fireEvent.click(screen.getByRole('button', { name: '转为提案' }))
  await screen.findByText('已保存提案 P-captured。')
  expect(screen.getByText(/捕获不表示批准，也不会解除原禁止/)).toBeTruthy()
  expect(api.confirmMainResolution).not.toHaveBeenCalled()
  expect(screen.queryByRole('button', { name: '转为提案' })).toBeNull()
  await ui.rerender({ visible: false })
  await ui.rerender({ visible: true })
  expect(screen.getByRole<HTMLButtonElement>('button', { name: '认领此提交' }).disabled).toBe(false)
  await fireEvent.click(screen.getByRole('button', { name: '生成撤销提交' }))
  await fireEvent.click(await screen.findByRole('button', { name: '确认生成撤销提交' }))
  await screen.findByText('已撤销此提交。')
  expect(screen.queryByText(/不会解除原禁止/)).toBeNull()
  expect(screen.queryByRole('button', { name: '生成撤销提交' })).toBeNull()
  expect(api.captureMainProposal).toHaveBeenCalledTimes(1)
  expect(api.prepareMainResolution).toHaveBeenCalledWith('token', 'revert', commit)
  expect(api.confirmMainResolution).toHaveBeenCalledWith('token', revert)
  expect(complete.mock.calls).toEqual([[], [45]])
  expect(blocked).toHaveBeenLastCalledWith(false)
})

it('cannot switch to claim or revert while the original capture result is unknown', async () => {
  const api = client(), blocked = vi.fn(), complete = vi.fn()
  api.captureMainProposal = vi.fn().mockRejectedValueOnce(new Error('连接中断'))
    .mockRejectedValueOnce(new Error('操作已拒绝：暂不可用')).mockResolvedValueOnce(captured)
  const ui = render(MainResolution, { client: api, token: 'token', commit, visible: true,
    onBlocked: blocked, onClose: vi.fn(), onComplete: complete })
  await fireEvent.click(screen.getByRole('button', { name: '转为提案' }))
  await screen.findByText('Error: 连接中断')
  await ui.rerender({ visible: false })
  await ui.rerender({ visible: true })
  await fireEvent.click(screen.getByRole('button', { name: '重试原提案捕获' }))
  await screen.findByText('Error: 操作已拒绝：暂不可用')
  expect(screen.getByRole<HTMLButtonElement>('button', { name: '认领此提交' }).disabled).toBe(true)
  expect(screen.getByRole<HTMLButtonElement>('button', { name: '生成撤销提交' }).disabled).toBe(true)
  expect(screen.getByRole<HTMLButtonElement>('button', { name: '转为提案' }).disabled).toBe(true)
  expect(api.prepareMainResolution).not.toHaveBeenCalled()
  expect(api.confirmMainResolution).not.toHaveBeenCalled()
  expect(complete).not.toHaveBeenCalled()
  expect(blocked).toHaveBeenLastCalledWith(true)
  await fireEvent.click(screen.getByRole('button', { name: '重试原提案捕获' }))
  await screen.findByText('已保存提案 P-captured。')
  expect(vi.mocked(api.captureMainProposal).mock.calls).toEqual([
    ['token', commit], ['token', commit], ['token', commit],
  ])
  expect(screen.getByRole<HTMLButtonElement>('button', { name: '生成撤销提交' }).disabled).toBe(false)
  expect(blocked).toHaveBeenLastCalledWith(false)
})
