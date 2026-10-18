import {expect,test,vi} from 'vitest';
vi.mock('@tauri-apps/api/core',()=>({invoke:vi.fn()}));
import {invoke} from '@tauri-apps/api/core';
import {decodeTaskReviewView,decodePreparedTaskReview,decodeTaskReviewOutcome,taskReviewClient,validTaskReviewHold,type TaskReviewView} from './task-review';
const requestId='0192f0aa-0000-7000-8000-000000000001';
const binding={run:'R-real',task:'T-real',baseline:'towermd/B0',fences:[{name:'module:A',holder:'R-real',epoch:7},{name:'module:B',holder:'R-real',epoch:9}],subject:{kind:'plan' as const,sha256:'a'.repeat(64)}};
const body={kind:'review' as const,binding,verdict:null,plan_content:'完整计划\r\n<script>只显示原文</script>\n',source_commit:'b'.repeat(40),source_path:'outside/plan.md',comment:null};
const original={type:'Hold' as const,id:'H-real',body,opened_ms:1000,opened_seq:0,reply:null,future:{keep:[2,1]}};
function fixture():TaskReviewView{return {project_id:'actual-project',opening:{seq:10,eid:'opening-real',host:'host',actor:'process:towermd-cli',type:'hold.opened',layer:'L5',obj:'H-real',data:{hold:original},guards:binding.fences,epoch:7,stale:false},current_hold:{...original,opened_seq:10},binding,current_validity:{state:'current'},review_ticket:'local-ticket',human_actor:'human:fixture',original_requests:[]};}
const prepared={request_id:requestId,hold:'H-real',binding,verdict:'approved' as const,comment:'已检查',human_actor:'human:fixture'};
const response={receipt:{request_id:requestId,body_hash:'c'.repeat(64),result_ref:{hold:'H-real',fact:{binding,verdict:'approved' as const,actor:'human:fixture',answer_eid:'answer-real'},verdict:'approved' as const,comment:'已检查'}},current_validity:{state:'current' as const}};

test('只调用四个固定native命令，前端不传actor、目录、binding或生成UUID',async()=>{
 vi.mocked(invoke).mockReset().mockResolvedValueOnce(fixture()).mockResolvedValueOnce(prepared).mockResolvedValueOnce([prepared]).mockResolvedValueOnce({state:'confirmed',request_id:requestId,response,error:null});
 await taskReviewClient.load('selected','H-real');await taskReviewClient.prepare('selected','local-ticket','approved','已检查');await taskReviewClient.pending('selected','H-real');await taskReviewClient.submit('selected',requestId);
 expect(vi.mocked(invoke).mock.calls).toEqual([['load_task_review',{token:'selected',id:'H-real'}],['prepare_task_review',{token:'selected',reviewTicket:'local-ticket',verdict:'approved',comment:'已检查'}],['pending_task_reviews',{token:'selected',id:'H-real'}],['submit_task_review',{token:'selected',requestId}]]);
});
test('完整原件、全部fences和unknown字段只读保留，不接受缺绑定或伪human',()=>{
 const view=fixture();const before=JSON.stringify(view);expect(decodeTaskReviewView(view,'H-real')).toBe(view);expect(JSON.stringify(view)).toBe(before);
 for(const id of ['', '..','../H-real','H-real\n','a'.repeat(161)])expect(validTaskReviewHold(id)).toBe(false);
 const bad=[null,{...view,human_actor:'process:cli'},{...view,human_actor:'human:with space'},{...view,current_validity:{state:'maybe'}},{...view,current_validity:{state:'historical_only'}},{...view,opening:{...view.opening,obj:'H-other'}},{...view,opening:{...view.opening,stale:true}},{...view,opening:{...view.opening,guards:binding.fences.slice(0,1)}},{...view,current_hold:{...view.current_hold,opened_seq:0}},{...view,binding:{...binding,fences:[binding.fences[0],binding.fences[0]]}}];
 for(const value of bad)expect(()=>decodeTaskReviewView(value,'H-real')).toThrow();
 expect(()=>decodeTaskReviewView(view,'H-other')).toThrow();
 const historical={...view,opening:{...view.opening,stale:true},current_hold:{...view.current_hold,opened_seq:0},current_validity:{state:'historical_only',reason:'真实外层重放已stale'}};
 expect(decodeTaskReviewView(historical,'H-real')).toBe(historical);
 const legacy=structuredClone(view);delete legacy.opening.data.hold.body.source_commit;delete legacy.opening.data.hold.body.source_path;delete legacy.opening.data.hold.body.comment;delete legacy.opening.data.hold.body.verdict;
 const raw=JSON.stringify(legacy);expect(decodeTaskReviewView(legacy,'H-real')).toBe(legacy);expect(JSON.stringify(legacy)).toBe(raw);
});
test('原准备请求和回执须保原UUID及精确结论，历史回执不能被推断为当前',()=>{
 expect(decodePreparedTaskReview(prepared,'H-real')).toBe(prepared);
 expect(()=>decodePreparedTaskReview({...prepared,request_id:'frontend-generated'},'H-real')).toThrow();
 expect(()=>decodeTaskReviewOutcome({state:'confirmed',request_id:requestId,response:{...response,receipt:{...response.receipt,request_id:'other'}},error:null},requestId,prepared)).toThrow();
 expect(()=>decodeTaskReviewOutcome({state:'confirmed',request_id:requestId,response:{...response,receipt:{...response.receipt,result_ref:{...response.receipt.result_ref,comment:'替换意见'}}},error:null},requestId,prepared)).toThrow();
 const value={state:'confirmed',request_id:requestId,response:{...response,current_validity:{state:'historical_only',reason:'原epoch已过期'}},error:null};
 expect(decodeTaskReviewOutcome(value,requestId,prepared).response?.current_validity).toEqual({state:'historical_only',reason:'原epoch已过期'});
});
