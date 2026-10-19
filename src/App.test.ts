import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, expect, test, vi } from 'vitest';
import App from './App.svelte';
import { candidateFixture } from './lib/candidate.fixture';
import type { CandidateClient } from './lib/candidate';
import type { DraftClient, PreparedEdit } from './lib/draft';
import { savedFixture } from './lib/draft.fixture';

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
  const taskProgressClient = { load: vi.fn(async (): Promise<import('./lib/task-progress').TaskProgressView> => ({ project_id: 'P', common_dir: '/真实项目/one/.git', revision: (calls === 1 ? 'a' : 'b').repeat(40), captured_at: 1800000000000, through_seq: 0, floor_seq: 0, state: 'recorded', source: { kind: 'hub_journal' }, tasks: [], runs: [], evidence: [], diagnostics: [] })) };
  const { container } = render(App, { choose: async () => picked('one'), load, taskProgressClient });
  await fireEvent.click(screen.getByRole('button', { name: '真实项目入口' }));
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await screen.findByText('Git 结构预览 · 尚未批准为模块');
  await fireEvent.click(screen.getByRole('button', { name: '查看项目画布' }));
  const node = container.querySelector<HTMLElement>('.live-flow .svelte-flow__node[data-id="dir:src"]');
  expect(node).not.toBeNull();
  expect(node!.style.transform).toBe('translate(-120px, 50px)');
  expect(node!.style.height).toBe('160px');
  expect(node!.style.width).toBe('300px');
  await fireEvent.click(node!);
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
  const { container } = render(App, { choose: async () => picked('empty'), taskProgressClient: { load: async () => ({ project_id: 'P', common_dir: '/真实项目/empty/.git', revision: '', captured_at: 1800000000000, through_seq: null, floor_seq: null, state: 'unavailable', source: { kind: 'none' }, tasks: [], runs: [], evidence: [], diagnostics: [] }) }, load: async () => {
    if (++attempts === 1) throw { message: '真实读取错误' };
    return { ...overview(), nodes: [], edges: [], revision: '', diagnostics: ['空仓库尚无提交'] };
  } });
  await fireEvent.click(screen.getByRole('button', { name: '真实项目入口' }));
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await screen.findByText('真实读取错误');
  await fireEvent.click(screen.getByRole('button', { name: '查看项目画布' }));
  await fireEvent.click(screen.getByRole('button', { name: '重试读取' }));
  await screen.findByText('当前层级尚无节点。');
  expect(container.querySelectorAll('.live-flow .svelte-flow__node')).toHaveLength(0);
  expect(screen.getByText('尚无提交')).toBeTruthy();
  expect(screen.queryByRole('complementary', { name: '节点详情' })).toBeNull();
  expect(screen.queryByRole('complementary', { name: '节点原文' })).toBeNull();
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
  expect(screen.queryByRole('button', { name: /^(批准|保存此编辑)/ })).toBeNull();
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

