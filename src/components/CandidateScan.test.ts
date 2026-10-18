import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, expect, test, vi } from 'vitest';
import CandidateScan from './CandidateScan.svelte';
import type { CandidateClient, ScanStatus } from '../lib/candidate';
import { candidateFixture } from '../lib/candidate.fixture';
afterEach(cleanup);

test('only the explicit click starts a scan; pending waits keep one id and fetch the actual draft', async () => {
  const draft = candidateFixture();
  let release!: (status: ScanStatus) => void;
  const client: CandidateClient = { start: vi.fn(async () => ({ scan_id: draft.scan_id, state: 'queued' })),
    wait: vi.fn().mockImplementationOnce(() => new Promise<ScanStatus>(resolve => { release = resolve; })).mockResolvedValueOnce({ scan_id: draft.scan_id, state: 'completed' }), draft: vi.fn(async () => draft) };
  const onloaded = vi.fn();
  render(CandidateScan, { token: 'selected', client, onloaded });
  expect(client.start).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole('button', { name: '生成候选画布' }));
  await waitFor(() => expect(client.wait).toHaveBeenCalledExactlyOnceWith('selected', draft.scan_id));
  release({ scan_id: draft.scan_id, state: 'pending', diagnostic: '90秒等待结束，后台仍在工作' });
  await screen.findByText('扫描仍在进行');
  expect(client.draft).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole('button', { name: '继续等待' }));
  await waitFor(() => expect(onloaded).toHaveBeenCalledExactlyOnceWith(draft));
  expect(client.start).toHaveBeenCalledTimes(1);
  expect(client.wait).toHaveBeenCalledTimes(2);
  expect(client.draft).toHaveBeenCalledExactlyOnceWith('selected', draft.scan_id);
  expect(screen.getByText('候选画布 · 未批准')).toBeTruthy();
});
test('a destroyed project discards late wait and does not load or rerun it', async () => {
  const draft = candidateFixture(); let release!: (status: ScanStatus) => void;
  const client: CandidateClient = { start: vi.fn(async () => ({ scan_id: draft.scan_id, state: 'queued' })), wait: vi.fn(() => new Promise<ScanStatus>(resolve => { release = resolve; })), draft: vi.fn(async () => draft) };
  const onloaded = vi.fn(); const view = render(CandidateScan, { token: 'old', client, onloaded });
  await fireEvent.click(screen.getByRole('button', { name: '生成候选画布' }));
  await waitFor(() => expect(client.wait).toHaveBeenCalled());
  view.unmount(); release({ scan_id: draft.scan_id, state: 'completed' });
  await Promise.resolve();
  expect(client.draft).not.toHaveBeenCalled(); expect(onloaded).not.toHaveBeenCalled();
});
test('daemon failures remain diagnostics and never turn into a completed or empty graph', async () => {
  const onloaded = vi.fn();
  const client: CandidateClient = { start: vi.fn(async () => { throw { code: 'daemon_incompatible', message: '旧 daemon 缺少 scan 能力', detail: '请检查版本' }; }), wait: vi.fn(), draft: vi.fn() };
  render(CandidateScan, { token: 'selected', client, onloaded });
  await fireEvent.click(screen.getByRole('button', { name: '生成候选画布' }));
  expect((await screen.findByRole('alert')).textContent).toContain('旧 daemon 缺少 scan 能力');
  expect(onloaded).not.toHaveBeenCalled(); expect(client.wait).not.toHaveBeenCalled();
});

test('completed handoff shows the actual scan/source and existing CLI instructions without executing them', async () => {
  const draft = candidateFixture();
  const client: CandidateClient = { start: vi.fn(async () => ({ scan_id: draft.scan_id, state: 'queued' })), wait: vi.fn(async () => ({ scan_id: draft.scan_id, state: 'completed' })), draft: vi.fn(async () => draft) };
  render(CandidateScan, { token: 'selected', client, onloaded: vi.fn() });
  expect(screen.queryByRole('region', { name: '交给现有 Agent 完成 S6' })).toBeNull();
  await fireEvent.click(screen.getByRole('button', { name: '生成候选画布' }));
  const handoff = await screen.findByRole('region', { name: '交给现有 Agent 完成 S6' });
  expect(handoff.textContent).toContain(draft.input.revision);
  expect(handoff.textContent).toContain(`towermd baseline scan --wait ${draft.scan_id}`);
  expect(handoff.textContent).toContain('towermd baseline schema');
  expect(handoff.textContent).toContain('towermd baseline submit input.json');
  expect(handoff.textContent).toContain('现有');
  expect(screen.queryByText(/编辑、保存和批准尚未接入/)).toBeNull();
  expect(client.start).toHaveBeenCalledTimes(1);
  expect(client.wait).toHaveBeenCalledTimes(1);
  expect(client.draft).toHaveBeenCalledTimes(1);
});
