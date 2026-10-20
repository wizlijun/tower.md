import {act,cleanup,fireEvent,render,screen,waitFor} from '@testing-library/svelte';
import {afterEach,expect,test,vi} from 'vitest';
import TaskReviewCard from './TaskReviewCard.svelte';
import App from '../App.svelte';
import type {TaskReviewClient,TaskReviewView,PreparedTaskReview,TaskReviewOutcome} from '../lib/task-review';
afterEach(cleanup);
const id='H-real';const requestId='0192f0aa-0000-7000-8000-000000000001';
function fixture():TaskReviewView {
 const binding={run:'R-real',task:'T-real',baseline:'towermd/B0',fences:[{name:'module:A',holder:'R-real',epoch:7},{name:'module:B',holder:'R-real',epoch:9}],subject:{kind:'plan' as const,sha256:'a'.repeat(64)}};
 const hold={type:'Hold' as const,id,body:{kind:'review' as const,binding,verdict:null,plan_content:'第一步\r\n<script>第二步原文</script>\n',source_commit:'b'.repeat(40),source_path:'outside/plan.md',comment:null},opened_ms:1000,opened_seq:0,reply:null,future:{keep:[2,1]}};
 return {project_id:'project-real',opening:{seq:10,eid:'opening-real',host:'host',actor:'process:towermd-cli',type:'hold.opened',layer:'L5',obj:id,data:{hold},guards:binding.fences,epoch:7,stale:false},current_hold:{...hold,body:{...hold.body},opened_seq:10},binding,current_validity:{state:'current'},review_ticket:'ticket-real',human_actor:'human:fixture',original_requests:[]};
}
function original(view=fixture()):PreparedTaskReview{return {request_id:requestId,hold:id,binding:view.binding,verdict:'approved',comment:'明确检查原计划',human_actor:view.human_actor};}
function confirmed(prepared=original(),historical=false):TaskReviewOutcome{return {state:'confirmed',request_id:prepared.request_id,response:{receipt:{request_id:prepared.request_id,body_hash:'c'.repeat(64),result_ref:{hold:id,fact:{binding:prepared.binding,verdict:prepared.verdict,actor:prepared.human_actor,answer_eid:'answer-real'},verdict:prepared.verdict,comment:prepared.comment}},current_validity:historical?{state:'historical_only',reason:'原epoch已过期'}:{state:'current'}},error:null};}
function client(view=fixture()):TaskReviewClient{return {load:vi.fn(async()=>view),pending:vi.fn(async()=>[]),prepare:vi.fn(async()=>original(view)),submit:vi.fn(async()=>confirmed(original(view)))};}
async function load(hold=id){await fireEvent.input(screen.getByRole('textbox',{name:'CLI 返回的评审 Hold ID'}),{target:{value:hold}});await fireEvent.click(screen.getByRole('button',{name:'读取任务评审'}));await screen.findByRole('region',{name:'任务评审原件'});}
async function decide(){await fireEvent.click(screen.getByRole('radio',{name:'批准'}));await fireEvent.input(screen.getByRole('textbox',{name:'本次评审意见'}),{target:{value:'明确检查原计划'}});await fireEvent.click(screen.getByRole('button',{name:'保存并提交本次评审'}));}

