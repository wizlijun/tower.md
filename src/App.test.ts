import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, expect, test, vi } from 'vitest';
import App from './App.svelte';
import { candidateFixture } from './lib/candidate.fixture';
import type { CandidateClient } from './lib/candidate';
import type { DraftClient, PreparedEdit } from './lib/draft';
import { savedFixture } from './lib/draft.fixture';
import { publicationFixture } from './lib/review-publication.fixture';
import type { PreparedReview, ReviewClient, ReviewOutcome, ReviewView } from './lib/review';
import type { TaskProgressClient, TaskProgressView } from './lib/task-progress';

afterEach(cleanup);

async function menu(name: string) {
  await fireEvent.contextMenu(screen.getByRole('main'));
  await fireEvent.click(screen.getByRole('menuitem', { name }));
}
async function tools() { await menu('项目工具'); }
async function drafts() { await tools(); await fireEvent.click(screen.getByRole('button', { name: '草稿与候选画布' })); }
async function projectCanvas() { await screen.findByRole('region', { name: '真实项目画布' }); }


test('默认只有空画布和项目节点，菜单、工具、表单、侧栏均不挂载且不自动读取或扫描', async () => {
  const choose = vi.fn(); const load = vi.fn();
  const scanClient: CandidateClient = { start: vi.fn(), wait: vi.fn(), draft: vi.fn() };
  const { container } = render(App, { choose, load, scanClient });
  expect(screen.getByRole('region', { name: '空画布' })).toBeTruthy();
  expect(screen.getAllByRole('button')).toHaveLength(1);
  expect(screen.getByRole('button', { name: '打开项目' })).toBeTruthy();
  expect(container.querySelector('header, footer, form, aside')).toBeNull();
  expect(screen.queryByRole('menu')).toBeNull();
  expect(screen.queryByRole('heading')).toBeNull();
  expect(choose).not.toHaveBeenCalled(); expect(load).not.toHaveBeenCalled();
  expect(scanClient.start).not.toHaveBeenCalled();
  await fireEvent.keyDown(screen.getByRole('main'), { key: 'F10', shiftKey: true });
  expect(screen.getByRole('menu', { name: '画布菜单' })).toBeTruthy();
  await fireEvent.keyDown(screen.getByRole('main'), { key: 'Escape' });
  expect(screen.queryByRole('menu')).toBeNull();
});

test('显式菜单进入演示，只有选择节点才打开详情且关闭后还原单画布', async () => {
  render(App);
  expect(screen.queryByRole('application')).toBeNull();
  await menu('查看演示');
  expect(screen.getByRole('application')).toBeTruthy();
  expect(screen.getAllByRole('button', { pressed: false })).toHaveLength(6);
  expect(screen.queryByRole('complementary')).toBeNull();
  await fireEvent.click(screen.getByRole('button', { name: '构建可交互画布，进行中' }));
  const details = screen.getByRole('complementary', { name: '节点详情' });
  expect(within(details).getByRole('heading', { name: '构建可交互画布' })).toBeTruthy();
  expect(within(details).getByText('连接真实项目投影')).toBeTruthy();
  expect(screen.queryByRole('button', { name: /批准|完成任务/ })).toBeNull();
  await fireEvent.click(screen.getByRole('button', { name: '关闭节点详情' }));
  expect(screen.queryByRole('complementary')).toBeNull();
});

test('手势缩放改变视口，打开项目仅通过画布节点或按需菜单', async () => {
  const choose = vi.fn(async () => null);
  const { container } = render(App, { choose });
  await menu('查看演示');
  const canvas = screen.getByRole('application');
  const world = container.querySelector('.canvas-world') as HTMLElement;
  const before = world.style.transform;
  await fireEvent.wheel(canvas, { deltaY: -25, ctrlKey: true });
  expect(world.style.transform).not.toBe(before);
  await fireEvent.wheel(canvas, { deltaY: 25, ctrlKey: true });
  expect(Number(world.style.transform.match(/scale\(([^)]+)\)/)?.[1])).toBeCloseTo(1, 12);
  expect(screen.queryByRole('button', { name: '放大' })).toBeNull();
  expect(screen.queryByRole('button', { name: '缩小' })).toBeNull();
  await menu('打开项目');
  expect(choose).toHaveBeenCalledTimes(1);
  expect(screen.getByRole('application')).toBe(canvas);
  expect(screen.queryByRole('complementary')).toBeNull();
});

