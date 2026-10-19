import type {TaskRecheckView,PreparedTaskRecheck,TaskRecheckOutcome} from './task-recheck';
export const recheckId='019a0010-0000-7000-8000-000000000001';
export function recheckView():TaskRecheckView {return {
 project_id:'project',human_actor:'human:owner',recheck_ticket:'019a0010-0000-7000-8000-000000000002',original_requests:[],
 binding:{task_id:'T',target_baseline:'towermd/B2',task_content_hash:'a'.repeat(64),publication_ids:['publication-1','publication-2'],running:{run:'run-1',fences:[{name:'module:A',holder:'run-1',epoch:1},{name:'module:B',holder:'run-1',epoch:2}],lease_expires_at:{'module:A':1800000100000,'module:B':1800000200000}}},
 task_text:'type: Task\nid: T\ntitle: <script>doNotExecute()</script>\nstate: running\n',
 reasons:[{task:'T',old_baseline:'towermd/B0',new_baseline:'towermd/B1',publication_id:'publication-1',reasons:['修改了接口契约']},{task:'T',old_baseline:'towermd/B1',new_baseline:'towermd/B2',publication_id:'publication-2',reasons:['新增了验收约束']}],
};}
export function preparedRecheck(view=recheckView()):PreparedTaskRecheck{return {request_id:recheckId,binding:structuredClone(view.binding),comment:'已核对全部原因',human_actor:view.human_actor,body_hash:'b'.repeat(64),subject:{binding:structuredClone(view.binding),task_text:view.task_text,reasons:structuredClone(view.reasons)}};}
export function recheckOutcome(original=preparedRecheck()):TaskRecheckOutcome{return {state:'confirmed',request_id:original.request_id,error:null,response:{receipt:{request_id:original.request_id,body_hash:original.body_hash,result_ref:{request_id:original.request_id,binding:structuredClone(original.binding),actor:original.human_actor,resolved_eid:'actual-native-verified-event'}},current_validity:{state:'current'}}};}
