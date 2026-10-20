import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import { afterEach, expect, it, vi } from 'vitest'
import MainResolution from './MainResolution.svelte'
import type { MainResolutionPreview, ProjectClient } from '../lib/api'

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
