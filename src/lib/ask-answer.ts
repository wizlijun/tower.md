import { invoke } from '@tauri-apps/api/core';

export type AskFence={name:string;holder:string;epoch:number};
export type AskRecipient={run:string;task:string;host:string;worktree:string;branch:string;fences:AskFence[]};
export type AskBinding={eid:string;sha256:string};
export type AskValidity={state:'current'}|{state:'historical_only';reason:string};
export type AskReply={id:string;text:string;actor:string;at_ms:number;relayed_from?:string|null;self_reported?:unknown;decision?:string|null;decision_revision?:string|null};
export type AskHold={type:'Hold';id:string;body:{kind:'ask';fingerprint:string;question:string;module:string;options:string[];recommended:number|null;context:string;[key:string]:unknown};recipients:AskRecipient[];opened_ms:number;opened_seq:number;reply:AskReply|null;[key:string]:unknown};
export type PreparedAskAnswer={request_id:string;hold:string;binding:AskBinding;text:string;human_actor:string};
export type AskAnswerView={project_id:string;opening:{seq:number;eid:string;type:'hold.opened';layer:'L5';actor:'process:towermd-cli';host:string;obj:string;stale:boolean;guards:AskFence[];epoch:number|null;data:{hold:AskHold};[key:string]:unknown};current_hold:AskHold;binding:AskBinding;current_validity:AskValidity;recipients:{recipient:AskRecipient;actionable:boolean}[];actions:'answer'[];answer_ticket:string;human_actor:string;original_requests:PreparedAskAnswer[]};
export type AskAnswerResponse={receipt:{request_id:string;body_hash:string;result_ref:{hold:string;opening_eid:string;reply:AskReply}};current_validity:AskValidity};
export type AskAnswerOutcome={request_id:string;state:'confirmed'|'uncertain'|'conflict'|'rejected';response:AskAnswerResponse|null;error:{code:string;message:string;detail?:string|null}|null};
export interface AskAnswerClient{
 load(token:string,id:string):Promise<AskAnswerView>;
 prepare(token:string,answerTicket:string,text:string):Promise<PreparedAskAnswer>;
 pending(token:string,id:string):Promise<PreparedAskAnswer[]>;
 submit(token:string,requestId:string):Promise<AskAnswerOutcome>;
}
const record=(x:unknown):x is Record<string,unknown>=>typeof x==='object'&&x!==null&&!Array.isArray(x);
const token=(x:unknown):x is string=>typeof x==='string'&&x.length>0&&!/[\s\x00-\x1f\x7f/\\]/u.test(x);
const human=(x:unknown):x is string=>typeof x==='string'&&/^human:[^:/\s\x00-\x1f\x7f]+$/u.test(x);
const integer=(x:unknown):x is number=>typeof x==='number'&&Number.isSafeInteger(x)&&x>=0;
const uuid=(x:unknown):x is string=>typeof x==='string'&&/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(x);
const hash=(x:unknown):x is string=>typeof x==='string'&&/^[0-9a-f]{64}$/.test(x);
export const validAskHoldId=(x:unknown):x is string=>token(x);
function binding(x:unknown):x is AskBinding{return record(x)&&token(x.eid)&&hash(x.sha256);}
export const sameAskBinding=(a:AskBinding,b:AskBinding)=>a.eid===b.eid&&a.sha256===b.sha256;
function fences(x:unknown,run?:string):x is AskFence[]{return Array.isArray(x)&&x.length>0&&x.every(f=>record(f)&&token(f.name)&&token(f.holder)&&(run===undefined||f.holder===run)&&integer(f.epoch)&&f.epoch>0)&&new Set(x.map(f=>f.name)).size===x.length;}
const sameFences=(a:AskFence[],b:AskFence[])=>a.length===b.length&&a.every((f,i)=>f.name===b[i].name&&f.holder===b[i].holder&&f.epoch===b[i].epoch);
function recipient(x:unknown):x is AskRecipient{return record(x)&&token(x.run)&&token(x.task)&&token(x.host)&&typeof x.worktree==='string'&&x.worktree.length>0&&x.branch===`t/${x.task}`&&fences(x.fences,x.run);}
const sameRecipient=(a:AskRecipient,b:AskRecipient)=>a.run===b.run&&a.task===b.task&&a.host===b.host&&a.worktree===b.worktree&&a.branch===b.branch&&sameFences(a.fences,b.fences);
function validity(x:unknown):x is AskValidity{return record(x)&&(x.state==='current'||(x.state==='historical_only'&&typeof x.reason==='string'&&x.reason.length>0));}
function reply(x:unknown):x is AskReply{return record(x)&&token(x.id)&&typeof x.text==='string'&&token(x.actor)&&integer(x.at_ms);}
function hold(x:unknown,id:string):x is AskHold{
 if(!record(x)||x.type!=='Hold'||x.id!==id||!record(x.body)||x.body.kind!=='ask'||typeof x.body.fingerprint!=='string'||typeof x.body.question!=='string'||!token(x.body.module)||typeof x.body.context!=='string'||!Array.isArray(x.body.options)||!x.body.options.every(y=>typeof y==='string')||!(x.body.recommended===null||(integer(x.body.recommended)&&x.body.recommended<x.body.options.length))||!integer(x.opened_ms)||!integer(x.opened_seq)||!Array.isArray(x.recipients)||!x.recipients.every(recipient)||new Set(x.recipients.map(r=>r.run)).size!==x.recipients.length)return false;
 return x.reply===null||reply(x.reply);
}
export function decodePreparedAskAnswer(x:unknown,id?:string):PreparedAskAnswer{
 if(!record(x)||!uuid(x.request_id)||!validAskHoldId(x.hold)||(id!==undefined&&x.hold!==id)||!binding(x.binding)||typeof x.text!=='string'||!x.text.trim()||!human(x.human_actor))throw new Error('原回答缺少完整请求身份，请保留原 UUID。');
 return x as PreparedAskAnswer;
}
export function decodeAskPending(x:unknown,id:string):PreparedAskAnswer[]{
 if(!Array.isArray(x))throw new Error('无法读取原回答队列，请重试读取。');
 return x.map(row=>decodePreparedAskAnswer(row,id));
}
export function decodeAskAnswerView(x:unknown,id:string):AskAnswerView{
 if(!validAskHoldId(id)||!record(x)||!token(x.project_id)||!uuid(x.answer_ticket)||!human(x.human_actor)||!binding(x.binding)||!validity(x.current_validity)||!record(x.opening)||!hold(x.current_hold,id)||!Array.isArray(x.recipients)||!Array.isArray(x.actions)||!x.actions.every(a=>a==='answer'))throw new Error('问答原件或当前状态不完整。');
 const o=x.opening;const current=x.current_hold;const historical=x.current_validity.state==='historical_only';
 if(o.type!=='hold.opened'||o.layer!=='L5'||o.actor!=='process:towermd-cli'||o.obj!==id||!token(o.eid)||o.eid!==x.binding.eid||!integer(o.seq)||o.seq===0||typeof o.stale!=='boolean'||(o.stale&&!historical)||!record(o.data)||!hold(o.data.hold,id)||!fences(o.guards)||o.epoch!==o.guards[0].epoch)throw new Error('问答缺少真实开立事件与完整绑定。');
 const original=o.data.hold;const a=original.body;const b=current.body;
 if(original.reply!==null||original.recipients.length!==1||!sameFences(o.guards,original.recipients[0].fences)||a.question!==b.question||a.module!==b.module||a.fingerprint!==b.fingerprint||a.context!==b.context||a.recommended!==b.recommended||JSON.stringify(a.options)!==JSON.stringify(b.options)||current.opened_ms!==original.opened_ms||(!historical&&current.opened_seq!==o.seq)||x.recipients.length!==current.recipients.length||!x.recipients.every((r,i)=>record(r)&&recipient(r.recipient)&&typeof r.actionable==='boolean'&&sameRecipient(r.recipient,current.recipients[i]))||(historical&&(x.actions.length>0||x.recipients.some(r=>r.actionable)))||(current.reply!==null&&x.actions.length>0))throw new Error('问答原件、收件人或历史资格不一致。');
 decodeAskPending(x.original_requests,id);
 return x as unknown as AskAnswerView;
}
export function decodeAskAnswerOutcome(x:unknown,id:string,original?:PreparedAskAnswer):AskAnswerOutcome{
 if(!record(x)||x.request_id!==id||!['confirmed','uncertain','conflict','rejected'].includes(String(x.state))||!(x.error===null||(record(x.error)&&typeof x.error.code==='string'&&typeof x.error.message==='string')))throw new Error('回答结果缺少原请求身份，请重试原请求。');
 if(x.state==='confirmed'){
  const response=x.response;
  if(!record(response)||!validity(response.current_validity)||!record(response.receipt)||response.receipt.request_id!==id||!hash(response.receipt.body_hash)||!record(response.receipt.result_ref))throw new Error('回答回执不完整，请重试原请求。');
  const result=response.receipt.result_ref;const r=result.reply;
  if(!validAskHoldId(result.hold)||!token(result.opening_eid)||!reply(r)||!human(r.actor)||r.relayed_from!=null||r.self_reported!=null||r.decision!=null||r.decision_revision!=null)throw new Error('回答回执缺少原本机人工答复。');
  if(original&&(original.request_id!==id||original.hold!==result.hold||original.binding.eid!==result.opening_eid||original.text!==r.text||original.human_actor!==r.actor))throw new Error('回执与已保存原回答不同，不会替换原件。');
 }else if(x.response!==null)throw new Error('未确认回答不能作为有效回执。');
 return x as unknown as AskAnswerOutcome;
}
export const askAnswerClient:AskAnswerClient={
 load:async(token,id)=>decodeAskAnswerView(await invoke('load_ask_answer',{token,id}),id),
 prepare:async(token,answerTicket,text)=>decodePreparedAskAnswer(await invoke('prepare_ask_answer',{token,answerTicket,text})),
 pending:async(token,id)=>decodeAskPending(await invoke('pending_ask_answers',{token,id}),id),
 submit:async(token,requestId)=>decodeAskAnswerOutcome(await invoke('submit_ask_answer',{token,requestId}),requestId),
};
