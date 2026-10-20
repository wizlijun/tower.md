import {invoke} from '@tauri-apps/api/core';
import type {AppError} from './project';
export interface RecheckFence {name:string;holder:string;epoch:number}
export interface RecheckBinding {task_id:string;target_baseline:string;task_content_hash:string;publication_ids:string[];running:null|{run:string;fences:RecheckFence[];lease_expires_at:Record<string,number>}}
export interface RecheckFact {task:string;old_baseline:string;new_baseline:string;publication_id:string;reasons:string[]}
export interface TaskRecheckSubject {binding:RecheckBinding;task_text:string;reasons:RecheckFact[]}
export interface PreparedTaskRecheck {request_id:string;binding:RecheckBinding;comment:string;human_actor:string;body_hash:string;subject:TaskRecheckSubject}
export interface TaskRecheckView extends TaskRecheckSubject {project_id:string;human_actor:string;recheck_ticket:string;original_requests:PreparedTaskRecheck[]}
export interface TaskRecheckResponse {receipt:{request_id:string;body_hash:string;result_ref:{request_id:string;binding:RecheckBinding;actor:string;resolved_eid:string}};current_validity:{state:'current'}|{state:'historical_only';reason:string}}
export interface TaskRecheckOutcome {state:'confirmed'|'uncertain'|'conflict'|'rejected';request_id:string;response:TaskRecheckResponse|null;error:AppError|null}
export interface TaskRecheckClient {load(token:string,id:string):Promise<TaskRecheckView>;prepare(token:string,ticket:string,comment:string):Promise<PreparedTaskRecheck>;pending(token:string,id:string):Promise<PreparedTaskRecheck[]>;history(token:string,id:string):Promise<PreparedTaskRecheck[]>;submit(token:string,id:string):Promise<TaskRecheckOutcome>}
const record=(v:unknown):v is Record<string,unknown>=>v!==null&&typeof v==='object'&&!Array.isArray(v);
const exact=(v:Record<string,unknown>,keys:string[])=>Object.keys(v).length===keys.length&&keys.every(k=>Object.hasOwn(v,k));
const text=(v:unknown):v is string=>typeof v==='string'&&v.length>0;
const integer=(v:unknown):v is number=>typeof v==='number'&&Number.isSafeInteger(v)&&v>=0;
const hash=(v:unknown):v is string=>typeof v==='string'&&/^[a-f0-9]{64}$/.test(v);
const uuid=(v:unknown):v is string=>typeof v==='string'&&/^[a-f0-9]{8}-[a-f0-9]{4}-7[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/.test(v);
const human=(v:unknown):v is string=>typeof v==='string'&&/^human:[^\s:/\p{Cc}]+$/u.test(v);
const baseline=(v:unknown):v is string=>typeof v==='string'&&/^towermd\/B(0|[1-9]\d*)$/.test(v);
export const validTaskRecheckId=(v:string)=>v.length>0&&v.length<=160&&/^[A-Za-z0-9_.-]+$/.test(v)&&v!=='.'&&v!=='..';
const sorted=(v:unknown):v is string[]=>Array.isArray(v)&&v.every(text)&&v.every((id,i)=>i===0||v[i-1]<id);
function validBinding(v:unknown):v is RecheckBinding {
 if(!record(v)||!exact(v,['task_id','target_baseline','task_content_hash','publication_ids','running'])||typeof v.task_id!=='string'||!validTaskRecheckId(v.task_id)||!baseline(v.target_baseline)||!hash(v.task_content_hash)||!sorted(v.publication_ids))return false;
 if(v.running===null)return true;
 const r=v.running;
 if(!record(r)||!exact(r,['run','fences','lease_expires_at'])||!text(r.run)||!Array.isArray(r.fences)||!r.fences.length||!record(r.lease_expires_at))return false;
 const expiries=r.lease_expires_at;const fences=r.fences;let previous='';
 for(const f of fences){
  if(!record(f)||!exact(f,['name','holder','epoch'])||!text(f.name)||f.holder!==r.run||!integer(f.epoch)||f.epoch===0||f.name<=previous)return false;
  const expiry=expiries[f.name];if(!integer(expiry)||expiry===0)return false;previous=f.name;
 }
 return Object.keys(expiries).length===fences.length;
}
export function sameTaskRecheckBinding(a:RecheckBinding,b:RecheckBinding):boolean {
 if(a.task_id!==b.task_id||a.target_baseline!==b.target_baseline||a.task_content_hash!==b.task_content_hash||a.publication_ids.length!==b.publication_ids.length||!a.publication_ids.every((id,i)=>id===b.publication_ids[i]))return false;
 if(a.running===null||b.running===null)return a.running===b.running;
 const x=a.running,y=b.running;
 return x.run===y.run&&x.fences.length===y.fences.length&&x.fences.every((f,i)=>f.name===y.fences[i].name&&f.holder===y.fences[i].holder&&f.epoch===y.fences[i].epoch&&x.lease_expires_at[f.name]===y.lease_expires_at[f.name]);
}
export function decodePreparedTaskRecheck(v:unknown,task?:string):PreparedTaskRecheck {
 if(!record(v)||!uuid(v.request_id)||!validBinding(v.binding)||(task!==undefined&&v.binding.task_id!==task)||typeof v.comment!=='string'||!human(v.human_actor)||!hash(v.body_hash))throw new Error('原复核请求缺少 UUID、完整 publication/租约绑定、人工署名或正文 hash。');
 const subject=decodeSubject(v.subject,v.binding.task_id);if(!sameTaskRecheckBinding(v.binding,subject.binding))throw new Error('原请求与保存的完整 Task 原件绑定不同。');
 return v as unknown as PreparedTaskRecheck;
}
export function decodeTaskRecheckPending(v:unknown,task:string):PreparedTaskRecheck[] {
 if(!Array.isArray(v))throw new Error('未核实原复核请求队列；请重新读取。');
 const values=v.map(item=>decodePreparedTaskRecheck(item,task));
 if(new Set(values.map(item=>item.request_id)).size!==values.length)throw new Error('原复核请求队列出现重复 UUID。');
 return values;
}
function decodeSubject(v:unknown,task:string):TaskRecheckSubject {
 if(!record(v)||!validBinding(v.binding)||v.binding.task_id!==task||!text(v.task_text)||!Array.isArray(v.reasons))throw new Error('Task 复核原件缺少当前任务文本或完整绑定。');
 const binding=v.binding;
 if(v.reasons.length!==binding.publication_ids.length||!v.reasons.every((f,i)=>record(f)&&exact(f,['task','old_baseline','new_baseline','publication_id','reasons'])&&f.task===task&&baseline(f.old_baseline)&&baseline(f.new_baseline)&&BigInt(f.old_baseline.slice(9))<BigInt(f.new_baseline.slice(9))&&BigInt(f.new_baseline.slice(9))<=BigInt(binding.target_baseline.slice(9))&&f.publication_id===binding.publication_ids[i]&&Array.isArray(f.reasons)&&f.reasons.length>0&&f.reasons.every(r=>typeof r==='string'&&r.trim().length>0)))throw new Error('Task 复核必须包含完整且排序一致的 publication 原因。');
 return v as unknown as TaskRecheckSubject;
}
export function sameTaskRecheckSubject(a:TaskRecheckSubject,b:TaskRecheckSubject):boolean {
 return sameTaskRecheckBinding(a.binding,b.binding)&&a.task_text===b.task_text&&a.reasons.length===b.reasons.length&&a.reasons.every((f,i)=>{const other=b.reasons[i];return f.task===other.task&&f.old_baseline===other.old_baseline&&f.new_baseline===other.new_baseline&&f.publication_id===other.publication_id&&f.reasons.length===other.reasons.length&&f.reasons.every((r,n)=>r===other.reasons[n]);});
}
export function decodeTaskRecheckView(v:unknown,task:string):TaskRecheckView {
 if(!validTaskRecheckId(task)||!record(v)||!text(v.project_id)||!human(v.human_actor)||!uuid(v.recheck_ticket))throw new Error('Task 复核原件缺少真实项目、人工身份或已读凭据。');
 decodeSubject(v,task);decodeTaskRecheckPending(v.original_requests,task);
 return v as unknown as TaskRecheckView;
}
export function decodeTaskRecheckOutcome(v:unknown,id:string,original?:PreparedTaskRecheck):TaskRecheckOutcome {
 if(!record(v)||v.request_id!==id||!['confirmed','uncertain','conflict','rejected'].includes(String(v.state))||!(v.error===null||(record(v.error)&&typeof v.error.code==='string'&&typeof v.error.message==='string'&&(v.error.detail==null||typeof v.error.detail==='string'))))throw new Error('复核回执缺少原 UUID；保留原请求重试。');
 if(v.state==='confirmed') {
  const response=v.response;
  if(!record(response)||!record(response.current_validity)||!(response.current_validity.state==='current'||(response.current_validity.state==='historical_only'&&text(response.current_validity.reason)))||!record(response.receipt)||response.receipt.request_id!==id||!hash(response.receipt.body_hash)||!record(response.receipt.result_ref))throw new Error('复核回执不完整；保留原请求重试。');
  const result=response.receipt.result_ref;
  if(result.request_id!==id||!validBinding(result.binding)||!human(result.actor)||!text(result.resolved_eid)||(original&&(original.request_id!==id||response.receipt.body_hash!==original.body_hash||result.actor!==original.human_actor||!sameTaskRecheckBinding(result.binding,original.binding))))throw new Error('复核回执与保存的原 UUID、正文、署名或完整绑定不一致。');
 }else if(v.response!==null)throw new Error('未确认的复核不能作为已解除记录。');
 return v as unknown as TaskRecheckOutcome;
}
export const taskRecheckClient:TaskRecheckClient={
 load:async(token,id)=>decodeTaskRecheckView(await invoke('load_task_recheck',{token,id}),id),
 prepare:async(token,recheckTicket,comment)=>decodePreparedTaskRecheck(await invoke('prepare_task_recheck',{token,recheckTicket,comment})),
 pending:async(token,id)=>decodeTaskRecheckPending(await invoke('pending_task_rechecks',{token,id}),id),
 history:async(token,id)=>decodeTaskRecheckPending(await invoke('load_task_recheck_history',{token,id}),id),
 submit:async(token,requestId)=>decodeTaskRecheckOutcome(await invoke('submit_task_recheck',{token,requestId}),requestId),
};
