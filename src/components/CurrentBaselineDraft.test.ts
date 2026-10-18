import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, expect, test, vi } from 'vitest';
import CurrentBaselineDraft from './CurrentBaselineDraft.svelte';
import type { CurrentDraftClient, PreparedCreation } from '../lib/current-draft';
import type { EditOutcome } from '../lib/draft';
afterEach(cleanup);
const subject={baseline:'towermd/B0',main_revision:'b'.repeat(40)};
const original:PreparedCreation={request_id:'original-uuid',draft_id:'original-uuid',...subject,human_actor:'human:fixture'};
const confirmed:EditOutcome={state:'confirmed',request_id:original.request_id,error:null,response:{receipt:{request_id:original.request_id,body_hash:'body',result_ref:{draft_id:original.draft_id,revision:'c'.repeat(40),reference:'refs/towermd/drafts/original-uuid',content_hash:'hash',delivery:{state:'local_saved'}}},current_validity:{state:'current'}}};
function fixture():CurrentDraftClient{return {load:vi.fn(async()=>subject),pending:vi.fn(async()=>[]),prepare:vi.fn(async()=>original),submit:vi.fn(async()=>confirmed)};}
async function open(){await fireEvent.click(screen.getByRole('button',{name:'创建或恢复变更草稿'}));}
test('读取绑定先于明确创建；默认查看不调用 daemon，成功只使用耐久回执 ID',async()=>{
 const client=fixture();const oncreated=vi.fn(async()=>{});render(CurrentBaselineDraft,{token:'selected',client,oncreated});
 expect(client.load).not.toHaveBeenCalled();expect(client.pending).not.toHaveBeenCalled();await open();await screen.findByText(subject.main_revision);
 expect(client.prepare).not.toHaveBeenCalled();expect(screen.getByText(subject.baseline)).toBeTruthy();
 await fireEvent.click(screen.getByRole('button',{name:'确认创建变更草稿'}));await waitFor(()=>expect(oncreated).toHaveBeenCalledWith(original.draft_id));
 expect(client.prepare).toHaveBeenCalledWith('selected',subject.baseline,subject.main_revision);expect(client.submit).toHaveBeenCalledWith('selected',original.request_id);
});
test('不确定状态保留原 UUID，禁用新创建并显式重试同一请求',async()=>{
 const client=fixture();let attempts=0;client.submit=vi.fn(async():Promise<EditOutcome>=>++attempts===1?{state:'uncertain',request_id:original.request_id,response:null,error:{code:'request_uncertain',message:'原状态未知',detail:null}}:confirmed);
 const oncreated=vi.fn(async()=>{});render(CurrentBaselineDraft,{token:'selected',client,oncreated});await open();await screen.findByText(subject.main_revision);
 await fireEvent.click(screen.getByRole('button',{name:'确认创建变更草稿'}));await screen.findByText('原状态未知');
 expect((screen.getByRole('button',{name:'确认创建变更草稿'}) as HTMLButtonElement).disabled).toBe(true);
 await waitFor(()=>expect((screen.getByRole('button',{name:'重试原创建请求'}) as HTMLButtonElement).disabled).toBe(false));await fireEvent.click(screen.getByRole('button',{name:'重试原创建请求'}));await waitFor(()=>expect(oncreated).toHaveBeenCalledTimes(1));expect(client.prepare).toHaveBeenCalledTimes(1);expect(client.submit).toHaveBeenNthCalledWith(2,'selected',original.request_id);
});
test('重启恢复队列在当前基线读取失败时仍可重试；不替换旧来源',async()=>{
 const client=fixture();client.pending=vi.fn(async()=>[original]);client.load=vi.fn(async()=>{throw {message:'当前来源暂不可用'};});const oncreated=vi.fn(async()=>{});
 render(CurrentBaselineDraft,{token:'selected',client,oncreated});await open();await screen.findByText('当前来源暂不可用');expect(screen.getByText(original.request_id)).toBeTruthy();
 await fireEvent.click(screen.getByRole('button',{name:'重试原创建请求'}));await waitFor(()=>expect(oncreated).toHaveBeenCalledWith(original.draft_id));expect(client.prepare).not.toHaveBeenCalled();
});
test('无认证基线不给创建按钮；切项目销毁后丢弃迟到读取',async()=>{
 const client=fixture();client.load=vi.fn(async()=>null);const view=render(CurrentBaselineDraft,{token:'selected',client,oncreated:vi.fn()});await open();await screen.findByText(/尚无已认证基线/);expect(screen.queryByRole('button',{name:'确认创建变更草稿'})).toBeNull();view.unmount();
 let deliver!:(value:typeof subject)=>void;client.load=vi.fn(()=>new Promise<typeof subject>(resolve=>{deliver=resolve;}));const oncreated=vi.fn();const old=render(CurrentBaselineDraft,{token:'old',client,oncreated});await open();await waitFor(()=>expect(client.load).toHaveBeenCalledWith('old'));old.unmount();deliver(subject);await Promise.resolve();expect(oncreated).not.toHaveBeenCalled();expect(screen.queryByText(subject.main_revision)).toBeNull();
});
test('冲突后不自动准备新请求或打开草稿',async()=>{
 const client=fixture();client.submit=vi.fn(async():Promise<EditOutcome>=>({state:'conflict',request_id:original.request_id,response:null,error:{code:'request_conflict',message:'main 已变化',detail:null}}));const oncreated=vi.fn();render(CurrentBaselineDraft,{token:'selected',client,oncreated});await open();await screen.findByText(subject.main_revision);await fireEvent.click(screen.getByRole('button',{name:'确认创建变更草稿'}));await screen.findByText('main 已变化');expect(client.prepare).toHaveBeenCalledTimes(1);await waitFor(()=>expect(screen.queryByRole('button',{name:'确认创建变更草稿'})).toBeNull());expect(oncreated).not.toHaveBeenCalled();
});


