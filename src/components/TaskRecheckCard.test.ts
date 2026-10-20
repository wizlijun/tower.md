import {act,cleanup,fireEvent,render,screen,waitFor} from '@testing-library/svelte';
import {afterEach,expect,test,vi} from 'vitest';
import TaskRecheckCard from './TaskRecheckCard.svelte';
import {preparedRecheck,recheckView,recheckOutcome} from '../lib/task-recheck.test-fixtures';
import type {TaskRecheckClient,TaskRecheckView,PreparedTaskRecheck} from '../lib/task-recheck';
afterEach(cleanup);
function fixture(){const view=recheckView(),original=preparedRecheck(view);const client:TaskRecheckClient={load:vi.fn(async()=>view),pending:vi.fn(async()=>[]),history:vi.fn(async()=>[]),prepare:vi.fn(async()=>original),submit:vi.fn(async()=>recheckOutcome(original))};return {view,original,client};}
test('只读原件完整展示后明确点击才保存/提交，人意见和原 UUID 不替换',async()=>{
 const {client,original}=fixture();const onresolved=vi.fn();render(TaskRecheckCard,{token:'selected',taskId:'T',client,onresolved});
 await screen.findByRole('button',{name:'确认已复核并解除本次标记'});expect(client.prepare).not.toHaveBeenCalled();expect(client.submit).not.toHaveBeenCalled();
 expect(screen.getByLabelText('当前 Task 完整文本').textContent).toContain('<script>doNotExecute()</script>');expect(document.querySelector('.task-review-card script')).toBeNull();
 expect(screen.getByText('修改了接口契约')).toBeTruthy();expect(screen.getByText('新增了验收约束')).toBeTruthy();expect(screen.getByText('1800000200000')).toBeTruthy();
 await fireEvent.input(screen.getByLabelText('本次复核意见'),{target:{value:original.comment}});await fireEvent.click(screen.getByRole('button',{name:'确认已复核并解除本次标记'}));
 await screen.findByText('本次复核已记录 · 当前绑定有效');expect(client.prepare).toHaveBeenCalledWith('selected',recheckView().recheck_ticket,original.comment);expect(client.submit).toHaveBeenCalledWith('selected',original.request_id);expect(onresolved).toHaveBeenCalledTimes(1);
});
test('不确定后只重试保存的原 UUID，历史回执不会声称当前已解除',async()=>{
 const {client,original}=fixture();const historical=recheckOutcome(original);historical.response!.current_validity={state:'historical_only',reason:'有后续 publication'};
 vi.mocked(client.submit).mockResolvedValueOnce({state:'uncertain',request_id:original.request_id,response:null,error:{code:'request_uncertain',message:'断线',detail:null}}).mockResolvedValueOnce(historical);
 render(TaskRecheckCard,{token:'selected',taskId:'T',client});await screen.findByRole('button',{name:'确认已复核并解除本次标记'});await fireEvent.input(screen.getByLabelText('本次复核意见'),{target:{value:original.comment}});await fireEvent.click(screen.getByRole('button',{name:'确认已复核并解除本次标记'}));
 const retry=await screen.findByRole('button',{name:'重试原复核请求'});await waitFor(()=>expect((retry as HTMLButtonElement).disabled).toBe(false));await fireEvent.click(retry);await screen.findByText('原复核回执已确认 · 仅历史效力');expect(client.prepare).toHaveBeenCalledTimes(1);expect(vi.mocked(client.submit).mock.calls).toEqual([['selected',original.request_id],['selected',original.request_id]]);
});
test('prepare 返回不同原件不会提交，重新读取也不能绕过同实例的阻止',async()=>{
 const {client,original}=fixture();const mismatch=structuredClone(original);mismatch.subject.task_text+='changed';vi.mocked(client.prepare).mockResolvedValue(mismatch);vi.mocked(client.pending).mockResolvedValueOnce([]).mockResolvedValue([mismatch]);
 render(TaskRecheckCard,{token:'selected',taskId:'T',client});await screen.findByRole('button',{name:'确认已复核并解除本次标记'});await fireEvent.input(screen.getByLabelText('本次复核意见'),{target:{value:original.comment}});await fireEvent.click(screen.getByRole('button',{name:'确认已复核并解除本次标记'}));
 await screen.findByText('此请求与已显示原件不同，已阻止提交。');expect(client.submit).not.toHaveBeenCalled();await fireEvent.click(screen.getByRole('button',{name:'重新读取复核原件'}));await waitFor(()=>expect(client.load).toHaveBeenCalledTimes(2));expect((screen.getByRole('button',{name:'重试原复核请求'}) as HTMLButtonElement).disabled).toBe(true);expect(client.submit).not.toHaveBeenCalled();
});

test('历史读取切项目拒绝旧项目迟到原件，只对新项目完整原件显式读回执',async()=>{
 const {client,original}=fixture();const newer=structuredClone(original);newer.request_id='019a0010-0000-7000-8000-000000000003';newer.subject.task_text='新项目保存的完整原件';newer.body_hash='c'.repeat(64);
 let deliver!:(values:PreparedTaskRecheck[])=>void;vi.mocked(client.history).mockImplementation(async token=>token==='old'?new Promise<PreparedTaskRecheck[]>(resolve=>{deliver=resolve;}):[newer]);vi.mocked(client.submit).mockResolvedValue(recheckOutcome(newer));
 const panel=render(TaskRecheckCard,{token:'old',taskId:'T',client,historyOnly:true});await waitFor(()=>expect(client.history).toHaveBeenCalledWith('old','T'));
 await panel.rerender({token:'new',taskId:'T',client,historyOnly:true});const summary=await screen.findByText('本机原复核请求（1）');await fireEvent.click(summary);await act(()=>deliver([original]));
 expect(JSON.parse(summary.closest('details')!.querySelector('pre')!.textContent!)).toEqual(newer);expect(screen.queryByRole('button',{name:'确认已复核并解除本次标记'})).toBeNull();expect(client.submit).not.toHaveBeenCalled();
 await fireEvent.click(screen.getByRole('button',{name:'读取原复核回执'}));await screen.findByRole('region',{name:'复核原回执'});expect(client.submit).toHaveBeenCalledExactlyOnceWith('new',newer.request_id);expect(client.load).not.toHaveBeenCalled();expect(client.prepare).not.toHaveBeenCalled();
});
test('切换为历史模式后拒绝迟到 current GET，不恢复旧 ticket 或新解除入口',async()=>{
 const {client,view,original}=fixture();let deliver!:(value:TaskRecheckView)=>void;vi.mocked(client.load).mockImplementation(()=>new Promise<TaskRecheckView>(resolve=>{deliver=resolve;}));vi.mocked(client.history).mockResolvedValue([original]);
 const panel=render(TaskRecheckCard,{token:'selected',taskId:'T',client});await waitFor(()=>expect(client.load).toHaveBeenCalledTimes(1));
 await panel.rerender({token:'selected',taskId:'T',client,historyOnly:true});const summary=await screen.findByText('本机原复核请求（1）');await act(()=>deliver(view));
 expect(summary).toBeTruthy();expect(screen.queryByLabelText('当前 Task 完整文本')).toBeNull();expect(screen.queryByRole('textbox',{name:'本次复核意见'})).toBeNull();expect(screen.queryByRole('button',{name:'确认已复核并解除本次标记'})).toBeNull();expect(client.prepare).not.toHaveBeenCalled();expect(client.submit).not.toHaveBeenCalled();
});
