import { afterEach, expect, test, vi } from 'vitest';
import { attentionItems, unresolvedItems, observeProgress, overviewFlow, taskEvents, taskFlow, moduleSummary } from './live-overview';
import type { TaskProgressClient, TaskProgressView } from './task-progress';
import type { ProjectOverview } from './project';

const fixture = (): TaskProgressView => ({ project_id: 'project', common_dir: '/project/.git', revision: 'a'.repeat(40), captured_at: 1800000000000, through_seq: 12, floor_seq: 0, state: 'recorded', source: { kind: 'hub_journal' }, tasks: [{ type: 'Task', id: 'T', title: '接通真实流程', module: 'A', state: 'verifying' }], runs: [{ type: 'Run', id: 'R', task: 'T', state: 'verifying', cost_usd: null }], evidence: [{ type: 'Evidence', id: 'E', task: 'T', run: 'R', result: 'passed' }], holds: [], events: [], diagnostics: [] });
const deferred = <T>() => { let resolve!: (value: T) => void; const promise = new Promise<T>(done => resolve = done); return { promise, resolve }; };
afterEach(() => vi.useRealTimers());

const verification = (id: string, result: string, module = 'A') => ({ type: 'Evidence' as const, id, kind: 'test', module, task: 'T', run: 'R', result, generated_by: 'process:towermd-verifier' });
const verificationEvent = (seq: number, evidence: ReturnType<typeof verification>, type = 'finish.completed') => ({ seq, eid: `event-${seq}`, obj: type === 'evidence.recorded' ? evidence.id : 'T', type, data: { evidence }, t: '2026-10-20T00:00:00Z' });

test('模块最近验证按真实事件seq选择，不按Evidence数组、ID或自述完成排序', () => {
  const view = fixture();
  const older = verification('E-z', 'passed'), latest = verification('E-a', 'failed');
  view.evidence = [latest, older];
  view.events = [verificationEvent(8, latest), verificationEvent(3, older)];
  view.runs[0].self_reported = { result: 'passed', done: true };
  const before = JSON.stringify(view);
  expect(moduleSummary(view, 'A')).toContain('最近验证（当前事件窗口）：验证失败 · E-a');
  expect(moduleSummary(view, 'A')).toContain('0/1 已合并');
  expect(JSON.stringify(view)).toBe(before);
});

test('模块初始验证无需Task或Run，由真实evidence.recorded关联模块', () => {
  const view = fixture();
  const evidence = { type: 'Evidence' as const, id: 'E-initial', kind: 'test', module: 'A', result: 'no_tests', generated: { by: 'process:towermd-verifier', at: '2026-10-20T00:00:00Z' } };
  view.tasks = []; view.runs = []; view.evidence = [evidence];
  view.events = [{ seq: 4, eid: 'initial', obj: evidence.id, type: 'evidence.recorded', data: { evidence }, t: '2026-10-20T00:00:00Z' }];
  expect(moduleSummary(view, 'A')).toContain('最近验证（当前事件窗口）：未配置测试 · E-initial');
});

test('新评审、其他模块、过期和非验证事件不覆盖当前模块的最近验证', () => {
  const view = fixture();
  const actual = verification('E-current', 'failed'), other = verification('E-other', 'passed', 'B');
  const stale = verification('E-stale', 'passed'), future = verification('E-future', 'passed');
  const gate = { ...verification('E-review', 'passed'), kind: 'gate', gate: 'g4' };
  view.evidence = [actual, other, stale, future, gate];
  view.events = [verificationEvent(2, actual), verificationEvent(3, other), { ...verificationEvent(4, stale), stale: true }, verificationEvent(5, future, 'future.event'), { seq: 6, eid: 'review', obj: gate.id, type: 'evidence.recorded', data: { evidence: gate }, t: '2026-10-20T00:00:00Z' }];
  expect(moduleSummary(view, 'A')).toContain('验证失败 · E-current');
  expect(moduleSummary(view, 'B')).toContain('验证通过 · E-other');
});

test.each(['missing-event', 'missing-evidence', 'different-result', 'history'])('验证来源不完整时明确未知：%s', mode => {
  const view = fixture(); const evidence = verification('E-check', 'passed');
  view.evidence = [evidence]; view.events = [verificationEvent(2, evidence)];
  if (mode === 'missing-event') view.events = [];
  if (mode === 'missing-evidence') view.evidence = [];
  if (mode === 'different-result') view.evidence = [{ ...evidence, result: 'failed' }];
  if (mode === 'history') view.state = 'recovery_required';
  expect(moduleSummary(view, 'A')).toContain('最近验证（当前事件窗口）：未知（缺少可核对的验证记录）');
  expect(moduleSummary(view, 'A')).not.toContain('验证通过');
});

test('更新后的未知验证结果不回退为较早通过', () => {
  const view = fixture(); const older = verification('E-old', 'passed'), latest = verification('E-new', 'future-result');
  view.evidence = [older, latest]; view.events = [verificationEvent(1, older), verificationEvent(2, latest)];
  expect(moduleSummary(view, 'A')).toContain('最近验证（当前事件窗口）：未知结果 · E-new');
  expect(moduleSummary(view, 'A')).not.toContain('验证通过');
});