test('candidate handoff explicitly opens the same durable S6 ref, retains a missing-ref preview and recovers its original queue', async () => {
  const draft = candidateFixture();
  const saved = savedFixture(); saved.id = draft.scan_id; saved.draft.bundle.id = draft.scan_id; saved.draft.bundle.source_revision = draft.input.revision;
  const original: PreparedEdit = { request_id: 'pending-original', draft_id: draft.scan_id, expected_revision: saved.revision, human_actor: 'human:fixture' };
  const scanClient: CandidateClient = { start: vi.fn(async () => ({ scan_id: draft.scan_id, state: 'queued' })), wait: vi.fn(async () => ({ scan_id: draft.scan_id, state: 'completed' })), draft: vi.fn(async () => draft) };
  const savedDraftClient: DraftClient = { load: vi.fn().mockRejectedValueOnce({ code: 'draft_not_saved', message: '此 ID 尚无已保存的 S6 草稿。请让已有会话中的 Agent 使用 baseline submit 提交；扫描候选不自动转为 S6。', detail: null }).mockResolvedValueOnce(saved), pending: vi.fn(async () => [original]), prepare: vi.fn(), submit: vi.fn() };
  render(App, { choose: async () => picked('handoff'), load: async () => overview(), scanClient, savedDraftClient });
  await fireEvent.click(screen.getByRole('button', { name: '打开真实项目' }));
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await screen.findByText('Git 结构预览 · 尚未批准为模块');
  await fireEvent.click(screen.getByRole('button', { name: '查看项目画布' }));
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
  await fireEvent.click(screen.getByRole('button', { name: '打开真实项目' }));
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await screen.findByText('Git 结构预览 · 尚未批准为模块');
  await fireEvent.click(screen.getByRole('button', { name: '查看项目画布' }));
  await fireEvent.click(screen.getByRole('button', { name: '生成候选画布' }));
  await fireEvent.click(await screen.findByRole('button', { name: '打开此扫描的 S6 草稿' }));
  await waitFor(() => expect(savedDraftClient.load).toHaveBeenCalledExactlyOnceWith('old', draft.scan_id));
  expect((screen.getByRole('button', { name: '打开此扫描的 S6 草稿' }) as HTMLButtonElement).disabled).toBe(true);
  await fireEvent.click(screen.getByRole('button', { name: '切换项目' }));
  await fireEvent.click(within(screen.getByRole('region', { name: '打开你的项目' })).getByRole('button', { name: '切换项目' }));
  await screen.findByRole('heading', { name: /new\s*\./ });
  const oldSaved = savedFixture(); oldSaved.id = draft.scan_id; oldSaved.draft.bundle.id = draft.scan_id; oldSaved.draft.bundle.source_revision = draft.input.revision;
  deliver(oldSaved); await new Promise(resolve => setTimeout(resolve, 0));
  expect(screen.queryByText('本机身份：human:fixture')).toBeNull();
  expect(screen.queryByRole('button', { name: '打开此扫描的 S6 草稿' })).toBeNull();
  expect(savedDraftClient.pending).not.toHaveBeenCalled();
  expect(scanClient.start).toHaveBeenCalledTimes(1);
});


test('初始化项目提供显式当前基线草稿入口，查看项目不会准备或创建', async () => {
  render(App, { choose: async () => picked('baseline-project'), load: async () => overview() });
  await fireEvent.click(screen.getByRole('button', { name: '真实项目入口' }));
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await screen.findByText('Git 结构预览 · 尚未批准为模块');
  expect(screen.getByRole('button', { name: '创建或恢复变更草稿' })).toBeTruthy();
  expect(screen.queryByRole('button', { name: '确认创建变更草稿' })).toBeNull();
});

test('当前基线创建成功后读取真实保存草稿并复用现有编辑画布',async()=>{
 const draft=savedFixture();draft.id='created-uuid';draft.draft.bundle.id=draft.id;draft.draft.bundle.target_baseline='towermd/B1';
 const currentClient:import('./lib/current-draft').CurrentDraftClient={load:vi.fn(async()=>({baseline:'towermd/B0',main_revision:'e'.repeat(40)})),pending:vi.fn(async()=>[]),prepare:vi.fn(async()=>({request_id:draft.id,draft_id:draft.id,baseline:'towermd/B0',main_revision:'e'.repeat(40),human_actor:'human:fixture'})),submit:vi.fn(async():Promise<import('./lib/draft').EditOutcome>=>({state:'confirmed',request_id:draft.id,error:null,response:{receipt:{request_id:draft.id,body_hash:'body',result_ref:{draft_id:draft.id,revision:draft.revision,reference:`refs/towermd/drafts/${draft.id}`,content_hash:draft.draft.content_hash,delivery:{state:'local_saved'}}},current_validity:{state:'current'}}}))};
 const savedDraftClient:DraftClient={load:vi.fn(async()=>draft),pending:vi.fn(async()=>[]),prepare:vi.fn(),submit:vi.fn()};
 render(App,{choose:async()=>picked('current'),load:async()=>overview(),currentDraftClient:currentClient,savedDraftClient});
 await fireEvent.click(screen.getByRole('button',{name:'真实项目入口'}));await fireEvent.click(screen.getByRole('button',{name:'打开项目'}));await screen.findByText('Git 结构预览 · 尚未批准为模块');
 expect(currentClient.load).not.toHaveBeenCalled();await fireEvent.click(screen.getByRole('button',{name:'查看项目画布'}));await fireEvent.click(screen.getByRole('button',{name:'创建或恢复变更草稿'}));await screen.findByText('e'.repeat(40));await fireEvent.click(screen.getByRole('button',{name:'确认创建变更草稿'}));
 await screen.findByText('目标 towermd/B1');expect(savedDraftClient.load).toHaveBeenCalledWith('current','created-uuid');expect(screen.getByRole('button',{name:'保存此编辑到草稿'})).toBeTruthy();expect(screen.getByRole('button',{name:'审阅与人工决定'})).toBeTruthy();
});

