import {cleanup,fireEvent,render,screen,waitFor} from '@testing-library/svelte';import {afterEach,expect,test,vi} from 'vitest';
import DraftWorkspace from './DraftWorkspace.svelte';import {savedFixture} from '../lib/draft.fixture';import type {DraftClient,PreparedEdit,SaveReceipt,EditOutcome} from '../lib/draft';
afterEach(cleanup);
const original:PreparedEdit={request_id:'original-request',draft_id:'draft-1',expected_revision:'a'.repeat(40),human_actor:'human:fixture'};
const receipt:SaveReceipt={receipt:{request_id:original.request_id,body_hash:'body',result_ref:{draft_id:'draft-1',revision:'d'.repeat(40),reference:'refs/towermd/drafts/draft-1',content_hash:'content',delivery:{state:'local_saved'}}},current_validity:{state:'current'}};
async function open(){await fireEvent.input(screen.getByRole('textbox',{name:'已保存草稿 ID'}),{target:{value:'draft-1'}});await fireEvent.click(screen.getByRole('button',{name:'打开已保存草稿'}));await screen.findByText('本机身份：human:fixture');}
test('manual saved-draft load, typed text save, exact retry and reload use one original request',async()=>{
 let load=0;let pending:PreparedEdit[]=[];let attempts=0;
 const client:DraftClient={load:vi.fn(async()=>savedFixture(++load===1?'a'.repeat(40):'d'.repeat(40))),pending:vi.fn(async()=>pending),prepare:vi.fn(async()=>{pending=[original];return original;}),submit:vi.fn(async():Promise<EditOutcome>=>++attempts===1?{state:'uncertain',request_id:original.request_id,response:null,error:{code:'writer_busy',message:'writer busy',detail:null}}:(pending=[],{state:'confirmed',request_id:original.request_id,response:receipt,error:null}))};
 render(DraftWorkspace,{token:'selected',client});expect(client.load).not.toHaveBeenCalled();await open();
 await fireEvent.input(screen.getByRole('textbox',{name:'卡片文本'}),{target:{value:'真实便签内容'}});await fireEvent.click(screen.getByRole('button',{name:'保存此编辑到草稿'}));
 await screen.findByText('writer busy');expect(client.prepare).toHaveBeenCalledWith('selected','draft-1','a'.repeat(40),expect.objectContaining({action:'put_annotation',node:expect.objectContaining({content:{type:'text',text:'真实便签内容'}})}));
 expect((screen.getByRole('button',{name:'保存此编辑到草稿'}) as HTMLButtonElement).disabled).toBe(true);
 await waitFor(()=>expect((screen.getByRole('button',{name:'重试原请求'}) as HTMLButtonElement).disabled).toBe(false));
 await fireEvent.click(screen.getByRole('button',{name:'重试原请求'}));await screen.findByText('草稿本地已保存；尚未批准。');
 expect(client.submit).toHaveBeenNthCalledWith(1,'selected',original.request_id);expect(client.submit).toHaveBeenNthCalledWith(2,'selected',original.request_id);expect(client.prepare).toHaveBeenCalledTimes(1);
 await waitFor(()=>expect(client.load).toHaveBeenCalledTimes(2));expect(screen.getByText('d'.repeat(40))).toBeTruthy();
});
test('conflict refreshes actual revision without reapplying edit or claiming save',async()=>{
 let loads=0;const client:DraftClient={load:vi.fn(async()=>savedFixture(++loads===1?'a'.repeat(40):'e'.repeat(40))),pending:vi.fn(async()=>[]),prepare:vi.fn(async()=>original),submit:vi.fn(async():Promise<EditOutcome>=>({state:'conflict',request_id:original.request_id,response:null,error:{code:'request_conflict',message:'revision conflict',detail:null}}))};
 render(DraftWorkspace,{token:'selected',client});await open();await fireEvent.click(screen.getByRole('button',{name:'保存此编辑到草稿'}));await screen.findByText(/草稿发生冲突/);
 expect(client.prepare).toHaveBeenCalledTimes(1);expect(client.submit).toHaveBeenCalledTimes(1);expect(screen.getByText('e'.repeat(40))).toBeTruthy();expect(screen.queryByText('草稿本地已保存；尚未批准。')).toBeNull();
});
test('pending queue recovered on reopen blocks new writes and project destruction discards late load',async()=>{
 const client:DraftClient={load:vi.fn(async()=>savedFixture()),pending:vi.fn(async()=>[original]),prepare:vi.fn(),submit:vi.fn()};
 const view=render(DraftWorkspace,{token:'selected',client});await open();await screen.findByRole('region',{name:'原请求等待确认'});expect((screen.getByRole('button',{name:'保存此编辑到草稿'}) as HTMLButtonElement).disabled).toBe(true);view.unmount();
 let resolve!:(value:ReturnType<typeof savedFixture>)=>void;const onloaded=vi.fn();const delayed={...client,load:vi.fn(()=>new Promise<ReturnType<typeof savedFixture>>(deliver=>resolve=deliver))};
 const old=render(DraftWorkspace,{token:'old',client:delayed,onloaded});await fireEvent.input(screen.getByRole('textbox',{name:'已保存草稿 ID'}),{target:{value:'draft-1'}});await fireEvent.click(screen.getByRole('button',{name:'打开已保存草稿'}));old.unmount();resolve(savedFixture());await Promise.resolve();expect(onloaded).not.toHaveBeenCalled();
});


