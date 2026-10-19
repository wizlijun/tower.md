import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, expect, test, vi } from 'vitest';
import App from './App.svelte';
import type { ProjectOverview } from './lib/project';
import type { ProgressChange, TaskProgressClient, TaskProgressView } from './lib/task-progress';
import { readonlyControlsFixture } from './lib/interventions.fixture';

afterEach(() => { cleanup(); vi.useRealTimers(); });
const oid = (letter: string) => letter.repeat(40);
function overview(letter: string): ProjectOverview {
  return { source: 'baseline', revision: oid(letter), authorityRevision: oid(letter), title: '同步项目', diagnostics: [], edges: [],
    nodes: [{ id: 'module-A', obj: 'A', objectType: 'Module', title: '真实模块 A', kind: 'text', status: 'bound', text: '原模块', x: 25, y: 60, width: 300, height: 180 }] };
}
function progress(letter: string): TaskProgressView {
  return { project_id: 'project', common_dir: '/project/.git', revision: oid(letter), daemon_instance: 'same-daemon', captured_at: 100, through_seq: letter === 'a' ? 1 : letter === 'b' ? 2 : 3, floor_seq: 0,
    state: 'recorded', source: { kind: 'hub_journal' }, tasks: [{ type: 'Task', id: 'T', title: '原 Task', module: 'A', state: 'ready', needs_recheck: letter !== 'a' }], runs: [], evidence: [], holds: [], events: [], diagnostics: [] };
}
async function opened(load: (token: string) => Promise<ProjectOverview>) {
  let current = progress('a'); let notify: ((value: ProgressChange) => void) | undefined;
  const stop = vi.fn();
  const client: TaskProgressClient = { load: vi.fn(async () => current), subscribe: vi.fn(async (_token, _after, changed) => { notify = changed; return stop; }) };
  const rendered = render(App, { choose: async () => ({ token: 'selected', displayPath: '/project', hasConfig: true }), load, taskProgressClient: client, interventionClient: readonlyControlsFixture() });
  await fireEvent.click(screen.getByRole('button', { name: '真实项目入口' }));
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await screen.findByText('权威 main 画布');
  await fireEvent.click(screen.getByRole('button', { name: '查看项目画布' }));
  await waitFor(() => expect(client.subscribe).toHaveBeenCalledTimes(1));
  return { ...rendered, client, stop, setCurrent(letter: string) { current = progress(letter); }, change(letter: string) { current = progress(letter); notify!({ kind: 'changed', through_seq: current.through_seq!, floor_seq: 0, revision: current.revision }); } };
}
async function advance(milliseconds: number) { await act(async () => { await vi.advanceTimersByTimeAsync(milliseconds); }); }

test('App同revision画布读失败后自动恢复，Live实例和原Task坐标保留且精确门禁恢复前不开放复核', async () => {
  const load = vi.fn().mockResolvedValueOnce(overview('a')).mockRejectedValueOnce(new Error('暂时读不到 main')).mockResolvedValue(overview('b'));
  const app = await opened(load);
  await fireEvent.click(within(screen.getByRole('region', { name: '模块状态与下钻' })).getByRole('button', { name: /真实模块 A/ }));
  const node = () => app.container.querySelector<HTMLElement>('.live-flow .svelte-flow__node[data-id="T"]');
  await waitFor(() => expect(node()).not.toBeNull()); const position = { transform: node()!.style.transform, width: node()!.style.width, height: node()!.style.height };
  const workspace = screen.getByRole('region', { name: '真实项目实时态势' });
  await fireEvent.click(within(screen.getByRole('region', { name: '模块真实任务' })).getByRole('button', { name: /原 Task/ }));
  vi.useFakeTimers(); app.change('b'); await advance(100);
  expect(load).toHaveBeenCalledTimes(2); expect(screen.getByRole('alert').textContent).toContain('暂时读不到 main');
  expect(screen.queryByRole('button', { name: '打开复核原件' })).toBeNull();
  await advance(1000);
  expect(load).toHaveBeenCalledTimes(3); expect(screen.getByRole('region', { name: '真实项目实时态势' })).toBe(workspace);
  expect(screen.getByRole('button', { name: '打开复核原件' })).toBeTruthy(); expect(screen.queryByRole('alert')).toBeNull();
  expect(app.client.load).toHaveBeenCalledTimes(2); expect(app.client.subscribe).toHaveBeenCalledTimes(2); expect(app.stop).toHaveBeenCalledTimes(1);
  await fireEvent.click(screen.getByRole('button', { name: 'Z1 A' }));
  expect({ transform: node()!.style.transform, width: node()!.style.width, height: node()!.style.height }).toEqual(position);
  await advance(10000); expect(load).toHaveBeenCalledTimes(3);
});