test('平移改变视口，选择节点保持节点坐标', async () => {
  const { container } = render(App);
  await menu('查看演示');
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
  const taskProgressClient = { load: vi.fn(async (): Promise<import('./lib/task-progress').TaskProgressView> => ({ project_id: 'P', common_dir: '/真实项目/one/.git', revision: (calls === 1 ? 'a' : 'b').repeat(40), captured_at: 1800000000000, through_seq: 0, floor_seq: 0, state: 'recorded', source: { kind: 'hub_journal' }, tasks: [], runs: [], evidence: [], diagnostics: [] })) };
  const { container } = render(App, { choose: async () => picked('one'), load, taskProgressClient });
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await projectCanvas();
  expect(container.querySelector('header, footer, form, aside')).toBeNull();
  expect(screen.queryByRole('link', { name: 'Svelte Flow attribution' })).toBeNull();
  expect(screen.queryByRole('button', { name: '生成候选画布' })).toBeNull();
  expect(screen.queryByRole('button', { name: '恢复本地 hub' })).toBeNull();
  const live = screen.getByRole('region', { name: '真实项目画布' });
  await fireEvent.contextMenu(live);
  expect(screen.getAllByRole('menu')).toHaveLength(1);
  expect(screen.getByRole('menu', { name: '画布操作' })).toBeTruthy();
  expect(screen.queryByRole('menu', { name: '画布菜单' })).toBeNull();
  await fireEvent.keyDown(window, { key: 'Escape' });
  expect(screen.queryByRole('menu')).toBeNull();
  const node = container.querySelector<HTMLElement>('.live-flow .svelte-flow__node[data-id="dir:src"]');
  expect(node).not.toBeNull();
  expect(node!.style.transform).toBe('translate(-120px, 50px)');
  expect(node!.style.height).toBe('160px');
  expect(node!.style.width).toBe('300px');
  await fireEvent.click(node!);
  expect(screen.getByLabelText('节点原文').textContent).toContain('<script>只显示原文</script>');
  expect(container.querySelector('script')).toBeNull();
  expect(screen.queryByRole('button', { name: '构建可交互画布，进行中' })).toBeNull();
  await menu('刷新画布');
  await projectCanvas();
  await tools();
  await screen.findByText('bbbbbbbbbbbb');
  expect(calls).toBe(2);
});

test('切换项目后拒绝旧读取迟到的结果', async () => {
  let deliver!: (value: import('./lib/project').ProjectOverview) => void;
  let choices = 0;
  render(App, { choose: async () => picked(++choices === 1 ? 'one' : 'two'), load: token => token === 'one' ? new Promise(resolve => { deliver = resolve; }) : Promise.resolve(overview('新项目')) });
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await screen.findByRole('status');
  await menu('切换项目');
  await projectCanvas();
  deliver(overview('旧项目迟到'));
  await new Promise(resolve => setTimeout(resolve, 0));
  expect(screen.queryByText('旧项目迟到')).toBeNull();
  await tools();
  expect(screen.getByRole('heading', { name: '新项目' })).toBeTruthy();
});

