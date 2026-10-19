import {cleanup,fireEvent,render,screen,waitFor} from '@testing-library/svelte';
import {afterEach,expect,test,vi} from 'vitest';
import ReviewPanel from './ReviewPanel.svelte';
import {savedFixture} from '../lib/draft.fixture';
import type {ReviewView,PreparedReview,ReviewClient,ReviewResponse,ReviewOutcome} from '../lib/review';
afterEach(cleanup);
const saved=savedFixture();
const binding={project_id:saved.project_id,object_id:saved.id,content_hash:saved.draft.content_hash,target_baseline:saved.draft.bundle.target_baseline,draft_revision:saved.revision};
const original:PreparedReview={request_id:'01900000-0000-7000-8000-000000000001',binding,verdict:'approved',comment:'逐项审阅',human_actor:'human:fixture'};
const view:ReviewView={binding,review_ticket:'01900000-0000-7000-8000-000000000002',human_actor:'human:fixture',current_baseline:null,draft:saved.draft,diff:[{path:'docs/tower/module.md',before:'原始正文',after:'候选完整正文'}],interface_modules:['A','B'],publish_blockers:[],verification:{state:'unavailable',reason:'固定报告不在可用历史内'}};
function response(state='pending'):ReviewResponse{return {receipt:{request_id:original.request_id,body_hash:'b'.repeat(64),result_ref:{approval_eid:'approval-original',binding,actor:'human:fixture',verdict:'approved',comment:original.comment}},current_validity:{state:'current'},publication:{state,reason:state==='pending'?'main 仍被检出':undefined}};}
function client():ReviewClient{return {load:vi.fn(async()=>view),pending:vi.fn(async()=>[]),prepare:vi.fn(async()=>original),submit:vi.fn(async():Promise<ReviewOutcome>=>({state:'confirmed',request_id:original.request_id,response:response(),error:null}))};}
test('review displays exact binding and complete text before a deliberate decision; approval and publication differ',async()=>{
 const api=client();render(ReviewPanel,{token:'selected',id:'draft-1',client:api,onclose:vi.fn()});
 await screen.findByText('候选完整正文');expect(screen.getByText('原始正文')).toBeTruthy();expect(screen.getByText(binding.draft_revision)).toBeTruthy();expect(screen.getByText(/固定报告不在可用历史内/,{selector:'p'})).toBeTruthy();expect(api.prepare).not.toHaveBeenCalled();expect(api.submit).not.toHaveBeenCalled();
 await waitFor(()=>expect((screen.getByRole('button',{name:'批准此版本并出版'}) as HTMLButtonElement).disabled).toBe(false));
 await fireEvent.input(screen.getByRole('textbox',{name:'审阅意见'}),{target:{value:'逐项审阅'}});await fireEvent.click(screen.getByRole('button',{name:'批准此版本并出版'}));
 await screen.findByText('人工批准已记录');expect(api.prepare).toHaveBeenCalledWith('selected','01900000-0000-7000-8000-000000000002','approved','逐项审阅');expect(api.submit).toHaveBeenCalledWith('selected',original.request_id);expect(screen.getByText('批准已记录 · 基线出版待完成')).toBeTruthy();expect(screen.queryByText('基线出版已记录')).toBeNull();
 await waitFor(()=>expect((screen.getByRole('button',{name:'重读原批准与出版状态'}) as HTMLButtonElement).disabled).toBe(false));await fireEvent.click(screen.getByRole('button',{name:'重读原批准与出版状态'}));await waitFor(()=>expect(api.submit).toHaveBeenCalledTimes(2));expect(api.prepare).toHaveBeenCalledTimes(1);
});
test('uncertain retry uses the original id and a conflict requires explicit review reload without reapproval',async()=>{
 let attempts=0;let pending:PreparedReview[]=[];const api=client();api.pending=vi.fn(async()=>pending);api.prepare=vi.fn(async()=>{pending=[original];return original;});api.submit=vi.fn(async():Promise<ReviewOutcome>=>++attempts===1?{state:'uncertain',request_id:original.request_id,response:null,error:{code:'request_uncertain',message:'原请求尚待确认',detail:null}}:(pending=[],{state:'conflict',request_id:original.request_id,response:null,error:{code:'request_conflict',message:'binding 已变化',detail:null}}));
 render(ReviewPanel,{token:'selected',id:'draft-1',client:api,onclose:vi.fn()});await screen.findByText('候选完整正文');await waitFor(()=>expect((screen.getByRole('button',{name:'批准此版本并出版'}) as HTMLButtonElement).disabled).toBe(false));await fireEvent.input(screen.getByRole('textbox',{name:'审阅意见'}),{target:{value:original.comment}});await fireEvent.click(screen.getByRole('button',{name:'批准此版本并出版'}));await screen.findByRole('region',{name:'原审阅请求等待确认'});
 await waitFor(()=>expect((screen.getByRole('button',{name:'重试原审阅请求'}) as HTMLButtonElement).disabled).toBe(false));await fireEvent.click(screen.getByRole('button',{name:'重试原审阅请求'}));await screen.findByText(/此 binding 已失效/);
 expect(api.prepare).toHaveBeenCalledTimes(1);expect(api.submit).toHaveBeenNthCalledWith(2,'selected',original.request_id);expect(api.load).toHaveBeenCalledTimes(1);expect((screen.getByRole('button',{name:'批准此版本并出版'}) as HTMLButtonElement).disabled).toBe(true);
});
test('reopened original request is displayed and only explicit retry coordinates the original decision',async()=>{
 const api=client();api.pending=vi.fn(async()=>[original]);render(ReviewPanel,{token:'selected',id:'draft-1',client:api,onclose:vi.fn()});await screen.findByRole('region',{name:'原审阅请求等待确认'});await screen.findByText('候选完整正文');expect(api.submit).not.toHaveBeenCalled();expect(api.prepare).not.toHaveBeenCalled();expect((screen.getByRole('button',{name:'批准此版本并出版'}) as HTMLButtonElement).disabled).toBe(true);
});