test('historical saved ref remains readable with its reason and cannot produce an edit',async()=>{
 const value=savedFixture();value.current_validity={state:'historical_only',reason:'当前已发布 B0，原目标失效'};
 const client:DraftClient={load:vi.fn(async()=>value),pending:vi.fn(async()=>[]),prepare:vi.fn(),submit:vi.fn()};
 render(DraftWorkspace,{token:'selected',client});await open();await screen.findByText(/历史草稿 · 只读：当前已发布 B0/);
 expect((screen.getByRole('button',{name:'保存此编辑到草稿'}) as HTMLButtonElement).disabled).toBe(true);expect(client.prepare).not.toHaveBeenCalled();
});

test('创建已确认而实际草稿读取失败时保留ID，只重读同一草稿',async()=>{
 const id='created-draft';const value=savedFixture();value.id=id;value.draft.bundle.id=id;value.draft.bundle.target_baseline='towermd/B1';
 const creationClient:import('../lib/current-draft').CurrentDraftClient={load:vi.fn(async()=>({baseline:'towermd/B0',main_revision:'b'.repeat(40)})),pending:vi.fn(async()=>[]),prepare:vi.fn(async()=>({request_id:id,draft_id:id,baseline:'towermd/B0',main_revision:'b'.repeat(40),human_actor:'human:fixture'})),submit:vi.fn(async():Promise<EditOutcome>=>({state:'confirmed',request_id:id,error:null,response:{...receipt,receipt:{...receipt.receipt,request_id:id,result_ref:{...receipt.receipt.result_ref,draft_id:id,reference:`refs/towermd/drafts/${id}`}}}}))};
 const client:DraftClient={load:vi.fn().mockRejectedValueOnce({message:'草稿读取暂不可用'}).mockResolvedValueOnce(value),pending:vi.fn(async()=>[]),prepare:vi.fn(),submit:vi.fn()};
 render(DraftWorkspace,{token:'selected',client,currentClient:creationClient});await fireEvent.click(screen.getByRole('button',{name:'创建或恢复变更草稿'}));await screen.findByText('b'.repeat(40));await fireEvent.click(screen.getByRole('button',{name:'确认创建变更草稿'}));await screen.findByText('草稿读取暂不可用');
 await waitFor(()=>expect((screen.getByRole('button',{name:'打开已创建草稿'}) as HTMLButtonElement).disabled).toBe(false));expect((screen.getByRole('button',{name:'确认创建变更草稿'}) as HTMLButtonElement).disabled).toBe(true);await fireEvent.click(screen.getByRole('button',{name:'打开已创建草稿'}));await screen.findByText('目标 towermd/B1');expect(client.load).toHaveBeenNthCalledWith(1,'selected',id);expect(client.load).toHaveBeenNthCalledWith(2,'selected',id);expect(creationClient.prepare).toHaveBeenCalledTimes(1);expect(creationClient.submit).toHaveBeenCalledTimes(1);
});
