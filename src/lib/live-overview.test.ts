import { afterEach, expect, test, vi } from 'vitest';
import { attentionItems, unresolvedItems, observeProgress, overviewFlow, taskEvents, taskFlow } from './live-overview';
import type { TaskProgressClient, TaskProgressView } from './task-progress';
import type { ProjectOverview } from './project';

const fixture = (): TaskProgressView => ({ project_id: 'project', common_dir: '/project/.git', revision: 'a'.repeat(40), captured_at: 1800000000000, through_seq: 12, floor_seq: 0, state: 'recorded', source: { kind: 'hub_journal' }, tasks: [{ type: 'Task', id: 'T', title: '接通真实流程', module: 'A', state: 'verifying' }], runs: [{ type: 'Run', id: 'R', task: 'T', state: 'verifying', cost_usd: null }], evidence: [{ type: 'Evidence', id: 'E', task: 'T', run: 'R', result: 'passed' }], holds: [], events: [], diagnostics: [] });
const deferred = <T>() => { let resolve!: (value: T) => void; const promise = new Promise<T>(done => resolve = done); return { promise, resolve }; };
afterEach(() => vi.useRealTimers());

test('关注按实际未决中断排序，已答/已决不继续索要放行，不合成中断', () => {
  const view = fixture();
  view.holds = [
    { id: 'question', body: { kind: 'ask', question: '该用哪条接口？' }, recipients: [{ task: 'T' }], opened_ms: 1, reply: null, attention: true },
    { id: 'review', body: { kind: 'review', binding: { task: 'T' } }, opened_ms: 2, reply: null },
    { id: 'gate', kind: 'gatefailed', task: 'T', gate: 'G3' },
    { id: 'answered', body: { kind: 'ask' }, reply: { text: '已回答' } },
    { id: 'reviewed', body: { kind: 'review', verdict: 'approved' }, reply: null },
    { id: 'unknown', body: { kind: 'unknown_future_hold' } },
  ];
  const items = attentionItems(view);
  expect(items.map(item => item.id)).toEqual(['gate', 'review', 'question']);
  expect(items[2].tasks).toEqual(['T']);
  expect(items[0].label).toBe('门禁失败');
  view.holds.push({ id: 'fresh-question', body: { kind: 'ask', question: '刚刚开立' }, recipients: [{ task: 'T' }], opened_ms: 3, reply: null, attention: false });
  expect(attentionItems(view).some(item => item.id === 'fresh-question')).toBe(false);
  expect(unresolvedItems(view).some(item => item.id === 'fresh-question')).toBe(true);
  expect(taskFlow(view.tasks, new Map(), view).nodes[0].data.text).toContain('4 项中断');
  view.tasks.push({ type: 'Task', id: 'blocked', module: 'B', state: 'blocked' });
  expect(attentionItems(view).at(-1)?.kind).toBe('module');
});

test('快层变化更新模块摘要与任务中断但保留基线和既有任务坐标', () => {
  const view = fixture();
  const overview: ProjectOverview = { source: 'baseline', revision: view.revision, title: '项目', diagnostics: [], edges: [], nodes: [{ id: 'module-card', obj: 'A', objectType: 'Module', title: '模块 A', kind: 'text', status: 'bound', text: '原文', x: -123, y: 47, width: 300, height: 180 }] };
  const before = overviewFlow(overview, view);
  expect(before.nodes[0].data.text).toContain('0/1 已合并');
  view.tasks[0].state = 'merged';
  const after = overviewFlow(overview, view);
  expect(after.nodes[0].position).toEqual(before.nodes[0].position);
  expect(after.nodes[0].data.text).toContain('1/1 已合并');
  const mismatched = overviewFlow(overview, { ...view, revision: 'b'.repeat(40) });
  expect(mismatched.nodes[0].data.text).toBe('原文');
  expect(mismatched.nodes[0].class).toBeUndefined();
  const positions = new Map<string, { x: number; y: number }>();
  const tasks = taskFlow(view.tasks, positions);
  const updated = taskFlow([{ type: 'Task', id: 'NEW', state: 'ready' }, ...view.tasks], positions);
  expect(updated.nodes.find(node => node.id === 'T')?.position).toEqual(tasks.nodes[0].position);
});