test('v0.1模块24小时费用保持未知，不把Run费用当作24小时合计或零', () => {
  const view = fixture();
  view.runs[0].cost_usd = 25.5;
  view.runs.push({ type: 'Run', id: 'R-older', task: 'T', state: 'ended', cost_usd: 4.5 });
  expect(moduleSummary(view, 'A')).toContain('24小时费用：未知');
  expect(moduleSummary(view, 'A')).not.toContain('$');
  view.runs = [];
  expect(moduleSummary(view, 'A')).toContain('24小时费用：未知');
});

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
  const evidence = verification('E-latest', 'failed');
  view.evidence = [evidence]; view.events = [verificationEvent(8, evidence)];
  const after = overviewFlow(overview, view);
  expect(after.nodes[0].position).toEqual(before.nodes[0].position);
  expect(after.nodes[0].data.text).toContain('1/1 已合并');
  expect(after.nodes[0].data.text).toContain('最近验证（当前事件窗口）：验证失败 · E-latest');
  expect(after.nodes[0].data.text).toContain('24小时费用：未知');
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

test('健康订阅的来源竞争保留已核实视图和连接，成功后取消旧重试', async () => {
  vi.useFakeTimers(); let changed!: () => void;
  const stop = vi.fn(), views = vi.fn(), states = vi.fn();
  const load = vi.fn().mockResolvedValue(fixture());
  const client: TaskProgressClient = { load, subscribe: vi.fn(async (_token, _after, onChange) => { changed = () => onChange({ kind: 'changed', through_seq: 13, floor_seq: 0, revision: fixture().revision }); return stop; }) };
  const observer = observeProgress('selected', client, views, states);
  await vi.advanceTimersByTimeAsync(0);
  load.mockRejectedValueOnce({ code: 'task_progress_source_changed', message: '读取期间来源已变化', detail: null });
  changed(); await vi.advanceTimersByTimeAsync(100);
  expect(states).toHaveBeenLastCalledWith('updating', expect.stringContaining('读取期间来源已变化'));
  expect(views).toHaveBeenCalledTimes(1); expect(stop).not.toHaveBeenCalled();
  // A subsequent real event permits a successful read before the backoff ends.
  changed(); await vi.advanceTimersByTimeAsync(100);
  expect(views).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(2000);
  expect(load).toHaveBeenCalledTimes(3); expect(client.subscribe).toHaveBeenCalledTimes(1);
  expect(stop).not.toHaveBeenCalled(); observer.close();
});

test('原生错误正文可读，不能按英文消息把任意失败当来源竞争', async () => {
  vi.useFakeTimers(); let changed!: () => void; const states = vi.fn(), stop = vi.fn();
  const load = vi.fn().mockResolvedValue(fixture());
  const client: TaskProgressClient = { load, subscribe: vi.fn(async (_token, _after, onChange) => { changed = () => onChange({ kind: 'changed', through_seq: 13, floor_seq: 0, revision: fixture().revision }); return stop; }) };
  const observer = observeProgress('selected', client, () => {}, states);
  await vi.advanceTimersByTimeAsync(0);
  load.mockRejectedValueOnce({ code: 'other_failure', message: 'recovery inputs changed during preparation', detail: null });
  changed(); await vi.advanceTimersByTimeAsync(100);
  expect(states).toHaveBeenLastCalledWith('disconnected', 'recovery inputs changed during preparation');
  await vi.advanceTimersByTimeAsync(1000);
  expect(client.subscribe).toHaveBeenCalledTimes(2); expect(stop).toHaveBeenCalledTimes(1);
  observer.close();
});

test('来源竞争期间流被关闭仍须重新订阅，不能只读一次后声称实时', async () => {
  vi.useFakeTimers(); let changed!: () => void, reset!: () => void;
  const stop = vi.fn(), load = vi.fn().mockResolvedValue(fixture());
  const client: TaskProgressClient = { load, subscribe: vi.fn(async (_token, _after, onChange, onError) => { changed = () => onChange({ kind: 'changed', through_seq: 13, floor_seq: 0, revision: fixture().revision }); reset = () => onError(Object.assign(new Error('源已更新'), { code: 'task_progress_stream_reset' })); return stop; }) };
  const observer = observeProgress('selected', client, () => {}, () => {});
  await vi.advanceTimersByTimeAsync(0);
  load.mockRejectedValueOnce({ code: 'task_progress_source_changed', message: '读取期间来源已变化' });
  changed(); await vi.advanceTimersByTimeAsync(100); reset();
  await vi.advanceTimersByTimeAsync(1000);
  expect(client.subscribe).toHaveBeenCalledTimes(2); expect(stop).toHaveBeenCalledTimes(1);
  observer.close(); await vi.advanceTimersByTimeAsync(5000);
  expect(load).toHaveBeenCalledTimes(3);
});

test('持续 open 后 reset 仍按原六次预算停止，不因读取成功无限重试', async () => {
  vi.useFakeTimers(); const states = vi.fn();
  const client: TaskProgressClient = { load: vi.fn(async () => fixture()), subscribe: vi.fn(async (_token, _after, _changed, onError) => { const timer = setTimeout(() => onError(Object.assign(new Error('重新同步'), { code: 'task_progress_stream_reset' })), 1); return () => clearTimeout(timer); }) };
  const observer = observeProgress('selected', client, () => {}, states);
  await vi.advanceTimersByTimeAsync(100000);
  expect(client.load).toHaveBeenCalledTimes(7); expect(client.subscribe).toHaveBeenCalledTimes(7);
  expect(states).toHaveBeenLastCalledWith('disconnected', expect.stringContaining('自动重试已停止'));
  await vi.advanceTimersByTimeAsync(100000); expect(client.load).toHaveBeenCalledTimes(7);
  observer.close();
});