test('读取失败显示诊断并允许重试，空仓库显示空态', async () => {
  let attempts = 0;
  const { container } = render(App, { choose: async () => picked('empty'), taskProgressClient: { load: async () => ({ project_id: 'P', common_dir: '/真实项目/empty/.git', revision: '', captured_at: 1800000000000, through_seq: null, floor_seq: null, state: 'unavailable', source: { kind: 'none' }, tasks: [], runs: [], evidence: [], diagnostics: [] }) }, load: async () => {
    if (++attempts === 1) throw { message: '真实读取错误' };
    return { ...overview(), nodes: [], edges: [], revision: '', diagnostics: ['空仓库尚无提交'] };
  } });
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await screen.findByText('真实读取错误');
  await fireEvent.click(screen.getByRole('button', { name: '重试读取' }));
  await projectCanvas();
  expect(container.querySelectorAll('.live-flow .svelte-flow__node')).toHaveLength(1);
  expect(container.querySelector('.live-flow .svelte-flow__node[data-id="__towermd_project_view__"]')).not.toBeNull();
  await tools();
  expect(screen.getByText('尚无提交')).toBeTruthy();
  expect(screen.queryByRole('complementary', { name: '节点详情' })).toBeNull();
  expect(screen.queryByRole('complementary', { name: '节点原文' })).toBeNull();
});


test('explicit generation shows actual unapproved modules, exact source text and all verification outcomes', async () => {
  const draft = candidateFixture();
  const scanClient: CandidateClient = { start: vi.fn(async () => ({ scan_id: draft.scan_id, state: 'queued' })), wait: vi.fn(async () => ({ scan_id: draft.scan_id, state: 'completed' })), draft: vi.fn(async () => draft) };
  const { container } = render(App, { choose: async () => picked('actual'), load: async () => overview(), scanClient });
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await projectCanvas();
  expect(scanClient.start).not.toHaveBeenCalled();
  await drafts();
  await fireEvent.click(screen.getByRole('button', { name: '生成候选画布' }));
  await fireEvent.click(await screen.findByRole('button', { name: 'A-pass，验证通过' }));
  expect(screen.getByRole('button', { name: 'A-fail，验证失败' })).toBeTruthy();
  expect(screen.getByRole('button', { name: 'A-empty，未验证 · 无可执行测试' })).toBeTruthy();
  expect(screen.getByLabelText('节点原文').textContent).toContain('<script>真实原文</script>');
  expect(container.querySelector('script')).toBeNull();
  expect(screen.getByLabelText('验证结果').textContent).toContain('"passed"');
  expect(within(screen.getByRole('region', { name: '共享验证报告' })).getByText(draft.report_eid)).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: 'A-empty，未验证 · 无可执行测试' }));
  expect(screen.getByLabelText('验证结果').textContent).toContain('"executions": []');
  expect(screen.queryByRole('button', { name: /^(批准|保存此编辑)/ })).toBeNull();
  expect(scanClient.start).toHaveBeenCalledTimes(1);
});

test('an ordinary Git project can open a read-only preview without initialization or scan', async () => {
  const scanClient: CandidateClient = { start: vi.fn(), wait: vi.fn(), draft: vi.fn() };
  render(App, { choose: async () => ({ ...picked('plain'), hasConfig: false }), load: async () => overview(), scanClient });
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await screen.findByRole('application');
  await tools();
  expect(screen.queryByRole('button', { name: '生成候选画布' })).toBeNull();
  expect(scanClient.start).not.toHaveBeenCalled();
});

test('zero-module results still expose the one real shared report without inventing nodes', async () => {
  const draft = candidateFixture(); draft.modules = []; draft.canvas.nodes = []; draft.evidence = []; draft.verification_report.executions = [];
  const scanClient: CandidateClient = { start: async () => ({ scan_id: draft.scan_id, state: 'queued' }), wait: async () => ({ scan_id: draft.scan_id, state: 'completed' }), draft: async () => draft };
  render(App, { choose: async () => picked('zero'), load: async () => overview(), scanClient });
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await projectCanvas();
  await drafts();
  await fireEvent.click(screen.getByRole('button', { name: '生成候选画布' }));
  await screen.findByText('这里还没有节点');
  expect(screen.getByText('共享验证报告（0 次执行）')).toBeTruthy();
  expect(screen.getByRole('region', { name: '共享验证报告' }).textContent).toContain(draft.report_eid);
  expect(screen.queryByRole('complementary', { name: '节点详情' })).toBeNull();
});