test('explicit rejection sends rejected only and a late project response cannot render after destruction',async()=>{
 const api=client();api.prepare=vi.fn(async()=>({...original,verdict:'rejected' as const,comment:''}));api.submit=vi.fn(async():Promise<ReviewOutcome>=>({state:'confirmed',request_id:original.request_id,response:{...response(),receipt:{...response().receipt,result_ref:{...response().receipt.result_ref,verdict:'rejected' as const,comment:''}}},error:null}));
 const current=render(ReviewPanel,{token:'selected',id:'draft-1',client:api,onclose:vi.fn()});await screen.findByText('候选完整正文');await waitFor(()=>expect((screen.getByRole('button',{name:'拒绝此版本'}) as HTMLButtonElement).disabled).toBe(false));await fireEvent.click(screen.getByRole('button',{name:'拒绝此版本'}));await screen.findByText('人工拒绝已记录');expect(api.prepare).toHaveBeenCalledWith('selected','01900000-0000-7000-8000-000000000002','rejected','');expect(screen.queryByText('基线出版已记录')).toBeNull();current.unmount();
 let deliver!:(value:ReviewView)=>void;const delayed=client();delayed.load=vi.fn(()=>new Promise<ReviewView>(resolve=>deliver=resolve));const old=render(ReviewPanel,{token:'old',id:'draft-1',client:delayed,onclose:vi.fn()});await waitFor(()=>expect(delayed.load).toHaveBeenCalled());old.unmount();deliver(view);await Promise.resolve();expect(screen.queryByText('候选完整正文')).toBeNull();expect(delayed.prepare).not.toHaveBeenCalled();
});

test('review exposes a read-only current and proposed canvas comparison before a decision',async()=>{
 const api=client();api.load=vi.fn(async()=>({...view,main_revision:'d'.repeat(40),current_baseline:'towermd/B0',canvas_comparison:{state:'available' as const,main_revision:'d'.repeat(40),baseline:'towermd/B0',baseline_canvas:{canvas_view:saved.canvas_view,object_views:saved.object_views},draft_canvas:{canvas_view:saved.canvas_view,object_views:saved.object_views},diagnostics:[]}}));
 render(ReviewPanel,{token:'selected',id:'draft-1',client:api,onclose:vi.fn()});
 await screen.findByRole('region',{name:'主干与草稿画布对照'});
 expect(screen.getByRole('button',{name:'叠加'}).getAttribute('aria-pressed')).toBe('true');
 await fireEvent.click(screen.getByRole('button',{name:'主干'}));
 await fireEvent.click(screen.getByRole('button',{name:'草稿'}));
 expect(api.prepare).not.toHaveBeenCalled();expect(api.submit).not.toHaveBeenCalled();expect(api.load).toHaveBeenCalledTimes(1);
});