test('已初始化项目提供显式恢复入口，打开项目不读取或确认恢复请求',async()=>{
 render(App,{choose:async()=>picked('recovery-project'),load:async()=>overview()});
 await fireEvent.click(screen.getByRole('button',{name:'打开真实项目'}));await fireEvent.click(screen.getByRole('button',{name:'打开项目'}));await screen.findByText('Git 结构预览 · 尚未批准为模块');
 expect(screen.getByRole('button',{name:'恢复本地 hub'})).toBeTruthy();expect(screen.queryByRole('button',{name:'确认恢复此请求'})).toBeNull();
});

test('恢复入口只在显式读取后调用原请求，切换项目丢弃旧恢复响应',async()=>{
 let choices=0;let deliver!:(value:import('./lib/recovery').RecoveryView)=>void;const recoveryClient:import('./lib/recovery').RecoveryClient={load:vi.fn(()=>new Promise<import('./lib/recovery').RecoveryView>(resolve=>deliver=resolve)),confirm:vi.fn()};
 render(App,{choose:async()=>picked(++choices===1?'old-recovery':'new-recovery'),load:async token=>overview(token),recoveryClient});await fireEvent.click(screen.getByRole('button',{name:'打开真实项目'}));await fireEvent.click(screen.getByRole('button',{name:'打开项目'}));await screen.findByText('Git 结构预览 · 尚未批准为模块');expect(recoveryClient.load).not.toHaveBeenCalled();await fireEvent.click(screen.getByRole('button',{name:'查看项目画布'}));await fireEvent.click(screen.getByRole('button',{name:'恢复本地 hub'}));expect(recoveryClient.load).not.toHaveBeenCalled();
 await fireEvent.input(screen.getByRole('textbox',{name:'已登记的恢复请求 ID'}),{target:{value:'a'.repeat(64)}});await fireEvent.click(screen.getByRole('button',{name:'读取原恢复请求'}));await waitFor(()=>expect(recoveryClient.load).toHaveBeenCalledExactlyOnceWith('old-recovery','a'.repeat(64)));
 await fireEvent.click(screen.getByRole('button',{name:'切换项目'}));await fireEvent.click(within(screen.getByRole('region',{name:'打开你的项目'})).getByRole('button',{name:'切换项目'}));await screen.findByRole('heading',{name:/new-recovery\s*\./});deliver({request:{status:'awaiting_app_confirmation',request_id:'a'.repeat(64),project_id:'old',main_revision:'b'.repeat(40),source_commit:'c'.repeat(40),source_seq:1,seq_boundary:4294967297,epoch_boundary:4294967297,source_epoch_high_water:1,source_reference:'main;refs/towermd/snapshot:missing',inputs_digest:'d'.repeat(64),requested_at:Date.now(),expires_at:Date.now()+100000,nonce:'0192f0aa-0000-7000-8000-000000000001'},human_actor:'human:old',can_confirm:true});await Promise.resolve();expect(screen.queryByRole('region',{name:'原恢复请求详情'})).toBeNull();expect(recoveryClient.confirm).not.toHaveBeenCalled();
});