test('Z2纳入已回答问题的原始事件与Run，排除无关任务', () => {
  const view = fixture();
  view.holds = [{ id: 'answered', body: { kind: 'ask' }, recipients: [{ task: 'T' }], reply: { text: '已回答' } }];
  view.events = [
    { seq: 1, eid: 'claim', obj: 'T', type: 'task.claimed', data: {}, t: '2026-10-19T00:00:00Z' },
    { seq: 2, eid: 'answer', obj: 'answered', type: 'hold.replied', data: {}, t: '2026-10-19T00:01:00Z' },
    { seq: 3, eid: 'other', obj: 'OTHER', type: 'task.state', data: {}, t: '2026-10-19T00:02:00Z' },
    { seq: 4, eid: 'null-data', obj: 'T', type: 'future.event', data: null, t: '2026-10-19T00:03:00Z' },
    { seq: 5, eid: 'primitive-data', obj: 'OTHER', type: 'future.event', data: 3, t: '2026-10-19T00:04:00Z' },
  ];
  view.events.push({ seq: 6, eid: 'prompt-actual', obj: 'H-global', type: 'human.prompt', data: { scope: { kind: 'project' }, text: '实际人工原文', hold: { recipients: [{ run: 'R', task: 'T' }] } }, t: '2026-10-19T00:05:00Z' });
  view.events.push({ seq: 7, eid: 'prompt-other', obj: 'H-other', type: 'human.prompt', data: { scope: { kind: 'project' }, text: '另一次人工输入', hold: { recipients: [{ run: 'OTHER', task: 'OTHER' }] } }, t: '2026-10-19T00:06:00Z' });
  expect(taskEvents(view, 'T').map(event => event.eid)).toEqual(['claim', 'answer', 'null-data', 'prompt-actual']);
});

test('自动读取并从真实水位订阅，事件风暴合并一次读取，关闭后释放订阅', async () => {
  vi.useFakeTimers(); let changed!: () => void; const stop = vi.fn(); const delivered: TaskProgressView[] = [];
  const client: TaskProgressClient = { load: vi.fn(async () => fixture()), subscribe: vi.fn(async (_token, after, onChange) => { expect(after).toBe(12); changed = () => onChange({ kind: 'changed', through_seq: 13, floor_seq: 0, revision: 'a'.repeat(40) }); return stop; }) };
  const observer = observeProgress('selected', client, value => delivered.push(value), () => {});
  await vi.advanceTimersByTimeAsync(0);
  expect(client.load).toHaveBeenCalledTimes(1);
  for (let i = 0; i < 20; i++) changed();
  await vi.advanceTimersByTimeAsync(100);
  expect(client.load).toHaveBeenCalledTimes(2);
  expect(delivered).toHaveLength(2);
  observer.close(); expect(stop).toHaveBeenCalledTimes(1);
  changed(); await vi.advanceTimersByTimeAsync(5000); expect(client.load).toHaveBeenCalledTimes(2);
});

test('切换项目后旧读取/旧异步订阅不发布且确实关闭', async () => {
  const first = deferred<TaskProgressView>(); const oldViews = vi.fn();
  const old = observeProgress('old', { load: () => first.promise }, oldViews, () => {});
  old.close(); first.resolve(fixture()); await Promise.resolve(); await Promise.resolve(); expect(oldViews).not.toHaveBeenCalled();
  const pending = deferred<() => void>(); const stop = vi.fn();
  const active = observeProgress('old', { load: async () => fixture(), subscribe: () => pending.promise }, () => {}, () => {});
  await Promise.resolve(); active.close(); pending.resolve(stop); await Promise.resolve(); await Promise.resolve(); expect(stop).toHaveBeenCalledTimes(1);
});

test('断线保留历史并有界重连，订阅建立前到达的错误不能冒充已连接', async () => {
  vi.useFakeTimers(); const states: string[] = [];
  const stop = vi.fn();
  const client: TaskProgressClient = { load: vi.fn(async () => fixture()), subscribe: vi.fn(async (_token, _after, _changed, onError) => { onError(new Error('连接已断开')); return stop; }) };
  const observer = observeProgress('selected', client, () => {}, value => states.push(value));
  await vi.advanceTimersByTimeAsync(100000);
  expect(states).not.toContain('live'); expect(states).toContain('disconnected');
  expect(client.subscribe).toHaveBeenCalledTimes(7); expect(client.load).toHaveBeenCalledTimes(7);
  await vi.advanceTimersByTimeAsync(100000); expect(client.load).toHaveBeenCalledTimes(7);
  observer.close();
});

test('读取期间手动重连请求不丢失，实际daemon身份变化关闭旧订阅', async () => {
  vi.useFakeTimers();
  const pending = deferred<TaskProgressView>(); const stop = vi.fn();
  let reads = 0;
  const client: TaskProgressClient = { load: vi.fn(() => ++reads === 2 ? pending.promise : Promise.resolve({ ...fixture(), daemon_instance: reads > 2 ? 'new-instance' : 'old-instance' })), subscribe: vi.fn(async () => stop) };
  const observer = observeProgress('selected', client, () => {}, () => {});
  await vi.advanceTimersByTimeAsync(0);
  observer.refresh(); observer.refresh();
  pending.resolve({ ...fixture(), daemon_instance: 'old-instance' });
  await vi.advanceTimersByTimeAsync(100);
  expect(client.load).toHaveBeenCalledTimes(3);
  expect(client.subscribe).toHaveBeenCalledTimes(3);
  expect(stop).toHaveBeenCalledTimes(2);
  observer.close();
});
