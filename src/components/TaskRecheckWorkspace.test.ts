import {readonlyControlsFixture} from '../lib/interventions.fixture';
import {act,cleanup,fireEvent,render,screen,waitFor,within} from '@testing-library/svelte';
import {afterEach,expect,test,vi} from 'vitest';
import LiveWorkspace from './LiveWorkspace.svelte';
import type {TaskProgressView,TaskProgressClient,ProgressChange} from '../lib/task-progress';
import type {ProjectOverview} from '../lib/project';
import type {TaskRecheckClient,TaskRecheckView,PreparedTaskRecheck,TaskRecheckOutcome} from '../lib/task-recheck';
import {recheckView,preparedRecheck,recheckOutcome} from '../lib/task-recheck.test-fixtures';
import {taskFlow} from '../lib/live-overview';
afterEach(cleanup);
const revision='a'.repeat(40);
const overview:ProjectOverview={source:'baseline',revision,title:'project',nodes:[],edges:[],diagnostics:[]};
function progress(marker:unknown):TaskProgressView{return {project_id:'project',common_dir:'/project/.git',revision,captured_at:1,through_seq:1,floor_seq:0,state:'recorded',source:{kind:'hub_journal'},tasks:[{type:'Task',id:'T',title:'真实 Task',module:'A',state:'ready',needs_recheck:marker}],runs:[],evidence:[],diagnostics:[]};}
function control():TaskRecheckClient{return {load:vi.fn(async()=>recheckView()),pending:vi.fn(async()=>[]),history:vi.fn(async()=>[]),prepare:vi.fn(async()=>preparedRecheck()),submit:vi.fn(async()=>recheckOutcome())};}
test('Z2 仅真实 needs_recheck 标记显示待复核入口，打开只读不触发解除',async()=>{
 const recheckClient=control();render(LiveWorkspace,{interventionClient:readonlyControlsFixture(),token:'selected',overview,client:{load:async()=>progress(true)},recheckClient});await screen.findByText(/任务 1/);
 await fireEvent.contextMenu(screen.getByRole('region',{name:'真实项目画布'}));await fireEvent.click(screen.getByRole('menuitem',{name:'态势与关注'}));await fireEvent.click(screen.getByText('未绑定画布模块的任务'));await fireEvent.click(screen.getByRole('button',{name:'真实 Task · 就绪'}));await screen.findByText('↻ 需复核 · 当前基线影响尚待人工核对');expect(recheckClient.load).not.toHaveBeenCalled();
 await fireEvent.click(screen.getByRole('button',{name:'打开复核原件'}));await screen.findByLabelText('当前 Task 完整文本');expect(recheckClient.load).toHaveBeenCalledWith('selected','T');expect(recheckClient.history).not.toHaveBeenCalled();expect(recheckClient.prepare).not.toHaveBeenCalled();expect(recheckClient.submit).not.toHaveBeenCalled();
});
test('字符串标记不造待复核入口；真实快层角标变化不移动节点',async()=>{
 render(LiveWorkspace,{interventionClient:readonlyControlsFixture(),token:'selected',overview,client:{load:async()=>progress('true')},recheckClient:control()});await screen.findByText(/任务 1/);await fireEvent.contextMenu(screen.getByRole('region',{name:'真实项目画布'}));await fireEvent.click(screen.getByRole('menuitem',{name:'态势与关注'}));await fireEvent.click(screen.getByText('未绑定画布模块的任务'));await fireEvent.click(screen.getByRole('button',{name:'真实 Task · 就绪'}));expect(screen.queryByRole('button',{name:'打开复核原件'})).toBeNull();
 const positions=new Map<string,{x:number;y:number}>();const before=taskFlow(progress(false).tasks,positions);const after=taskFlow(progress(true).tasks,positions);expect(after.nodes[0].position).toEqual(before.nodes[0].position);expect(after.nodes[0].data.text).toContain('↻ 需复核');expect(before.nodes[0].data.text).not.toContain('↻ 需复核');
});