test('older service without comparison declares it unavailable instead of inventing a baseline graph',async()=>{
 const api=client();render(ReviewPanel,{token:'selected',id:'draft-1',client:api,onclose:vi.fn()});
 await screen.findByText(/服务端尚未返回权威 main 与草稿画布/,{selector:'p'});
 expect(screen.queryByRole('group',{name:'画布对照模式'})).toBeNull();expect(api.prepare).not.toHaveBeenCalled();expect(api.submit).not.toHaveBeenCalled();
});
test('reload clears the previous comparison immediately and late destroyed load cannot leave its graph',async()=>{
 const api=client();const available:ReviewView={...view,main_revision:'e'.repeat(40),current_baseline:'towermd/B0',canvas_comparison:{state:'available',main_revision:'e'.repeat(40),baseline:'towermd/B0',baseline_canvas:{canvas_view:saved.canvas_view,object_views:saved.object_views},draft_canvas:{canvas_view:saved.canvas_view,object_views:saved.object_views},diagnostics:[]}};
 let deliver!:(value:ReviewView)=>void;api.load=vi.fn().mockResolvedValueOnce(available).mockImplementationOnce(()=>new Promise<ReviewView>(resolve=>deliver=resolve));
 const panel=render(ReviewPanel,{token:'selected',id:'draft-1',client:api,onclose:vi.fn()});await screen.findByRole('group',{name:'画布对照模式'});
 await waitFor(()=>expect((screen.getByRole('button',{name:'重新加载审阅'}) as HTMLButtonElement).disabled).toBe(false));
 await fireEvent.click(screen.getByRole('button',{name:'重新加载审阅'}));
 await waitFor(()=>expect(api.load).toHaveBeenCalledTimes(2));expect(screen.queryByRole('group',{name:'画布对照模式'})).toBeNull();panel.unmount();deliver(available);await Promise.resolve();expect(screen.queryByRole('region',{name:'主干与草稿画布对照'})).toBeNull();expect(api.prepare).not.toHaveBeenCalled();expect(api.submit).not.toHaveBeenCalled();
});
test('prepare返回另一个完整版本时保留原请求但不提交，不能用旧B0界面批准Bn',async()=>{
 const api=client();api.prepare=vi.fn(async()=>({...original,binding:{...binding,target_baseline:'towermd/B1'}}));
 render(ReviewPanel,{token:'selected',id:'draft-1',client:api,onclose:vi.fn()});await screen.findByText('候选完整正文');
 await fireEvent.input(screen.getByRole('textbox',{name:'审阅意见'}),{target:{value:original.comment}});
 await fireEvent.click(screen.getByRole('button',{name:'批准此版本并出版'}));
 await screen.findByText('已保存原决定与刚审阅的完整版本、身份或意见不同；不会提交。');
 expect(api.submit).not.toHaveBeenCalled();expect((screen.getByRole('button',{name:'重试原审阅请求'}) as HTMLButtonElement).disabled).toBe(true);
});
test('返回回执的草稿hash不同不显示人工批准成功并保留原UUID',async()=>{
 const api=client();api.submit=vi.fn(async()=>({state:'confirmed' as const,request_id:original.request_id,error:null,response:{...response(),receipt:{...response().receipt,result_ref:{...response().receipt.result_ref,binding:{...binding,content_hash:'d'.repeat(64)}}}}}));
 render(ReviewPanel,{token:'selected',id:'draft-1',client:api,onclose:vi.fn()});await screen.findByText('候选完整正文');
 await fireEvent.input(screen.getByRole('textbox',{name:'审阅意见'}),{target:{value:original.comment}});
 await fireEvent.click(screen.getByRole('button',{name:'批准此版本并出版'}));
 await screen.findByText('审阅回执与原决定的完整 binding、署名或意见不同。');expect(screen.queryByText('人工批准已记录')).toBeNull();
 expect(screen.getByText(original.request_id)).toBeTruthy();expect(api.prepare).toHaveBeenCalledTimes(1);
});
