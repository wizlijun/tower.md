import {invoke} from '@tauri-apps/api/core';
import type {AppError} from './project';

export type TaskReviewVerdict='approved'|'changes_requested'|'rejected';
export type TaskReviewValidity={state:'current'}|{state:'historical_only';reason:string};
export interface TaskReviewFence {name:string;holder:string;epoch:number}
export interface TaskReviewBinding {run:string;task:string;baseline:string;fences:TaskReviewFence[];subject:{kind:'plan';sha256:string}|{kind:'tree';tree:string}}
export interface TaskReviewBody {kind:'review';binding:TaskReviewBinding;verdict?:TaskReviewVerdict|null;plan_content?:string|null;source_commit?:string|null;source_path?:string|null;comment?:string|null}
export interface TaskReviewReply {id:string;text:string;actor:string;at_ms:number;[key:string]:unknown}
export interface TaskReviewHold {type:'Hold';id:string;body:TaskReviewBody;opened_ms:number;opened_seq:number;reply:TaskReviewReply|null;[key:string]:unknown}
export interface TaskReviewOpening {seq:number;eid:string;host:string;actor:string;type:'hold.opened';layer:'L5';obj:string;data:{hold:TaskReviewHold;[key:string]:unknown};guards:TaskReviewFence[];epoch:number|null;stale:boolean;[key:string]:unknown}
export interface PreparedTaskReview {request_id:string;hold:string;binding:TaskReviewBinding;verdict:TaskReviewVerdict;comment:string;human_actor:string}
export interface TaskReviewView {project_id:string;opening:TaskReviewOpening;current_hold:TaskReviewHold;binding:TaskReviewBinding;current_validity:TaskReviewValidity;review_ticket:string;human_actor:string;original_requests:PreparedTaskReview[]}
export interface TaskReviewResponse {receipt:{request_id:string;body_hash:string;result_ref:{hold:string;fact:{binding:TaskReviewBinding;verdict:TaskReviewVerdict;actor:string;answer_eid:string};verdict:TaskReviewVerdict;comment:string}};current_validity:TaskReviewValidity}
export interface TaskReviewOutcome {state:'confirmed'|'uncertain'|'conflict'|'rejected';request_id:string;response:TaskReviewResponse|null;error:AppError|null}
export interface TaskReviewClient {load(token:string,id:string):Promise<TaskReviewView>;prepare(token:string,ticket:string,verdict:TaskReviewVerdict,comment:string):Promise<PreparedTaskReview>;pending(token:string,id:string):Promise<PreparedTaskReview[]>;submit(token:string,id:string):Promise<TaskReviewOutcome>}