const boundOverview:ProjectOverview={...overview,nodes:[{id:'module-A',obj:'A',objectType:'Module',title:'真实模块 A',kind:'text',status:'bound',text:'原模块正文',x:25,y:60,width:300,height:180}]};
async function enterTask(container:HTMLElement,id='T'){
 await waitFor(()=>expect(container.querySelector('.live-flow .svelte-flow__node[data-id="module-A"]')).not.toBeNull());
 await fireEvent.click(container.querySelector<HTMLElement>('.live-flow .svelte-flow__node[data-id="module-A"]')!);
 await fireEvent.click(screen.getByRole('button',{name:'进入模块任务'}));
 await selectTask(container,id);
}
async function selectTask(container:HTMLElement,id='T'){
 await waitFor(()=>expect(container.querySelector(`.live-flow .svelte-flow__node[data-id="${id}"]`)).not.toBeNull());
 await fireEvent.click(container.querySelector<HTMLElement>(`.live-flow .svelte-flow__node[data-id="${id}"]`)!);
}
function readySubject():TaskRecheckView{
 const value=recheckView();value.binding.running=null;value.task_text='type: Task\nid: T\nstate: ready\n原 Ready 完整正文';return value;
}
function clearedSubject(original:PreparedTaskRecheck):TaskRecheckView{
 const value=readySubject();value.binding.publication_ids=[];value.reasons=[];value.original_requests=[original];return value;
}

