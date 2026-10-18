import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, expect, test, vi } from 'vitest';
import App from './App.svelte';
import { candidateFixture } from './lib/candidate.fixture';
import type { CandidateClient } from './lib/candidate';

afterEach(cleanup);

test('默认不展示假数据，显式进入演示后选择节点更新详情且没有批准动作', async () => {
  render(App);
  expect(screen.queryByText('原型 · 演示数据')).toBeNull();
  await fireEvent.click(screen.getByRole('button', { name: '查看演示' }));
  expect(screen.getByText('原型 · 演示数据')).toBeTruthy();
  expect(screen.getByRole('application')).toBeTruthy();
  expect(screen.getAllByRole('button', { pressed: false })).toHaveLength(5);
  await fireEvent.click(screen.getByRole('button', { name: '构建可交互画布，进行中' }));
  const details = screen.getByRole('complementary', { name: '节点详情' });
  expect(within(details).getByRole('heading', { name: '构建可交互画布' })).toBeTruthy();
  expect(within(details).getByText('连接真实项目投影')).toBeTruthy();
  expect(screen.queryByRole('button', { name: /批准|完成任务/ })).toBeNull();
});

test('缩放按钮改变比例，真实项目入口可以展开和收起', async () => {
  render(App);
  await fireEvent.click(screen.getByRole('button', { name: '查看演示' }));
  const ratio = screen.getByLabelText('缩放比例');
  const before = ratio.textContent;
  await fireEvent.click(screen.getByRole('button', { name: '放大' }));
  expect(ratio.textContent).not.toBe(before);
  await fireEvent.click(screen.getByRole('button', { name: '缩小' }));
  expect(ratio.textContent).toBe(before);
  await fireEvent.click(screen.getByRole('button', { name: '真实项目入口' }));
  expect(screen.getByRole('button', { name: '打开项目' })).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: '返回画布' }));
  expect(screen.queryByRole('button', { name: '打开项目' })).toBeNull();
});

test('平移改变视口，选择节点保持节点坐标', async () => {
  const { container } = render(App);
  await fireEvent.click(screen.getByRole('button', { name: '查看演示' }));
  const canvas = screen.getByRole('application');
  const world = container.querySelector('.canvas-world') as HTMLElement;
  const node = screen.getByRole('button', { name: '构建可交互画布，进行中' });
  const position = node.getAttribute('style');
  await fireEvent.keyDown(canvas, { key: 'ArrowRight' });
  expect(world.style.transform).toContain('translate(-40px, 0px)');
  await fireEvent.wheel(canvas, { deltaY: 25, deltaX: 10 });
  expect(world.style.transform).toContain('translate(-50px, -25px)');
  await fireEvent.click(node);
  expect(node.getAttribute('style')).toBe(position);
  expect(world.style.transform).toContain('translate(-50px, -25px)');
  Object.defineProperty(canvas, 'setPointerCapture', { value: () => undefined });
  await fireEvent.pointerDown(canvas, { button: 0, pointerId: 1, clientX: 100, clientY: 100 });
  await fireEvent.pointerMove(canvas, { pointerId: 1, clientX: 140, clientY: 130 });
  expect(world.style.transform).toContain('translate(-10px, 5px)');
  await fireEvent.pointerUp(canvas, { pointerId: 1 });
  await fireEvent.pointerMove(canvas, { pointerId: 1, clientX: 200, clientY: 200 });
  expect(world.style.transform).toContain('translate(-10px, 5px)');
});

const overview = (title = '实际项目'): import('./lib/project').ProjectOverview => ({
  source: 'repository', revision: 'a'.repeat(40), title, diagnostics: [],
  nodes: [{ id: 'dir:src', title: 'src', kind: 'directory', status: 'draft', text: 'src/main.rs\n<script>只显示原文</script>', x: -120, y: 50, width: 300, height: 160 }], edges: [],
});
const picked = (token: string) => ({ token, displayPath: `/真实项目/${token}`, hasConfig: true });

test('选择真实项目加载画布并查看原文，刷新后读取新提交', async () => {
  let calls = 0;
  const load = async (token: string) => { expect(token).toBe('one'); calls++; return { ...overview(), revision: (calls === 1 ? 'a' : 'b').repeat(40) }; };
  const { container } = render(App, { choose: async () => picked('one'), load });
  await fireEvent.click(screen.getByRole('button', { name: '真实项目入口' }));
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await screen.findByText('Git 结构预览 · 尚未批准为模块');
  await fireEvent.click(screen.getByRole('button', { name: '查看项目画布' }));
  const node = screen.getByRole('button', { name: 'src，目录预览' });
  expect(node.style.left).toBe('-120px');
  expect(node.style.width).toBe('300px');
  expect(screen.getByLabelText('节点原文').textContent).toContain('<script>只显示原文</script>');
  expect(container.querySelector('script')).toBeNull();
  expect(screen.queryByText('原型 · 演示数据')).toBeNull();
  await fireEvent.click(screen.getByRole('button', { name: '刷新' }));
  await screen.findByText('bbbbbbbbbbbb');
  expect(calls).toBe(2);
});