test('candidate handoff explicitly opens the same durable S6 ref, retains a missing-ref preview and recovers its original queue', async () => {
  const draft = candidateFixture();
  const saved = savedFixture(); saved.id = draft.scan_id; saved.draft.bundle.id = draft.scan_id; saved.draft.bundle.source_revision = draft.input.revision;
  const original: PreparedEdit = { request_id: 'pending-original', draft_id: draft.scan_id, expected_revision: saved.revision, human_actor: 'human:fixture' };
  const scanClient: CandidateClient = { start: vi.fn(async () => ({ scan_id: draft.scan_id, state: 'queued' })), wait: vi.fn(async () => ({ scan_id: draft.scan_id, state: 'completed' })), draft: vi.fn(async () => draft) };
  const savedDraftClient: DraftClient = { load: vi.fn().mockRejectedValueOnce({ code: 'draft_not_saved', message: '此 ID 尚无已保存的 S6 草稿。请让已有会话中的 Agent 使用 baseline submit 提交；扫描候选不自动转为 S6。', detail: null }).mockResolvedValueOnce(saved), pending: vi.fn(async () => [original]), prepare: vi.fn(), submit: vi.fn() };
  render(App, { choose: async () => picked('handoff'), load: async () => overview(), scanClient, savedDraftClient });
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await projectCanvas();
  await drafts();
  await fireEvent.click(screen.getByRole('button', { name: '生成候选画布' }));
  const open = await screen.findByRole('button', { name: '打开此扫描的 S6 草稿' });
  expect(savedDraftClient.load).not.toHaveBeenCalled();
  await fireEvent.click(open);
  expect((await screen.findByRole('alert')).textContent).toContain('此 ID 尚无已保存的 S6 草稿。');
  expect(screen.getByRole('button', { name: 'A-pass，验证通过' })).toBeTruthy();
  expect(screen.queryByRole('button', { name: '保存此编辑到草稿' })).toBeNull();
  await fireEvent.click(screen.getByRole('button', { name: '打开此扫描的 S6 草稿' }));
  await screen.findByRole('region', { name: '原请求等待确认' });
  expect(savedDraftClient.load).toHaveBeenNthCalledWith(1, 'handoff', draft.scan_id);
  expect(savedDraftClient.load).toHaveBeenNthCalledWith(2, 'handoff', draft.scan_id);
  expect(savedDraftClient.pending).toHaveBeenCalledExactlyOnceWith('handoff', draft.scan_id);
  expect((screen.getByRole('button', { name: '保存此编辑到草稿' }) as HTMLButtonElement).disabled).toBe(true);
  expect(savedDraftClient.prepare).not.toHaveBeenCalled(); expect(savedDraftClient.submit).not.toHaveBeenCalled();
  expect(scanClient.start).toHaveBeenCalledTimes(1); expect(scanClient.wait).toHaveBeenCalledTimes(1);
});

test('switching project while opening its candidate discards the old durable response', async () => {
  const draft = candidateFixture(); let deliver!: (value: ReturnType<typeof savedFixture>) => void; let choices = 0;
  const scanClient: CandidateClient = { start: vi.fn(async () => ({ scan_id: draft.scan_id, state: 'queued' })), wait: vi.fn(async () => ({ scan_id: draft.scan_id, state: 'completed' })), draft: vi.fn(async () => draft) };
  const savedDraftClient: DraftClient = { load: vi.fn(() => new Promise<ReturnType<typeof savedFixture>>(resolve => { deliver = resolve; })), pending: vi.fn(async () => []), prepare: vi.fn(), submit: vi.fn() };
  render(App, { choose: async () => picked(++choices === 1 ? 'old' : 'new'), load: async token => overview(token), scanClient, savedDraftClient });
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await projectCanvas();
  await drafts();
  await fireEvent.click(screen.getByRole('button', { name: '生成候选画布' }));
  await fireEvent.click(await screen.findByRole('button', { name: '打开此扫描的 S6 草稿' }));
  await waitFor(() => expect(savedDraftClient.load).toHaveBeenCalledExactlyOnceWith('old', draft.scan_id));
  expect((screen.getByRole('button', { name: '打开此扫描的 S6 草稿' }) as HTMLButtonElement).disabled).toBe(true);
  await menu('切换项目');
  await projectCanvas();
  await tools();
  await screen.findByRole('heading', { name: 'new' });
  const oldSaved = savedFixture(); oldSaved.id = draft.scan_id; oldSaved.draft.bundle.id = draft.scan_id; oldSaved.draft.bundle.source_revision = draft.input.revision;
  deliver(oldSaved); await new Promise(resolve => setTimeout(resolve, 0));
  expect(screen.queryByText('本机身份：human:fixture')).toBeNull();
  expect(screen.queryByRole('button', { name: '打开此扫描的 S6 草稿' })).toBeNull();
  expect(savedDraftClient.pending).not.toHaveBeenCalled();
  expect(scanClient.start).toHaveBeenCalledTimes(1);
});