const record=(value:unknown):value is Record<string,unknown>=>value!==null&&typeof value==='object'&&!Array.isArray(value);
const token=(value:unknown):value is string=>typeof value==='string'&&value.length>0&&!/[\s\p{Cc}]/u.test(value);
const human=(value:unknown):value is string=>token(value)&&value.startsWith('human:')&&value.slice(6).length>0&&!/[:/]/.test(value.slice(6));
const integer=(value:unknown):value is number=>typeof value==='number'&&Number.isSafeInteger(value)&&value>=0;
const textOrNull=(value:unknown)=>value===null||typeof value==='string';
const optionalText=(value:unknown)=>value==null||typeof value==='string';
const verdict=(value:unknown):value is TaskReviewVerdict=>value==='approved'||value==='changes_requested'||value==='rejected';
const hash=(value:unknown):value is string=>typeof value==='string'&&/^[a-f0-9]{64}$/.test(value);
const oid=(value:unknown):value is string=>typeof value==='string'&&/^([a-f0-9]{40}|[a-f0-9]{64})$/.test(value);
export const validTaskReviewHold=(value:string)=>value.length>0&&value.length<=160&&/^[A-Za-z0-9_.-]+$/.test(value)&&value!=='.'&&value!=='..';
const requestId=(value:unknown):value is string=>typeof value==='string'&&/^[a-f0-9]{8}-[a-f0-9]{4}-7[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/.test(value);
function validFences(value:unknown,run:string):value is TaskReviewFence[] {
 return Array.isArray(value)&&value.length>0&&value.every(f=>record(f)&&token(f.name)&&f.holder===run&&integer(f.epoch)&&f.epoch>0)&&new Set(value.map(f=>f.name)).size===value.length;
}
function validBinding(value:unknown):value is TaskReviewBinding {
 if(!record(value)||!token(value.run)||!token(value.task)||typeof value.baseline!=='string'||!/^towermd\/B(0|[1-9]\d*)$/.test(value.baseline)||!validFences(value.fences,value.run)||!record(value.subject))return false;
 return (value.subject.kind==='plan'&&hash(value.subject.sha256))||(value.subject.kind==='tree'&&oid(value.subject.tree));
}
const sameFences=(a:TaskReviewFence[],b:TaskReviewFence[])=>a.length===b.length&&a.every(f=>b.some(other=>f.name===other.name&&f.holder===other.holder&&f.epoch===other.epoch));
export function sameTaskReviewBinding(a:TaskReviewBinding,b:TaskReviewBinding):boolean {
 return a.run===b.run&&a.task===b.task&&a.baseline===b.baseline&&sameFences(a.fences,b.fences)&&a.subject.kind===b.subject.kind&&(a.subject.kind==='plan'&&b.subject.kind==='plan'?a.subject.sha256===b.subject.sha256:a.subject.kind==='tree'&&b.subject.kind==='tree'&&a.subject.tree===b.subject.tree);
}
function validValidity(value:unknown):value is TaskReviewValidity {
 return record(value)&&(value.state==='current'||(value.state==='historical_only'&&typeof value.reason==='string'&&value.reason.length>0));
}
function validHold(value:unknown,id:string):value is TaskReviewHold {
 if(!record(value)||value.type!=='Hold'||value.id!==id||!integer(value.opened_ms)||!integer(value.opened_seq)||!record(value.body))return false;
 const body=value.body;
 if(body.kind!=='review'||!validBinding(body.binding)||!(body.verdict==null||verdict(body.verdict))||!optionalText(body.comment)||!optionalText(body.source_commit)||!optionalText(body.source_path))return false;
 if(body.binding.subject.kind==='plan'?typeof body.plan_content!=='string':body.plan_content!=null)return false;
 return value.reply===null||(record(value.reply)&&token(value.reply.id)&&typeof value.reply.text==='string'&&human(value.reply.actor)&&integer(value.reply.at_ms));
}

export function decodePreparedTaskReview(value:unknown,hold?:string):PreparedTaskReview {
 if(!record(value)||!requestId(value.request_id)||typeof value.hold!=='string'||!validTaskReviewHold(value.hold)||(hold!==undefined&&value.hold!==hold)||!validBinding(value.binding)||!verdict(value.verdict)||typeof value.comment!=='string'||!human(value.human_actor))throw new Error('原任务评审请求缺少 UUID、人工身份或完整绑定。');
 return value as unknown as PreparedTaskReview;
}
export function decodeTaskReviewPending(value:unknown,hold:string):PreparedTaskReview[] {
 if(!Array.isArray(value))throw new Error('无法核实原任务评审队列；请重新读取。');
 return value.map(item=>decodePreparedTaskReview(item,hold));
}
export function decodeTaskReviewView(value:unknown,id:string):TaskReviewView {
 if(!validTaskReviewHold(id)||!record(value)||!token(value.project_id)||!token(value.review_ticket)||!human(value.human_actor)||!validBinding(value.binding)||!validValidity(value.current_validity)||!record(value.opening)||!validHold(value.current_hold,id))throw new Error('任务评审缺少真实项目、人工身份或当前有效性声明。');
 const opening=value.opening;const current=value.current_hold;
 if(!integer(opening.seq)||opening.seq===0||!token(opening.eid)||!token(opening.host)||opening.type!=='hold.opened'||opening.layer!=='L5'||opening.obj!==id||opening.actor!=='process:towermd-cli'||typeof opening.stale!=='boolean'||(opening.stale&&value.current_validity.state!=='historical_only')||!record(opening.data)||!validHold(opening.data.hold,id)||!validFences(opening.guards,value.binding.run)||!sameFences(opening.guards,value.binding.fences)||opening.epoch!==opening.guards[0].epoch)throw new Error('任务评审缺少实际原开立事件与全部 fences。');
 const original=opening.data.hold;
 if(original.reply!==null||original.body.verdict!=null||original.body.comment!=null||!sameTaskReviewBinding(original.body.binding,value.binding)||!sameTaskReviewBinding(current.body.binding,value.binding)||(value.current_validity.state==='current'&&current.opened_seq!==opening.seq)||current.opened_ms!==original.opened_ms||(current.body.plan_content??null)!==(original.body.plan_content??null)||(current.body.source_commit??null)!==(original.body.source_commit??null)||(current.body.source_path??null)!==(original.body.source_path??null))throw new Error('任务评审原件与当前投影身份不一致。');
 decodeTaskReviewPending(value.original_requests,id);
 return value as unknown as TaskReviewView;
}
export function decodeTaskReviewOutcome(value:unknown,id:string,original?:PreparedTaskReview):TaskReviewOutcome {
 if(!record(value)||value.request_id!==id||!['confirmed','uncertain','conflict','rejected'].includes(String(value.state))||!(value.error===null||(record(value.error)&&typeof value.error.code==='string'&&typeof value.error.message==='string'&&textOrNull(value.error.detail))))throw new Error('任务评审结果缺少原请求身份；请保留原 UUID 重试。');
 if(value.state==='confirmed') {
  const response=value.response;
  if(!record(response)||!validValidity(response.current_validity)||!record(response.receipt)||response.receipt.request_id!==id||!hash(response.receipt.body_hash)||!record(response.receipt.result_ref))throw new Error('任务评审回执不完整；请重试原请求。');
  const result=response.receipt.result_ref;const fact=result.fact;
  if(typeof result.hold!=='string'||!validTaskReviewHold(result.hold)||!verdict(result.verdict)||typeof result.comment!=='string'||!record(fact)||!validBinding(fact.binding)||fact.verdict!==result.verdict||!human(fact.actor)||!token(fact.answer_eid))throw new Error('任务评审回执缺少完整人工事实。');
  if(original&&(original.request_id!==id||original.hold!==result.hold||original.verdict!==result.verdict||original.comment!==result.comment||original.human_actor!==fact.actor||!sameTaskReviewBinding(original.binding,fact.binding)))throw new Error('任务评审回执与已保存原请求不同；不会替换原请求。');
 } else if(value.response!==null)throw new Error('未确认的任务评审不能当作有效回执。');
 return value as unknown as TaskReviewOutcome;
}
export const taskReviewClient:TaskReviewClient={
 load:async(token,id)=>decodeTaskReviewView(await invoke('load_task_review',{token,id}),id),
 prepare:async(token,reviewTicket,verdict,comment)=>decodePreparedTaskReview(await invoke('prepare_task_review',{token,reviewTicket,verdict,comment})),
 pending:async(token,id)=>decodeTaskReviewPending(await invoke('pending_task_reviews',{token,id}),id),
 submit:async(token,requestId)=>decodeTaskReviewOutcome(await invoke('submit_task_review',{token,requestId}),requestId),
};
export const taskReviewVerdictLabel=(value:TaskReviewVerdict)=>({approved:'批准',changes_requested:'需要修改',rejected:'拒绝'}[value]);
export const terminalTaskReview=(view:TaskReviewView)=>view.current_hold.body.verdict!=null||view.current_hold.reply!==null;