test('切换项目后拒绝旧读取迟到的结果', async () => {
  let deliver!: (value: import('./lib/project').ProjectOverview) => void;
  let choices = 0;
  render(App, { choose: async () => picked(++choices === 1 ? 'one' : 'two'), load: token => token === 'one' ? new Promise(resolve => { deliver = resolve; }) : Promise.resolve(overview('新项目')) });
  await fireEvent.click(screen.getByRole('button', { name: '真实项目入口' }));
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await screen.findByRole('status');
  const picker = screen.getByRole('region', { name: '打开你的项目' });
  await fireEvent.click(within(picker).getByRole('button', { name: '切换项目' }));
  await screen.findByText('Git 结构预览 · 尚未批准为模块');
  deliver(overview('旧项目迟到'));
  await new Promise(resolve => setTimeout(resolve, 0));
  expect(screen.queryByText('旧项目迟到')).toBeNull();
  expect(screen.getByRole('heading', { name: /新项目\s*\./ })).toBeTruthy();
});

test('读取失败显示诊断并允许重试，空仓库显示空态', async () => {
  let attempts = 0;
  render(App, { choose: async () => picked('empty'), load: async () => {
    if (++attempts === 1) throw { message: '真实读取错误' };
    return { ...overview(), nodes: [], edges: [], revision: '', diagnostics: ['空仓库尚无提交'] };
  } });
  await fireEvent.click(screen.getByRole('button', { name: '真实项目入口' }));
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await screen.findByText('真实读取错误');
  await fireEvent.click(screen.getByRole('button', { name: '查看项目画布' }));
  await fireEvent.click(screen.getByRole('button', { name: '重试读取' }));
  await screen.findByText('这里还没有节点');
  expect(screen.getByText('尚无提交')).toBeTruthy();
  expect(screen.queryByRole('complementary', { name: '节点详情' })).toBeNull();
});


test('explicit generation shows actual unapproved modules, exact source text and all verification outcomes', async () => {
  const draft = candidateFixture();
  const scanClient: CandidateClient = { start: vi.fn(async () => ({ scan_id: draft.scan_id, state: 'queued' })), wait: vi.fn(async () => ({ scan_id: draft.scan_id, state: 'completed' })), draft: vi.fn(async () => draft) };
  const { container } = render(App, { choose: async () => picked('actual'), load: async () => overview(), scanClient });
  await fireEvent.click(screen.getByRole('button', { name: '打开真实项目' }));
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await screen.findByText('Git 结构预览 · 尚未批准为模块');
  expect(scanClient.start).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole('button', { name: '查看项目画布' }));
  await fireEvent.click(screen.getByRole('button', { name: '生成候选画布' }));
  await screen.findByRole('button', { name: 'A-pass，验证通过' });
  expect(screen.getByRole('button', { name: 'A-fail，验证失败' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'A-empty，未验证 · 无可执行测试' })).toBeTruthy();
  expect(screen.getByLabelText('节点原文').textContent).toContain('<script>真实原文</script>');
  expect(container.querySelector('script')).toBeNull();
  expect(screen.getByLabelText('验证结果').textContent).toContain('"passed"');
  expect(screen.getByText('actual-report')).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: 'A-empty，未验证 · 无可执行测试' }));
  expect(screen.getByLabelText('验证结果').textContent).toContain('"executions": []');
  expect(screen.queryByRole('button', { name: /批准|保存/ })).toBeNull();
  expect(scanClient.start).toHaveBeenCalledTimes(1);
});

test('an ordinary Git project can open a read-only preview without initialization or scan', async () => {
  const scanClient: CandidateClient = { start: vi.fn(), wait: vi.fn(), draft: vi.fn() };
  render(App, { choose: async () => ({ ...picked('plain'), hasConfig: false }), load: async () => overview(), scanClient });
  await fireEvent.click(screen.getByRole('button', { name: '打开真实项目' }));
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await screen.findByText('Git 结构预览 · 尚未批准为模块');
  expect(screen.queryByRole('button', { name: '生成候选画布' })).toBeNull();
  expect(scanClient.start).not.toHaveBeenCalled();
});

test('zero-module results still expose the one real shared report without inventing nodes', async () => {
  const draft = candidateFixture(); draft.modules = []; draft.canvas.nodes = []; draft.evidence = []; draft.verification_report.executions = [];
  const scanClient: CandidateClient = { start: async () => ({ scan_id: draft.scan_id, state: 'queued' }), wait: async () => ({ scan_id: draft.scan_id, state: 'completed' }), draft: async () => draft };
  render(App, { choose: async () => picked('zero'), load: async () => overview(), scanClient });
  await fireEvent.click(screen.getByRole('button', { name: '打开真实项目' }));
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await screen.findByText('Git 结构预览 · 尚未批准为模块');
  await fireEvent.click(screen.getByRole('button', { name: '查看项目画布' }));
  await fireEvent.click(screen.getByRole('button', { name: '生成候选画布' }));
  await screen.findByText('这里还没有节点');
  expect(screen.getByText('共享验证报告（0 次执行）')).toBeTruthy();
  expect(screen.getByRole('region', { name: '共享验证报告' }).textContent).toContain(draft.report_eid);
  expect(screen.queryByRole('complementary', { name: '节点详情' })).toBeNull();
});