test('初始化项目提供显式当前基线草稿入口，查看项目不会准备或创建', async () => {
  render(App, { choose: async () => picked('baseline-project'), load: async () => overview() });
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await projectCanvas();
  expect(screen.queryByRole('button', { name: '创建或恢复变更草稿' })).toBeNull();
  await drafts();
  expect(screen.getByRole('button', { name: '创建或恢复变更草稿' })).toBeTruthy();
  expect(screen.queryByRole('button', { name: '确认创建变更草稿' })).toBeNull();
});

test('当前基线创建成功后读取真实保存草稿并复用现有编辑画布',async()=>{
 const draft=savedFixture();draft.id='created-uuid';draft.draft.bundle.id=draft.id;draft.draft.bundle.target_baseline='towermd/B1';
 const currentClient:import('./lib/current-draft').CurrentDraftClient={load:vi.fn(async()=>({baseline:'towermd/B0',main_revision:'e'.repeat(40)})),pending:vi.fn(async()=>[]),prepare:vi.fn(async()=>({request_id:draft.id,draft_id:draft.id,baseline:'towermd/B0',main_revision:'e'.repeat(40),human_actor:'human:fixture'})),submit:vi.fn(async():Promise<import('./lib/draft').EditOutcome>=>({state:'confirmed',request_id:draft.id,error:null,response:{receipt:{request_id:draft.id,body_hash:'body',result_ref:{draft_id:draft.id,revision:draft.revision,reference:`refs/towermd/drafts/${draft.id}`,content_hash:draft.draft.content_hash,delivery:{state:'local_saved'}}},current_validity:{state:'current'}}}))};
 const savedDraftClient:DraftClient={load:vi.fn(async()=>draft),pending:vi.fn(async()=>[]),prepare:vi.fn(),submit:vi.fn()};
 render(App,{choose:async()=>picked('current'),load:async()=>overview(),currentDraftClient:currentClient,savedDraftClient});
 await fireEvent.click(screen.getByRole('button',{name:'打开项目'}));await projectCanvas();
 expect(currentClient.load).not.toHaveBeenCalled();await drafts();await fireEvent.click(screen.getByRole('button',{name:'创建或恢复变更草稿'}));await screen.findByText('e'.repeat(40));await fireEvent.click(screen.getByRole('button',{name:'确认创建变更草稿'}));
 await screen.findByText('目标 towermd/B1');expect(savedDraftClient.load).toHaveBeenCalledWith('current','created-uuid');expect(screen.getByRole('button',{name:'保存此编辑到草稿'})).toBeTruthy();expect(screen.getByRole('button',{name:'审阅与人工决定'})).toBeTruthy();
});

test('已初始化项目提供显式恢复入口，打开项目不读取或确认恢复请求',async()=>{
 render(App,{choose:async()=>picked('recovery-project'),load:async()=>overview()});
 await fireEvent.click(screen.getByRole('button',{name:'打开项目'}));await projectCanvas();
 expect(screen.queryByRole('button',{name:'恢复本地 hub'})).toBeNull();await tools();expect(screen.getByRole('button',{name:'恢复本地 hub'})).toBeTruthy();expect(screen.queryByRole('button',{name:'确认恢复此请求'})).toBeNull();
});