test('迟到 S 清除标记后，关闭重开 Task 按需恢复原 UUID，再重开读取已确认原回执且无新解除',async()=>{
 const subject=readySubject(),original=preparedRecheck(subject);let view=subject;let originals:PreparedTaskRecheck[]=[];
 let flagged=true,seq=1,attempts=0;let notify!: (value:ProgressChange)=>void;
 const progressClient:TaskProgressClient={load:vi.fn(async()=>({...progress(flagged),through_seq:seq})),subscribe:vi.fn(async(_token,_after,changed)=>{notify=changed;return vi.fn<()=>void>();})};
 const client:TaskRecheckClient={load:vi.fn(async()=>view),pending:vi.fn(async()=>originals),history:vi.fn(async()=>view.original_requests),prepare:vi.fn(async()=>{originals=[original];return original;}),submit:vi.fn(async():Promise<TaskRecheckOutcome>=>{
  if(++attempts===1)return {state:'uncertain',request_id:original.request_id,response:null,error:{code:'request_uncertain',message:'原 HTTP 结果待核实',detail:null}};
  originals=[];return recheckOutcome(original);
 })};
 const {container}=render(LiveWorkspace,{interventionClient:readonlyControlsFixture(),token:'selected',overview:boundOverview,client:progressClient,recheckClient:client});
 await enterTask(container);
 expect(client.pending).not.toHaveBeenCalled();expect(client.load).not.toHaveBeenCalled();expect(client.prepare).not.toHaveBeenCalled();expect(client.submit).not.toHaveBeenCalled();
 await fireEvent.click(screen.getByRole('button',{name:'打开复核原件'}));
 await screen.findByRole('button',{name:'确认已复核并解除本次标记'});
 await fireEvent.input(screen.getByLabelText('本次复核意见'),{target:{value:original.comment}});
 await fireEvent.click(screen.getByRole('button',{name:'确认已复核并解除本次标记'}));
 await screen.findByText('原 HTTP 结果待核实');
 // The server's original S becomes visible after the uncertain HTTP response.
 flagged=false;seq=2;view=clearedSubject(original);
 notify({kind:'changed',through_seq:seq,floor_seq:0,revision});
 await waitFor(()=>expect(screen.queryByText('↻ 需复核 · 当前基线影响尚待人工核对')).toBeNull());
 expect(client.history).not.toHaveBeenCalled();expect(screen.getByRole('region',{name:'原复核请求'})).toBeTruthy();
 const node=()=>container.querySelector<HTMLElement>('.live-flow .svelte-flow__node[data-id="T"]')!;
 const position=node().style.transform;
 await fireEvent.click(screen.getByRole('button',{name:'关闭详情'}));await selectTask(container);
 expect(screen.queryByRole('button',{name:'打开复核原件'})).toBeNull();
 expect(screen.getByRole('button',{name:'查看复核记录'})).toBeTruthy();
 expect(client.load).toHaveBeenCalledTimes(1);expect(client.pending).toHaveBeenCalledTimes(1);expect(client.submit).toHaveBeenCalledTimes(1);
 await fireEvent.click(screen.getByRole('button',{name:'查看复核记录'}));
 await screen.findByRole('region',{name:'原复核请求'});
 await waitFor(()=>expect(client.history).toHaveBeenCalledExactlyOnceWith('selected','T'));expect(client.load).toHaveBeenCalledTimes(1);
 expect(screen.queryByRole('button',{name:'确认已复核并解除本次标记'})).toBeNull();
 expect(screen.queryByRole('textbox',{name:'本次复核意见'})).toBeNull();
 expect(screen.getAllByRole('button',{name:'重试原复核请求'})).toHaveLength(1);
 expect(screen.queryByRole('button',{name:'读取原复核回执'})).toBeNull();
 const pending=screen.getByRole('region',{name:'原复核请求'});expect(pending.textContent).toContain(original.request_id);expect(pending.textContent).toContain(original.body_hash);
 const retry=screen.getByRole('button',{name:'重试原复核请求'});await waitFor(()=>expect((retry as HTMLButtonElement).disabled).toBe(false));await fireEvent.click(retry);
 const receipt=await screen.findByRole('region',{name:'复核原回执'});expect(receipt.textContent).toContain(original.request_id);expect(receipt.textContent).toContain('actual-native-verified-event');
 await fireEvent.click(screen.getByRole('button',{name:'关闭详情'}));await selectTask(container);
 expect(client.load).toHaveBeenCalledTimes(1);await fireEvent.click(screen.getByRole('button',{name:'查看复核记录'}));
 const summary=await screen.findByText('本机原复核请求（1）');expect(client.load).toHaveBeenCalledTimes(1);expect(client.history).toHaveBeenCalledTimes(2);await fireEvent.click(summary);const history=summary.closest('details')!;
 expect(history.textContent).toContain(original.request_id);expect(history.textContent).toContain(original.body_hash);expect(JSON.parse(history.querySelector('pre')!.textContent!)).toEqual(original);
 expect(screen.queryByRole('button',{name:'确认已复核并解除本次标记'})).toBeNull();
 expect(screen.queryByRole('button',{name:'重试原复核请求'})).toBeNull();
 await fireEvent.click(within(history).getByRole('button',{name:'读取原复核回执'}));
 await screen.findByRole('region',{name:'复核原回执'});
 expect(client.prepare).toHaveBeenCalledExactlyOnceWith('selected',subject.recheck_ticket,original.comment);
 expect(vi.mocked(client.submit).mock.calls).toEqual([['selected',original.request_id],['selected',original.request_id],['selected',original.request_id]]);
 expect(node().style.transform).toBe(position);
});

