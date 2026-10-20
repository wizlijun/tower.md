import {expect,test,vi} from 'vitest';
import {decodePreparedTaskRecheck,decodeTaskRecheckView,decodeTaskRecheckOutcome,sameTaskRecheckBinding,taskRecheckClient} from './task-recheck';
import {preparedRecheck,recheckView,recheckOutcome} from './task-recheck.test-fixtures';
vi.mock('@tauri-apps/api/core',()=>({invoke:vi.fn()}));
import {invoke} from '@tauri-apps/api/core';
test('完整 publication、每条原因和每项 lease expiry 都是绑定，不能省略或扩大',()=>{
 const view=recheckView();expect(decodeTaskRecheckView(view,'T')).toEqual(view);
 for(const change of [(v:TaskRecheckView)=>v.binding.publication_ids.pop(),(v:TaskRecheckView)=>v.reasons.pop(),(v:TaskRecheckView)=>delete v.binding.running!.lease_expires_at['module:B'],(v:TaskRecheckView)=>v.binding.running!.fences.reverse(),(v:TaskRecheckView)=>v.reasons[1].task='another']){const wrong=structuredClone(view);change(wrong);expect(()=>decodeTaskRecheckView(wrong,'T')).toThrow();}
 const unknown=structuredClone(view);Object.assign(unknown.binding,{skip:true});expect(()=>decodeTaskRecheckView(unknown,'T')).toThrow();
});
import type {TaskRecheckView} from './task-recheck';
test('运行、文本 hash、基线、publication 和逐项 expiry 改变都不等价',()=>{
 const original=recheckView().binding;
 for(const change of [(v:typeof original)=>v.task_content_hash='c'.repeat(64),(v:typeof original)=>v.target_baseline='towermd/B3',(v:typeof original)=>v.publication_ids.push('publication-3'),(v:typeof original)=>v.running!.run='run-2',(v:typeof original)=>v.running!.fences[1].epoch++,(v:typeof original)=>v.running!.lease_expires_at['module:B']++]){const changed=structuredClone(original);change(changed);expect(sameTaskRecheckBinding(original,changed)).toBe(false);}
});
test('保存的原件必须与请求 binding 一致；lease expiry 不能从恢复队列消失',()=>{
 const original=preparedRecheck();expect(decodePreparedTaskRecheck(original,'T')).toEqual(original);const wrong=structuredClone(original);wrong.subject.binding.running!.lease_expires_at['module:B']++;expect(()=>decodePreparedTaskRecheck(wrong,'T')).toThrow();
});
test('只接受原 UUID/hash/人工/全量 binding 的 confirmed，历史效力保持历史',()=>{
 const original=preparedRecheck();const outcome=recheckOutcome(original);expect(decodeTaskRecheckOutcome(outcome,original.request_id,original)).toEqual(outcome);
 for(const change of [(v:typeof outcome)=>v.response!.receipt.body_hash='c'.repeat(64),(v:typeof outcome)=>v.response!.receipt.result_ref.actor='human:other',(v:typeof outcome)=>v.response!.receipt.result_ref.binding.running!.lease_expires_at['module:B']++,(v:typeof outcome)=>v.response!.receipt.result_ref.request_id='another']){const wrong=structuredClone(outcome);change(wrong);expect(()=>decodeTaskRecheckOutcome(wrong,original.request_id,original)).toThrow();}
 outcome.response!.current_validity={state:'historical_only',reason:'publication 后续又有变化'};expect(decodeTaskRecheckOutcome(outcome,original.request_id,original).response!.current_validity.state).toBe('historical_only');
});
test('默认真实 native bridge 只发固定 command，不携带 actor 或路径',async()=>{
 vi.mocked(invoke).mockReset();const view=recheckView(),original=preparedRecheck(view),outcome=recheckOutcome(original);vi.mocked(invoke).mockResolvedValueOnce(view).mockResolvedValueOnce(original).mockResolvedValueOnce([original]).mockResolvedValueOnce([original]).mockResolvedValueOnce(outcome);
 await taskRecheckClient.load('selected','T');await taskRecheckClient.prepare('selected',view.recheck_ticket,original.comment);await taskRecheckClient.pending('selected','T');await taskRecheckClient.history('selected','T');await taskRecheckClient.submit('selected',original.request_id);
 expect(vi.mocked(invoke).mock.calls).toEqual([['load_task_recheck',{token:'selected',id:'T'}],['prepare_task_recheck',{token:'selected',recheckTicket:view.recheck_ticket,comment:original.comment}],['pending_task_rechecks',{token:'selected',id:'T'}],['load_task_recheck_history',{token:'selected',id:'T'}],['submit_task_recheck',{token:'selected',requestId:original.request_id}]]);
});

test('历史 native bridge 同样校验完整原件、Task 归属和重复 UUID',async()=>{
 const original=preparedRecheck();const wrong=structuredClone(original);wrong.subject.binding.running!.lease_expires_at['module:B']++;
 for(const values of [[wrong],[original,original]]){vi.mocked(invoke).mockResolvedValueOnce(values);await expect(taskRecheckClient.history('selected','T')).rejects.toThrow();}
 vi.mocked(invoke).mockResolvedValueOnce([original]);await expect(taskRecheckClient.history('selected','U')).rejects.toThrow();
});