test('恢复入口只在显式读取后调用原请求，切换项目丢弃旧恢复响应',async()=>{
 let choices=0;let deliver!:(value:import('./lib/recovery').RecoveryView)=>void;const recoveryClient:import('./lib/recovery').RecoveryClient={load:vi.fn(()=>new Promise<import('./lib/recovery').RecoveryView>(resolve=>deliver=resolve)),confirm:vi.fn()};
 render(App,{choose:async()=>picked(++choices===1?'old-recovery':'new-recovery'),load:async token=>overview(token),recoveryClient});await fireEvent.click(screen.getByRole('button',{name:'打开项目'}));await projectCanvas();expect(recoveryClient.load).not.toHaveBeenCalled();await tools();await fireEvent.click(screen.getByRole('button',{name:'恢复本地 hub'}));expect(recoveryClient.load).not.toHaveBeenCalled();
 await fireEvent.input(screen.getByRole('textbox',{name:'已登记的恢复请求 ID'}),{target:{value:'a'.repeat(64)}});await fireEvent.click(screen.getByRole('button',{name:'读取原恢复请求'}));await waitFor(()=>expect(recoveryClient.load).toHaveBeenCalledExactlyOnceWith('old-recovery','a'.repeat(64)));
 await menu('切换项目');await projectCanvas();await tools();await screen.findByRole('heading',{name:'new-recovery'});deliver({request:{status:'awaiting_app_confirmation',request_id:'a'.repeat(64),project_id:'old',main_revision:'b'.repeat(40),source_commit:'c'.repeat(40),source_seq:1,seq_boundary:4294967297,epoch_boundary:4294967297,source_epoch_high_water:1,source_reference:'main;refs/towermd/snapshot:missing',inputs_digest:'d'.repeat(64),requested_at:Date.now(),expires_at:Date.now()+100000,nonce:'0192f0aa-0000-7000-8000-000000000001'},human_actor:'human:old',can_confirm:true});await Promise.resolve();expect(screen.queryByRole('region',{name:'原恢复请求详情'})).toBeNull();expect(recoveryClient.confirm).not.toHaveBeenCalled();
});


function lifecycleProgress(revision: string): TaskProgressView {
  return { project_id: 'project', common_dir: '/真实项目/lifecycle/.git', revision,
    captured_at: 1800000000000, through_seq: 2, floor_seq: 0, state: 'recorded',
    source: { kind: 'hub_journal' }, tasks: [], runs: [], evidence: [], diagnostics: [] };
}

test.each(['关闭项目工具', '返回项目工具', 'Escape', '刷新画布'])(
  '退出候选显示（%s）恢复权威画布与单一订阅，并保留同扫描 ID 供显式重开', async exit => {
    const draft = candidateFixture();
    const scanClient: CandidateClient = { start: vi.fn(async () => ({ scan_id: draft.scan_id, state: 'queued' })), wait: vi.fn(async () => ({ scan_id: draft.scan_id, state: 'completed' })), draft: vi.fn(async () => draft) };
    let active = 0;
    const stop = vi.fn(() => { active--; });
    const taskProgressClient: TaskProgressClient = { load: vi.fn(async () => lifecycleProgress('a'.repeat(40))), subscribe: vi.fn(async () => { active++; return stop; }) };
    const savedDraftClient: DraftClient = { load: vi.fn(), pending: vi.fn(), prepare: vi.fn(), submit: vi.fn() };
    const { container } = render(App, { choose: async () => picked('lifecycle'), load: async () => ({ ...overview(), source: 'baseline' }), scanClient, savedDraftClient, taskProgressClient });
    await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
    await projectCanvas();
    await waitFor(() => expect(taskProgressClient.subscribe).toHaveBeenCalledTimes(1));
    const position = container.querySelector<HTMLElement>('.live-flow .svelte-flow__node[data-id="dir:src"]')!.style.transform;
    await drafts();
    await fireEvent.click(screen.getByRole('button', { name: '生成候选画布' }));
    await screen.findByRole('button', { name: 'A-pass，验证通过' });
    expect(screen.queryByRole('region', { name: '真实项目画布' })).toBeNull();
    expect(stop).toHaveBeenCalledTimes(1); expect(active).toBe(0);
    if (exit === 'Escape') await fireEvent.keyDown(screen.getByRole('main'), { key: 'Escape' });
    else if (exit === '刷新画布') await menu('刷新画布');
    else await fireEvent.click(screen.getByRole('button', { name: exit }));
    await projectCanvas();
    await waitFor(() => expect(taskProgressClient.subscribe).toHaveBeenCalledTimes(2));
    expect(active).toBe(1);
    expect(container.querySelector<HTMLElement>('.live-flow .svelte-flow__node[data-id="dir:src"]')!.style.transform).toBe(position);
    expect(screen.queryByRole('button', { name: 'A-pass，验证通过' })).toBeNull();
    await drafts();
    expect(screen.getByRole('button', { name: '打开此扫描的 S6 草稿' })).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: '查看候选画布' }));
    await screen.findByRole('button', { name: 'A-pass，验证通过' });
    expect(scanClient.start).toHaveBeenCalledTimes(1); expect(scanClient.wait).toHaveBeenCalledTimes(1);
    expect(scanClient.draft).toHaveBeenCalledExactlyOnceWith('lifecycle', draft.scan_id);
    expect(savedDraftClient.load).not.toHaveBeenCalled(); expect(savedDraftClient.prepare).not.toHaveBeenCalled(); expect(savedDraftClient.submit).not.toHaveBeenCalled();
  },
);