test.each(['history','mismatch'])('历史或正在同步的 Task 记录不开放任何复核入口（%s）',async mode=>{
 const client=control();let current=progress(true);let notify!: (value:ProgressChange)=>void;
 const progressClient:TaskProgressClient={load:async()=>current,subscribe:async(_token,_after,changed)=>{notify=changed;return vi.fn<()=>void>();}};
 const {container}=render(LiveWorkspace,{interventionClient:readonlyControlsFixture(),token:'selected',overview:boundOverview,client:progressClient,recheckClient:client,onrevision:vi.fn(async()=>{})});
 await enterTask(container);expect(screen.getByRole('button',{name:'打开复核原件'})).toBeTruthy();
 current=mode==='history'?{...progress(true),through_seq:2,floor_seq:null,state:'recovery_required',source:{kind:'recovery_snapshot',commit:revision,reference:'main'}}:{...progress(true),through_seq:2,revision:'b'.repeat(40)};
 notify({kind:'changed',through_seq:2,floor_seq:0,revision:current.revision});
 await screen.findByText('历史或版本同步中的记录只读；等待重新核实当前状态。');
 expect(screen.queryByRole('button',{name:'打开复核原件'})).toBeNull();expect(screen.queryByRole('button',{name:'查看复核记录'})).toBeNull();
 expect(client.pending).not.toHaveBeenCalled();expect(client.load).not.toHaveBeenCalled();expect(client.history).not.toHaveBeenCalled();expect(client.prepare).not.toHaveBeenCalled();expect(client.submit).not.toHaveBeenCalled();
});

test.each(['hash','subject','task'])('复核历史未通过完整原件校验时不产生可提交的历史动作（%s）',async kind=>{
 const original=preparedRecheck(readySubject());if(kind==='hash')original.body_hash='invalid';else if(kind==='subject')original.subject.binding.target_baseline='towermd/B3';else original.binding.task_id='U';const client=control();vi.mocked(client.history).mockResolvedValue([original]);
 const {container}=render(LiveWorkspace,{interventionClient:readonlyControlsFixture(),token:'selected',overview:boundOverview,client:{load:async()=>progress(false)},recheckClient:client});
 await enterTask(container);await fireEvent.click(screen.getByRole('button',{name:'查看复核记录'}));
 await screen.findByRole('alert');expect(screen.queryByRole('button',{name:'读取原复核回执'})).toBeNull();expect(screen.queryByRole('button',{name:'确认已复核并解除本次标记'})).toBeNull();
 expect(client.prepare).not.toHaveBeenCalled();expect(client.submit).not.toHaveBeenCalled();
});

test('查看另一 Task 的复核记录后拒绝旧 Task 迟到原件，不自动开启旧确认',async()=>{
 let deliver!: (values:PreparedTaskRecheck[])=>void;const subject=readySubject();subject.binding.task_id='U';subject.task_text='Task U 保存的原完整正文';subject.reasons=subject.reasons.map(fact=>({...fact,task:'U'}));const original=preparedRecheck(subject);
 const client=control();vi.mocked(client.history).mockImplementation(async(_token,id)=>id==='T'?new Promise<PreparedTaskRecheck[]>(resolve=>{deliver=resolve;}):[original]);
 const value=progress(false);value.tasks.push({...value.tasks[0],id:'U',title:'另一 Task'});
 const {container}=render(LiveWorkspace,{interventionClient:readonlyControlsFixture(),token:'selected',overview:boundOverview,client:{load:async()=>value},recheckClient:client});
 await enterTask(container);await fireEvent.click(screen.getByRole('button',{name:'查看复核记录'}));await waitFor(()=>expect(client.history).toHaveBeenCalledWith('selected','T'));
 await fireEvent.click(screen.getByRole('button',{name:'关闭详情'}));await selectTask(container,'U');await fireEvent.click(screen.getByRole('button',{name:'查看复核记录'}));
 const summary=await screen.findByText('本机原复核请求（1）');await fireEvent.click(summary);await act(()=>deliver([preparedRecheck(readySubject())]));
 expect(JSON.parse(summary.closest('details')!.querySelector('pre')!.textContent!)).toEqual(original);expect(screen.queryByRole('button',{name:'确认已复核并解除本次标记'})).toBeNull();
 expect(client.load).not.toHaveBeenCalled();expect(client.prepare).not.toHaveBeenCalled();expect(client.submit).not.toHaveBeenCalled();
});