test('显式读取前没有假数据或默认批准，非法Hold不调用native',async()=>{
 const api=client();render(TaskReviewCard,{token:'selected',client:api});expect(api.load).not.toHaveBeenCalled();expect(api.pending).not.toHaveBeenCalled();expect(screen.queryByRole('radio')).toBeNull();expect(screen.getByText('towermd review request --gate G1 --content <计划文件>')).toBeTruthy();
 await fireEvent.input(screen.getByRole('textbox'),{target:{value:'../fake'}});await fireEvent.click(screen.getByRole('button',{name:'读取任务评审'}));await screen.findByRole('alert');expect(api.load).not.toHaveBeenCalled();expect(api.prepare).not.toHaveBeenCalled();expect(api.submit).not.toHaveBeenCalled();
});
test('展示完整原计划、所有fences与不可编辑署名，明确选择后才保存和提交',async()=>{
 const view=fixture();const api=client(view);const {container}=render(TaskReviewCard,{token:'selected',client:api});await load();
 expect(screen.getByLabelText('完整计划原文').textContent).toBe(view.opening.data.hold.body.plan_content);expect(container.querySelector('script')).toBeNull();
 const details=screen.getByRole('region',{name:'任务评审原件'});for(const text of ['R-real','T-real','towermd/B0','module:A','module:B','opening-real','outside/plan.md','b'.repeat(40),'a'.repeat(64),'human:fixture'])expect(details.textContent).toContain(text);
 for(const radio of screen.getAllByRole('radio'))expect((radio as HTMLInputElement).checked).toBe(false);
 expect((screen.getByRole('button',{name:'保存并提交本次评审'}) as HTMLButtonElement).disabled).toBe(true);expect(api.prepare).not.toHaveBeenCalled();expect(api.submit).not.toHaveBeenCalled();
 await decide();await screen.findByRole('region',{name:'任务评审提交结果'});expect(api.prepare).toHaveBeenCalledExactlyOnceWith('selected','ticket-real','approved','明确检查原计划');expect(api.submit).toHaveBeenCalledExactlyOnceWith('selected',requestId);expect(screen.getByText('批准已记录')).toBeTruthy();expect(screen.queryByRole('radio')).toBeNull();
});
test('不确定保原UUID，重试只提交原请求，不重新prepare或自动批准',async()=>{
 const api=client();api.submit=vi.fn().mockResolvedValueOnce({state:'uncertain',request_id:requestId,response:null,error:{code:'request_uncertain',message:'确认写入不确定',detail:null}}).mockResolvedValueOnce(confirmed(original(),true));render(TaskReviewCard,{token:'selected',client:api});await load();await decide();
 await screen.findByText('确认写入不确定');expect(screen.getByRole('region',{name:'原任务评审请求'}).textContent).toContain(requestId);expect(screen.queryByRole('radio')).toBeNull();
 await fireEvent.click(screen.getByRole('button',{name:'重试原请求'}));await screen.findByText('原epoch已过期',{exact:true});expect(api.prepare).toHaveBeenCalledTimes(1);expect(vi.mocked(api.submit).mock.calls).toEqual([['selected',requestId],['selected',requestId]]);expect(screen.getByText('原评审回执已确认 · 仅历史效力')).toBeTruthy();expect(screen.queryByText('批准已记录')).toBeNull();
});
test('准备响应丢失后读取原队列，不能为未知结果重新创建请求',async()=>{
 const api=client();api.pending=vi.fn().mockResolvedValueOnce([]).mockResolvedValueOnce([original()]);api.prepare=vi.fn(async()=>{throw {code:'transport',message:'准备响应丢失'};});render(TaskReviewCard,{token:'selected',client:api});await load();await decide();await screen.findByRole('region',{name:'原任务评审请求'});expect(api.prepare).toHaveBeenCalledTimes(1);expect(api.submit).not.toHaveBeenCalled();expect(screen.queryByRole('radio')).toBeNull();await fireEvent.click(screen.getByRole('button',{name:'重试原请求'}));await screen.findByRole('region',{name:'任务评审提交结果'});expect(api.submit).toHaveBeenCalledExactlyOnceWith('selected',requestId);expect(api.prepare).toHaveBeenCalledTimes(1);
});
test('准备返回另一份绑定或意见时只读取原队列，不自动提交',async()=>{
 const api=client();api.prepare=vi.fn(async()=>({...original(),comment:'另一份意见'}));api.pending=vi.fn().mockResolvedValueOnce([]).mockResolvedValueOnce([{...original(),comment:'另一份意见'}]);render(TaskReviewCard,{token:'selected',client:api});await load();await decide();await screen.findByText(/不会自动提交/);expect(api.prepare).toHaveBeenCalledTimes(1);expect(api.submit).not.toHaveBeenCalled();expect(screen.getByRole('region',{name:'原任务评审请求'}).textContent).toContain('另一份意见');expect(screen.queryByRole('radio')).toBeNull();
});
test('历史评审与已有terminal回答只读，G4显示实际tree而不假装计划',async()=>{
 const old=fixture();old.current_validity={state:'historical_only',reason:'currentPlan已替换'};old.opening.stale=true;old.current_hold.opened_seq=0;const panel=render(TaskReviewCard,{token:'selected',client:client(old)});await load();expect(screen.getByText(/currentPlan已替换/)).toBeTruthy();expect(screen.queryByRole('radio')).toBeNull();panel.unmount();
 const tree=fixture();tree.binding.subject={kind:'tree',tree:'d'.repeat(40)};tree.opening.data.hold.body.plan_content=null;tree.current_hold.body.plan_content=null;tree.current_hold.body.verdict='rejected';tree.current_hold.body.comment='原拒绝意见';tree.current_hold.reply={id:'reply-real',text:'原拒绝意见',actor:'human:owner',at_ms:1100};render(TaskReviewCard,{token:'selected',client:client(tree)});await load();expect(screen.getByLabelText('实际评审 tree').textContent).toBe('d'.repeat(40));expect(screen.queryByLabelText('完整计划原文')).toBeNull();expect(screen.getByText('此前已记录：拒绝')).toBeTruthy();expect(screen.getByText('原拒绝意见')).toBeTruthy();expect(screen.queryByRole('radio')).toBeNull();
});
test('输入换Hold或切项目后立即清显示，迟到原件与准备响应不再显示或发送',async()=>{
 let deliver!:(view:TaskReviewView)=>void;const api=client();api.load=vi.fn(()=>new Promise<TaskReviewView>(resolve=>deliver=resolve));const panel=render(TaskReviewCard,{token:'old',client:api});await fireEvent.input(screen.getByRole('textbox'),{target:{value:id}});await fireEvent.click(screen.getByRole('button',{name:'读取任务评审'}));await waitFor(()=>expect(api.load).toHaveBeenCalledTimes(1));await fireEvent.input(screen.getByRole('textbox'),{target:{value:'H-new'}});await act(()=>deliver(fixture()));expect(screen.queryByRole('region',{name:'任务评审原件'})).toBeNull();
 api.load=vi.fn(async()=>fixture());await load();let prepared!:(value:PreparedTaskReview)=>void;api.prepare=vi.fn(()=>new Promise<PreparedTaskReview>(resolve=>prepared=resolve));await decide();await waitFor(()=>expect(api.prepare).toHaveBeenCalledTimes(1));await panel.rerender({token:'new',client:api});await act(()=>prepared(original()));expect(screen.queryByRole('region',{name:'任务评审原件'})).toBeNull();expect((screen.getByRole('textbox') as HTMLInputElement).value).toBe('');expect(api.submit).not.toHaveBeenCalled();
});
test('配置项目提供显式任务评审入口，展开不会自动读取或提交',async()=>{
 const api=client();render(App,{choose:async()=>({token:'selected',displayPath:'/local/project',hasConfig:true}),load:async()=>({source:'repository' as const,revision:'f'.repeat(40),title:'真实项目',nodes:[],edges:[],diagnostics:[]}),taskReviewClient:api});await fireEvent.click(screen.getByRole('button',{name:'打开项目'}));await screen.findByRole('region',{name:'真实项目画布'});await fireEvent.contextMenu(screen.getByRole('main'));await fireEvent.click(screen.getByRole('menuitem',{name:'项目工具'}));await screen.findByRole('button',{name:'任务评审'});await fireEvent.click(screen.getByRole('button',{name:'任务评审'}));expect(screen.getByRole('region',{name:'任务人工评审'})).toBeTruthy();expect(api.load).not.toHaveBeenCalled();expect(api.prepare).not.toHaveBeenCalled();expect(api.submit).not.toHaveBeenCalled();
});