test('同revision连续失败仅首次加3次自动重试，重读态势不重置预算且人工重试仍可用', async () => {
  const load = vi.fn().mockResolvedValueOnce(overview('a')).mockRejectedValue(new Error('main 暂不可读'));
  const app = await opened(load); vi.useFakeTimers(); app.change('b'); await advance(100);
  await fireEvent.click(screen.getByRole('button', { name: '重新读取态势' })); await advance(0);
  await advance(7000);
  expect(load).toHaveBeenCalledTimes(5); expect(screen.getByRole('alert').textContent).toContain('main 暂不可读');
  expect(screen.queryByRole('button', { name: '打开复核原件' })).toBeNull();
  await advance(60000); expect(load).toHaveBeenCalledTimes(5);
  await fireEvent.click(screen.getByRole('button', { name: '重试同步画布' })); expect(load).toHaveBeenCalledTimes(6);
  app.unmount(); await advance(60000); expect(load).toHaveBeenCalledTimes(6);
});

test('main再次推进时取消旧目标重试，只接受最新精确版本且不重挂Live', async () => {
  const load = vi.fn().mockResolvedValueOnce(overview('a')).mockRejectedValueOnce(new Error('B版本暂不可读')).mockResolvedValue(overview('c'));
  const app = await opened(load); const workspace = screen.getByRole('region', { name: '真实项目实时态势' });
  vi.useFakeTimers(); app.change('b'); await advance(100); expect(load).toHaveBeenCalledTimes(2);
  app.change('c'); await advance(100); expect(load).toHaveBeenCalledTimes(3);
  expect(screen.getByRole('region', { name: '真实项目实时态势' })).toBe(workspace);
  expect(screen.queryByText(/main 已更新，正在同步基线画布/)).toBeNull();
  await advance(10000); expect(load).toHaveBeenCalledTimes(3); expect(app.client.load).toHaveBeenCalledTimes(3);
});

test('overview已读到更晚main时重读真实progress并收敛，不把旧目标定时器继续当新请求', async () => {
  let app: Awaited<ReturnType<typeof opened>>;
  const load = vi.fn().mockResolvedValueOnce(overview('a')).mockImplementation(async () => { app.setCurrent('c'); return overview('c'); });
  app = await opened(load); vi.useFakeTimers(); app.change('b'); await advance(300);
  expect(load).toHaveBeenCalledTimes(2); expect(app.client.load).toHaveBeenCalledTimes(3);
  expect(screen.queryByText(/main 已更新，正在同步基线画布/)).toBeNull();
  await advance(10000); expect(load).toHaveBeenCalledTimes(2);
});

test('首次同步失败后卸载即取消尚未触发的overview重试', async () => {
  const load = vi.fn().mockResolvedValueOnce(overview('a')).mockRejectedValue(new Error('main 暂不可读'));
  const app = await opened(load); vi.useFakeTimers(); app.change('b'); await advance(100);
  expect(load).toHaveBeenCalledTimes(2); app.unmount(); await advance(10000);
  expect(load).toHaveBeenCalledTimes(2);
});
