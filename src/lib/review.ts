import { invoke } from '@tauri-apps/api/core';
import type {SavedDraft} from './draft';
import type {CanvasSource} from './canvas-flow';
export type CanvasComparison = {state:'available';main_revision:string;baseline:string|null;baseline_canvas:CanvasSource|null;draft_canvas:CanvasSource;diagnostics:string[]}|{state:'unavailable';reason:string};
export interface ReviewBinding { project_id:string;object_id:string;content_hash:string;target_baseline:string;draft_revision:string }
export type Verdict='approved'|'rejected';
export interface ReviewView { canvas_comparison?:CanvasComparison;main_revision?:string;binding:ReviewBinding; review_ticket:string;human_actor:string;current_baseline:string|null;draft:SavedDraft['draft'];diff:{path:string;before:string|null;after:string|null}[];interface_modules:string[];publish_blockers:string[];verification?:{state:'available';scan_id:string;source_revision:string;report_eid:string;report:unknown;evidence:unknown[]}|{state:'unavailable';reason:string};original_requests?:PreparedReview[];[key:string]:unknown }
export interface PreparedReview { request_id:string;binding:ReviewBinding;verdict:Verdict;comment:string;human_actor:string }
export interface PublicationResult {commit:string;publication_id:string;approval_eid:string;content_hash:string;target:string}
export interface RecordedPublication extends PublicationResult {p3_request:{request_id:string;body_hash:string};p3_receipt:{request_id:string;body_hash:string;result_ref:PublicationResult}}
export interface ReviewResponse { receipt:{request_id:string;body_hash:string;result_ref:{approval_eid:string;binding:ReviewBinding;actor:string;verdict:Verdict;comment:string}};current_validity:{state:'current'|'historical_only';reason?:string};publication?:{state:string;reason?:string;target?:string;commit?:string|null;stage?:string;created?:RecordedPublication;[key:string]:unknown} }
export interface ReviewOutcome {state:'confirmed'|'uncertain'|'conflict'|'rejected';request_id:string;response:ReviewResponse|null;error:{code:string;message:string;detail:string|null}|null}
export interface ReviewClient {load(token:string,id:string):Promise<ReviewView>;prepare(token:string,ticket:string,verdict:Verdict,comment:string):Promise<PreparedReview>;pending(token:string,id:string):Promise<PreparedReview[]>;submit(token:string,id:string):Promise<ReviewOutcome>}
const record=(value:unknown):value is Record<string,unknown>=>value!==null&&typeof value==='object'&&!Array.isArray(value);
const text=(value:unknown):value is string=>typeof value==='string'&&value.length>0;
const hash=(value:unknown):value is string=>typeof value==='string'&&/^[a-f0-9]{64}$/.test(value);
const oid=(value:unknown):value is string=>typeof value==='string'&&/^([a-f0-9]{40}|[a-f0-9]{64})$/.test(value);
const uuid=(value:unknown):value is string=>typeof value==='string'&&/^[a-f0-9]{8}-[a-f0-9]{4}-7[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/.test(value);
const human=(value:unknown):value is string=>typeof value==='string'&&/^human:[^:/\s\p{Cc}]+$/u.test(value);
const baseline=(value:unknown):value is string=>typeof value==='string'&&/^towermd\/B(0|[1-9]\d*)$/.test(value);
const bindingKeys=['project_id','object_id','content_hash','target_baseline','draft_revision'];
function binding(value:unknown):value is ReviewBinding {
 return record(value)&&Object.keys(value).length===bindingKeys.length&&bindingKeys.every(key=>key in value)&&text(value.project_id)&&text(value.object_id)&&hash(value.content_hash)&&baseline(value.target_baseline)&&oid(value.draft_revision);
}
export const sameReviewBinding=(first:ReviewBinding,second:ReviewBinding)=>bindingKeys.every(key=>first[key as keyof ReviewBinding]===second[key as keyof ReviewBinding]);
export function decodePreparedReview(value:unknown,id?:string):PreparedReview {
 if(!record(value)||!uuid(value.request_id)||!binding(value.binding)||(id!==undefined&&value.binding.object_id!==id)||!['approved','rejected'].includes(String(value.verdict))||typeof value.comment!=='string'||!human(value.human_actor))throw new Error('原基线审阅请求缺少完整 UUID、人工身份或全部版本绑定。');
 return value as unknown as PreparedReview;
}
export function decodeReviewPending(value:unknown,id:string):PreparedReview[] {
 if(!Array.isArray(value))throw new Error('无法核对本机原基线审阅队列。');
 const originals=value.map(item=>decodePreparedReview(item,id));
 if(new Set(originals.map(item=>item.request_id)).size!==originals.length)throw new Error('原基线审阅队列有重复 UUID。');
 return originals;
}
function comparison(value:unknown):value is CanvasComparison {
 if(!record(value))return false;
 if(value.state==='unavailable')return text(value.reason);
 const source=(entry:unknown)=>record(entry)&&record(entry.canvas_view)&&Array.isArray(entry.canvas_view.nodes)&&Array.isArray(entry.canvas_view.edges)&&[...entry.canvas_view.nodes,...entry.canvas_view.edges].every(item=>record(item)&&Number.isSafeInteger(item.index)&&typeof item.opaque==='boolean'&&'raw' in item)&&Array.isArray(entry.canvas_view.diagnostics)&&Array.isArray(entry.object_views)&&entry.object_views.every(item=>record(item)&&text(item.id)&&text(item.path)&&typeof item.text==='string'&&(item.type===null||typeof item.type==='string')&&(item.title===null||typeof item.title==='string'));
 return value.state==='available'&&oid(value.main_revision)&&(value.baseline===null||baseline(value.baseline))&&(value.baseline_canvas===null||source(value.baseline_canvas))&&source(value.draft_canvas)&&Array.isArray(value.diagnostics)&&value.diagnostics.every(item=>typeof item==='string');
}
export function decodeReviewView(value:unknown,id:string):ReviewView {
 if(!record(value)||!binding(value.binding)||value.binding.object_id!==id||!uuid(value.review_ticket)||!human(value.human_actor)||!(value.current_baseline===null||baseline(value.current_baseline))||!record(value.draft)||!record(value.draft.bundle)||!record(value.draft.files)||!record(value.draft.base_directory))throw new Error('基线审阅原件或当前来源不完整；不会以旧审批对象替代。');
 const subject=value.binding;const draft=value.draft;const bundle=draft.bundle as Record<string,unknown>;
 if(bundle.id!==id||bundle.project_id!==subject.project_id||bundle.target_baseline!==subject.target_baseline||draft.content_hash!==subject.content_hash||!oid(bundle.source_revision)||typeof bundle.canvas_text!=='string'||!record(bundle.objects)||!Array.isArray(bundle.questions)||!Array.isArray(bundle.risks)||!Array.isArray(bundle.deletions)
  ||!Array.isArray(value.diff)||!value.diff.every(item=>record(item)&&text(item.path)&&(item.before===null||typeof item.before==='string')&&(item.after===null||typeof item.after==='string'))||!Array.isArray(value.interface_modules)||!value.interface_modules.every(text)||!Array.isArray(value.publish_blockers)||!value.publish_blockers.every(item=>typeof item==='string'))throw new Error('基线审阅正文、差异与完整 binding 不一致。');
 if(!Object.entries(bundle.objects).every(([id,item])=>record(item)&&item.id===id&&text(item.path)&&typeof item.text==='string')||!Object.values(draft.files as Record<string,unknown>).every(item=>record(item)&&text(item.mode)&&hash(item.sha256)&&typeof item.text==='string'))throw new Error('草稿对象或冻结文件原文缺失，不能以摘要替代。');
 if(value.main_revision!==undefined&&!oid(value.main_revision))throw new Error('审阅 main revision 无效。');
 if(value.canvas_comparison!==undefined&&(!comparison(value.canvas_comparison)||(value.canvas_comparison.state==='available'&&(value.canvas_comparison.main_revision!==value.main_revision||value.canvas_comparison.baseline!==value.current_baseline))))throw new Error('画布对照不属于当前审阅的 main 或基线。');
 if(value.verification!==undefined){const verification=value.verification;if(!record(verification)||!(verification.state==='unavailable'?text(verification.reason):verification.state==='available'&&text(verification.scan_id)&&oid(verification.source_revision)&&text(verification.report_eid)&&'report' in verification&&Array.isArray(verification.evidence)))throw new Error('审阅验证证据来源不完整。');}
 if(value.original_requests!==undefined)decodeReviewPending(value.original_requests,id);
 return value as unknown as ReviewView;
}
function recordedPublication(value:unknown,original:PreparedReview,approval:string):boolean {
 if(!record(value)||!oid(value.commit)||value.target!==original.binding.target_baseline||value.content_hash!==original.binding.content_hash||value.approval_eid!==approval||value.publication_id!==approval)return false;
 const meta=value.p3_request,receipt=value.p3_receipt;
 if(!record(meta)||!text(meta.request_id)||!hash(meta.body_hash)||!record(receipt)||receipt.request_id!==meta.request_id||receipt.body_hash!==meta.body_hash)return false;
 const result=receipt.result_ref;
 return record(result)&&['commit','publication_id','approval_eid','content_hash','target'].every(key=>result[key]===value[key]);
}
export function decodeReviewOutcome(value:unknown,original:PreparedReview):ReviewOutcome {
 if(!record(value)||value.request_id!==original.request_id||!['confirmed','uncertain','conflict','rejected'].includes(String(value.state))||!(value.error===null||(record(value.error)&&typeof value.error.code==='string'&&typeof value.error.message==='string')))throw new Error('审阅结果缺少原 UUID，请保留原请求。');
 if(value.state==='confirmed'){
  const response=value.response;
  if(!record(response)||!record(response.receipt)||response.receipt.request_id!==original.request_id||!hash(response.receipt.body_hash)||!record(response.receipt.result_ref)||!record(response.current_validity)||!(response.current_validity.state==='current'||(response.current_validity.state==='historical_only'&&text(response.current_validity.reason))))throw new Error('审阅回执缺少完整原请求或历史效力说明。');
  const fact=response.receipt.result_ref;
  if(!text(fact.approval_eid)||!binding(fact.binding)||!sameReviewBinding(fact.binding,original.binding)||fact.actor!==original.human_actor||fact.verdict!==original.verdict||fact.comment!==original.comment)throw new Error('审阅回执与原决定的完整 binding、署名或意见不同。');
  if(response.publication!==undefined){const publication=response.publication;if(!record(publication)||!text(publication.state)||(publication.state==='recorded'&&(original.verdict!=='approved'||!recordedPublication(publication.created,original,fact.approval_eid))))throw new Error('出版结果缺少真实提交或目标，不能推断基线生效。');}
 }else if(value.response!==null)throw new Error('未确认审阅不能作为批准回执。');
 return value as unknown as ReviewOutcome;
}
export const reviewClient:ReviewClient={load:async(token,id)=>decodeReviewView(await invoke('load_baseline_review',{token,id}),id),prepare:async(token,reviewTicket,verdict,comment)=>decodePreparedReview(await invoke('prepare_baseline_review',{token,reviewTicket,verdict,comment})),pending:async(token,id)=>decodeReviewPending(await invoke('pending_baseline_reviews',{token,id}),id),submit:(token,requestId)=>invoke('submit_baseline_review',{token,requestId})};
export function publicationLabel(publication:ReviewResponse['publication']):string {
 if(!publication)return '出版状态未返回；批准不代表基线已生效。';
 if(publication.state==='recorded')return '基线出版已记录';
 if(publication.state==='pending'){const stages:Record<string,string>={preparation:'等待出版准备',main:'等待 main 提交',tag:'提交已发布，等待标签',record:'标签已创建，等待耐久记录'};const stage=stages[publication.stage??''];return '批准已记录 · 基线出版待完成'+(stage?`（${stage}）`:'');}
 if(publication.state==='blocked')return '批准已记录 · 基线出版受阻';
 return `基线出版状态：${publication.state}`;
}