test.each(['merged','released'])('已完成 Task（%s）显式读本机历史，不依赖 ready/running GET，不把已 resolved 原件当待确认',async state=>{
 const original=preparedRecheck(readySubject()),client=control();vi.mocked(client.load).mockRejectedValue(new Error('requires ready/running'));
 vi.mocked(client.history).mockResolvedValue([original]);const outcome=recheckOutcome(original);outcome.response!.current_validity={state:'historical_only',reason:'当前 Task 已完成；原回执仅有历史效力'};vi.mocked(client.submit).mockResolvedValue(outcome);
 const value=progress(false);value.tasks[0].state=state;
 const {container}=render(LiveWorkspace,{interventionClient:readonlyControlsFixture(),token:'selected',overview:boundOverview,client:{load:async()=>value},recheckClient:client});
 await enterTask(container);expect(client.pending).not.toHaveBeenCalled();expect(client.history).not.toHaveBeenCalled();expect(client.load).not.toHaveBeenCalled();
 await fireEvent.click(screen.getByRole('button',{name:'查看复核记录'}));const summary=await screen.findByText('本机原复核请求（1）');await fireEvent.click(summary);
 expect(JSON.parse(summary.closest('details')!.querySelector('pre')!.textContent!)).toEqual(original);
 expect(screen.queryByRole('region',{name:'原复核请求'})).toBeNull();expect(screen.queryByRole('region',{name:'复核原回执'})).toBeNull();expect(screen.queryByRole('button',{name:'重试原复核请求'})).toBeNull();expect(screen.queryByRole('button',{name:'确认已复核并解除本次标记'})).toBeNull();expect(client.submit).not.toHaveBeenCalled();
 await fireEvent.click(screen.getByRole('button',{name:'读取原复核回执'}));const receipt=await screen.findByRole('region',{name:'复核原回执'});
 expect(receipt.textContent).toContain('仅历史效力');expect(receipt.textContent).toContain(original.request_id);expect(receipt.textContent).toContain('actual-native-verified-event');
 expect(client.pending).toHaveBeenCalledExactlyOnceWith('selected','T');expect(client.history).toHaveBeenCalledExactlyOnceWith('selected','T');expect(client.load).not.toHaveBeenCalled();expect(client.prepare).not.toHaveBeenCalled();expect(client.submit).toHaveBeenCalledExactlyOnceWith('selected',original.request_id);
});

test('当前复核确认后真实刷新清除角标仍保留原回执，不自动切换历史模式',async()=>{
 const client=control(),original=preparedRecheck();let flagged=true;
 const progressClient:TaskProgressClient={load:vi.fn(async()=>progress(flagged))};vi.mocked(client.submit).mockImplementation(async()=>{flagged=false;return recheckOutcome(original);});
 const {container}=render(LiveWorkspace,{interventionClient:readonlyControlsFixture(),token:'selected',overview:boundOverview,client:progressClient,recheckClient:client});await enterTask(container);
 await fireEvent.click(screen.getByRole('button',{name:'打开复核原件'}));await screen.findByRole('button',{name:'确认已复核并解除本次标记'});await fireEvent.input(screen.getByLabelText('本次复核意见'),{target:{value:original.comment}});await fireEvent.click(screen.getByRole('button',{name:'确认已复核并解除本次标记'}));
 await screen.findByRole('region',{name:'复核原回执'});await waitFor(()=>expect(screen.queryByText('↻ 需复核 · 当前基线影响尚待人工核对')).toBeNull());
 expect(screen.getByRole('region',{name:'复核原回执'}).textContent).toContain('本次复核已记录 · 当前绑定有效');expect(screen.getByRole('region',{name:'复核原回执'}).textContent).toContain(original.request_id);
 expect(screen.getByRole('button',{name:'查看复核记录'})).toBeTruthy();expect(client.history).not.toHaveBeenCalled();expect(client.load).toHaveBeenCalledExactlyOnceWith('selected','T');expect(client.prepare).toHaveBeenCalledExactlyOnceWith('selected',recheckView().recheck_ticket,original.comment);expect(client.submit).toHaveBeenCalledExactlyOnceWith('selected',original.request_id);
});
