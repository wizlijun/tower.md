import { invoke } from '@tauri-apps/api/core';
import type {SavedDraft} from './draft';
import type {CanvasSource} from './canvas-flow';
export type CanvasComparison = {state:'available';main_revision:string;baseline:string|null;baseline_canvas:CanvasSource|null;draft_canvas:CanvasSource;diagnostics:string[]}|{state:'unavailable';reason:string};
export interface ReviewBinding { project_id:string;object_id:string;content_hash:string;target_baseline:string;draft_revision:string }
export type Verdict='approved'|'rejected';
export interface ReviewView { canvas_comparison?:CanvasComparison;main_revision?:string;binding:ReviewBinding; review_ticket:string;human_actor:string;current_baseline:string|null;draft:SavedDraft['draft'];diff:{path:string;before:string|null;after:string|null}[];interface_modules:string[];publish_blockers:string[];verification?:{state:'available';scan_id:string;source_revision:string;report_eid:string;report:unknown;evidence:unknown[]}|{state:'unavailable';reason:string};original_requests?:PreparedReview[];[key:string]:unknown }
export interface PreparedReview { request_id:string;binding:ReviewBinding;verdict:Verdict;comment:string;human_actor:string }
export interface ReviewResponse { receipt:{request_id:string;body_hash:string;result_ref:{approval_eid:string;binding:ReviewBinding;actor:string;verdict:Verdict;comment:string}};current_validity:{state:'current'|'historical_only';reason?:string};publication?:{state:string;reason?:string;target?:string;commit?:string|null;stage?:string;[key:string]:unknown} }
export interface ReviewOutcome {state:'confirmed'|'uncertain'|'conflict'|'rejected';request_id:string;response:ReviewResponse|null;error:{code:string;message:string;detail:string|null}|null}
export interface ReviewClient {load(token:string,id:string):Promise<ReviewView>;prepare(token:string,ticket:string,verdict:Verdict,comment:string):Promise<PreparedReview>;pending(token:string,id:string):Promise<PreparedReview[]>;submit(token:string,id:string):Promise<ReviewOutcome>}
export const reviewClient:ReviewClient={load:(token,id)=>invoke('load_baseline_review',{token,id}),prepare:(token,reviewTicket,verdict,comment)=>invoke('prepare_baseline_review',{token,reviewTicket,verdict,comment}),pending:(token,id)=>invoke('pending_baseline_reviews',{token,id}),submit:(token,requestId)=>invoke('submit_baseline_review',{token,requestId})};
export function publicationLabel(publication:ReviewResponse['publication']):string {
 if(!publication)return '出版状态未返回；批准不代表基线已生效。';
 if(publication.state==='recorded')return '基线出版已记录';
 if(publication.state==='pending'){const stages:Record<string,string>={preparation:'等待出版准备',main:'等待 main 提交',tag:'提交已发布，等待标签',record:'标签已创建，等待耐久记录'};const stage=stages[publication.stage??''];return '批准已记录 · 基线出版待完成'+(stage?`（${stage}）`:'');}
 if(publication.state==='blocked')return '批准已记录 · 基线出版受阻';
 return `基线出版状态：${publication.state}`;
}