test('确认响应的草稿身份不匹配原请求时保持原请求，不打开错误草稿',async()=>{
 const client=fixture();client.submit=vi.fn(async():Promise<EditOutcome>=>({...confirmed,response:{...confirmed.response!,receipt:{...confirmed.response!.receipt,result_ref:{...confirmed.response!.receipt.result_ref,draft_id:'wrong-draft'}}}}));const oncreated=vi.fn();render(CurrentBaselineDraft,{token:'selected',client,oncreated});await open();await screen.findByText(subject.main_revision);await fireEvent.click(screen.getByRole('button',{name:'确认创建变更草稿'}));await screen.findByText('保存回执与原创建请求不一致；请重试原请求。');expect(oncreated).not.toHaveBeenCalled();expect(screen.getByRole('button',{name:'重试原创建请求'})).toBeTruthy();
});

test('准备可能已写盘后返回错误，必须重读队列且读取失败持续阻止新 UUID',async()=>{
 const client=fixture();let pendingReads=0;client.pending=vi.fn(async()=>{pendingReads++;if(pendingReads===2)throw {message:'本机队列读取失败'};return pendingReads===1?[]:[original];});client.prepare=vi.fn(async()=>{throw {code:'request_queue_failed',message:'fsync 结果未知'};});
 render(CurrentBaselineDraft,{token:'selected',client,oncreated:vi.fn()});await open();await screen.findByText(subject.main_revision);await fireEvent.click(screen.getByRole('button',{name:'确认创建变更草稿'}));await screen.findByText('fsync 结果未知');
 await waitFor(()=>expect(screen.queryByRole('button',{name:'确认创建变更草稿'})).toBeNull());expect(client.prepare).toHaveBeenCalledTimes(1);
 await fireEvent.click(screen.getByRole('button',{name:'重新读取当前基线与原请求'}));await screen.findByText('本机队列读取失败');expect(screen.queryByRole('button',{name:'确认创建变更草稿'})).toBeNull();
 await waitFor(()=>expect((screen.getByRole('button',{name:'重新读取当前基线与原请求'}) as HTMLButtonElement).disabled).toBe(false));await fireEvent.click(screen.getByRole('button',{name:'重新读取当前基线与原请求'}));await screen.findByText(original.request_id);await waitFor(()=>expect(screen.getAllByText(subject.main_revision)).toHaveLength(2));
 expect((screen.getByRole('button',{name:'确认创建变更草稿'}) as HTMLButtonElement).disabled).toBe(true);expect(client.prepare).toHaveBeenCalledTimes(1);expect(client.submit).not.toHaveBeenCalled();
});
test('已确认后首次打开失败只重读已创建 ID，不再 prepare 或 submit',async()=>{
 const client=fixture();const oncreated=vi.fn().mockRejectedValueOnce({message:'已保存草稿暂不可读'}).mockResolvedValueOnce(undefined);
 render(CurrentBaselineDraft,{token:'selected',client,oncreated});await open();await screen.findByText(subject.main_revision);await fireEvent.click(screen.getByRole('button',{name:'确认创建变更草稿'}));await screen.findByText('已保存草稿暂不可读');await screen.findByRole('region',{name:'已创建草稿'});
 expect(client.prepare).toHaveBeenCalledTimes(1);expect(client.submit).toHaveBeenCalledTimes(1);
 await waitFor(()=>expect((screen.getByRole('button',{name:'打开已创建草稿'}) as HTMLButtonElement).disabled).toBe(false));await fireEvent.click(screen.getByRole('button',{name:'打开已创建草稿'}));await waitFor(()=>expect(oncreated).toHaveBeenCalledTimes(2));expect(oncreated.mock.calls).toEqual([[original.draft_id],[original.draft_id]]);expect(client.prepare).toHaveBeenCalledTimes(1);expect(client.submit).toHaveBeenCalledTimes(1);
});