test.each(['towermd/B0', 'towermd/B1'])(
  '候选原 S6 的原 UUID 重试出版 %s 后仅显式关闭才回到重新核实的 main，保持单一订阅且不重扫或再批准', async target => {
    const draft = candidateFixture(); const saved = savedFixture();
    saved.id = draft.scan_id; saved.draft.bundle.id = saved.id;
    saved.draft.bundle.source_revision = draft.input.revision; saved.draft.bundle.target_baseline = target;
    const binding = { project_id: saved.project_id, object_id: saved.id, content_hash: saved.draft.content_hash, target_baseline: target, draft_revision: saved.revision };
    const original: PreparedReview = { request_id: '01900000-0000-7000-8000-000000000001', binding, verdict: 'approved', comment: '原决定，逐项核对', human_actor: saved.human_actor };
    const review: ReviewView = { binding, review_ticket: '01900000-0000-7000-8000-000000000002', human_actor: saved.human_actor,
      current_baseline: target === 'towermd/B0' ? null : 'towermd/B0', draft: saved.draft,
      diff: [{ path: 'docs/tower/arch/A-module.md', before: '原主干正文', after: '原 S6 候选完整正文' }], interface_modules: [], publish_blockers: [], original_requests: [original] };
    const scanClient: CandidateClient = { start: vi.fn(async () => ({ scan_id: draft.scan_id, state: 'queued' })), wait: vi.fn(async () => ({ scan_id: draft.scan_id, state: 'completed' })), draft: vi.fn(async () => draft) };
    const savedDraftClient: DraftClient = { load: vi.fn(async () => saved), pending: vi.fn(async () => []), prepare: vi.fn(), submit: vi.fn() };
    let revision = 'a'.repeat(40); let active = 0; let attempts = 0;
    const stop = vi.fn(() => { active--; });
    const taskProgressClient: TaskProgressClient = { load: vi.fn(async () => lifecycleProgress(revision)), subscribe: vi.fn(async () => { active++; return stop; }) };
    const load = vi.fn(async () => ({ ...overview('当前权威 main'), source: 'baseline' as const, revision }));
    let deliver!: (value: ReviewOutcome) => void;
    const reviewClient: ReviewClient = { load: vi.fn(async () => review), pending: vi.fn(async () => []), prepare: vi.fn(), submit: vi.fn(async (): Promise<ReviewOutcome> => {
      if (++attempts === 1) return { state: 'uncertain', request_id: original.request_id, response: null, error: { code: 'request_uncertain', message: '原出版状态待核实', detail: null } };
      return new Promise<ReviewOutcome>(resolve => { deliver = resolve; });
    }) };
    const { container } = render(App, { choose: async () => picked('lifecycle'), load, scanClient, savedDraftClient, taskProgressClient, proposalReviewClient: reviewClient });
    await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
    await projectCanvas();
    await waitFor(() => expect(taskProgressClient.subscribe).toHaveBeenCalledTimes(1));
    await drafts(); await fireEvent.click(screen.getByRole('button', { name: '生成候选画布' }));
    await fireEvent.click(await screen.findByRole('button', { name: '打开此扫描的 S6 草稿' }));
    await screen.findByRole('button', { name: '审阅与人工决定' });
    await waitFor(() => expect((screen.getByRole('button', { name: '审阅与人工决定' }) as HTMLButtonElement).disabled).toBe(false));
    await fireEvent.click(screen.getByRole('button', { name: '审阅与人工决定' }));
    await screen.findByText('原 S6 候选完整正文');
    await fireEvent.click(screen.getByRole('button', { name: '读取或协调原审阅结果' }));
    await screen.findByText('原出版状态待核实');
    await waitFor(() => expect((screen.getByRole('button', { name: '读取或协调原审阅结果' }) as HTMLButtonElement).disabled).toBe(false));
    await fireEvent.click(screen.getByRole('button', { name: '读取或协调原审阅结果' }));
    await waitFor(() => expect(reviewClient.submit).toHaveBeenCalledTimes(2));
    expect(screen.getByRole('region', { name: '基线人工审阅' })).toBeTruthy();
    expect((screen.getByRole('button', { name: '返回草稿' }) as HTMLButtonElement).disabled).toBe(true);
    revision = 'b'.repeat(40);
    const created = publicationFixture(original, 'approval-original');
    deliver({ state: 'confirmed', request_id: original.request_id, error: null, response: {
      receipt: { request_id: original.request_id, body_hash: 'b'.repeat(64), result_ref: { approval_eid: 'approval-original', binding, actor: saved.human_actor, verdict: 'approved', comment: original.comment } },
      current_validity: { state: 'current' }, publication: { state: 'recorded', created },
    } });
    const receipt = await screen.findByRole('region', { name: '基线出版回执' });
    expect(receipt.textContent).toContain(target); expect(receipt.textContent).toContain(created.commit);
    expect(screen.getByRole('region', { name: '基线人工审阅' })).toBeTruthy();
    expect(active).toBe(0);
    await fireEvent.click(screen.getByRole('button', { name: '关闭项目工具' }));
    await projectCanvas();
    await waitFor(() => expect(load).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(taskProgressClient.subscribe).toHaveBeenCalledTimes(2));
    expect(active).toBe(1); expect(stop).toHaveBeenCalledTimes(1);
    expect(container.querySelector('.live-flow .svelte-flow__node[data-id="dir:src"]')).not.toBeNull();
    expect(screen.queryByRole('button', { name: 'A-pass，验证通过' })).toBeNull();
    expect(screen.queryByRole('complementary')).toBeNull();
    await tools(); expect(within(screen.getByRole('complementary', { name: '项目工具' })).getByText('bbbbbbbbbbbb')).toBeTruthy();
    await fireEvent.click(screen.getByRole('button', { name: '草稿与候选画布' }));
    expect(screen.getByRole('button', { name: '打开此扫描的 S6 草稿' })).toBeTruthy();
    expect(savedDraftClient.load).toHaveBeenCalledExactlyOnceWith('lifecycle', draft.scan_id);
    expect(reviewClient.submit).toHaveBeenNthCalledWith(1, 'lifecycle', original.request_id);
    expect(reviewClient.submit).toHaveBeenNthCalledWith(2, 'lifecycle', original.request_id);
    expect(reviewClient.prepare).not.toHaveBeenCalled(); expect(savedDraftClient.prepare).not.toHaveBeenCalled(); expect(savedDraftClient.submit).not.toHaveBeenCalled();
    expect(scanClient.start).toHaveBeenCalledTimes(1); expect(scanClient.wait).toHaveBeenCalledTimes(1); expect(scanClient.draft).toHaveBeenCalledTimes(1);
  },
);
