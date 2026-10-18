// Renderer fixtures only; they do not establish core authorization.
import type {AskAnswerView,PreparedAskAnswer,AskAnswerOutcome} from './ask-answer';
export const askId='H-real';
export const askRequestId='0192f0aa-0000-7000-8000-000000000001';
export function askView():AskAnswerView{
 const recipient={run:'R-real',task:'T-real',host:'host',worktree:'/real/task',branch:'t/T-real',fences:[{name:'module:A',holder:'R-real',epoch:3}]};
 const original={type:'Hold' as const,id:askId,body:{kind:'ask' as const,fingerprint:'fingerprint',question:'应该先做哪一步？',module:'A',options:['先完成原型','先补细节'],recommended:0,context:'完整上下文\n保留原文'},recipients:[recipient],opened_ms:1000,opened_seq:0,reply:null,waits:[],delivered:{},attention:false};
 return {project_id:'project-real',opening:{seq:7,eid:'opening-real',type:'hold.opened',layer:'L5',actor:'process:towermd-cli',host:'host',obj:askId,stale:false,guards:recipient.fences,epoch:3,data:{hold:original}},current_hold:{...original,opened_seq:7},binding:{eid:'opening-real',sha256:'a'.repeat(64)},current_validity:{state:'current'},recipients:[{recipient,actionable:true}],actions:['answer'],answer_ticket:'0192f0aa-0000-7000-8000-000000000002',human_actor:'human:owner',original_requests:[]};
}
export function askOriginal():PreparedAskAnswer{return {request_id:askRequestId,hold:askId,binding:askView().binding,text:'先完成原型',human_actor:'human:owner'};}
export function askConfirmed(historical=false):AskAnswerOutcome{return {request_id:askRequestId,state:'confirmed',error:null,response:{receipt:{request_id:askRequestId,body_hash:'b'.repeat(64),result_ref:{hold:askId,opening_eid:'opening-real',reply:{id:'reply-real',text:'先完成原型',actor:'human:owner',at_ms:1100,relayed_from:null,self_reported:null,decision:null,decision_revision:null}}},current_validity:historical?{state:'historical_only',reason:'hub 已重建'}:{state:'current'}}};}
