import {invoke} from '@tauri-apps/api/core';
export interface RecoveryRequest {
 status:'awaiting_app_confirmation';request_id:string;project_id:string;main_revision:string;source_commit:string;source_seq:number;seq_boundary:number;epoch_boundary:number;source_epoch_high_water:number;source_reference:string;inputs_digest:string;requested_at:number;expires_at:number;nonce:string;
}
export interface RecoveryView {request:RecoveryRequest;human_actor:string;can_confirm:boolean}
export interface RebuildReport {status:'rebuilt';commit:string;seq:number;epoch:number;reason:null}
export interface RecoveryClient {load(token:string,id:string):Promise<RecoveryView>;confirm(token:string,id:string):Promise<RebuildReport>}
export const validRecoveryId=(id:string)=>id.length===64&&/^[a-f0-9]{64}$/.test(id);
const record=(value:unknown):value is Record<string,unknown>=>!!value&&typeof value==='object'&&!Array.isArray(value);
export function decodeRecoveryView(value:unknown,id:string):RecoveryView {
 if(!record(value)||!record(value.request)||!validRecoveryId(id)||value.request.request_id!==id||value.request.status!=='awaiting_app_confirmation'||typeof value.human_actor!=='string'||!value.human_actor.startsWith('human:')||!value.human_actor.slice(6)||/[:/\s\p{Cc}]/u.test(value.human_actor.slice(6))||typeof value.can_confirm!=='boolean')throw new Error('恢复请求缺少真实保存身份或本机人工身份。');
 const request=value.request;
 for(const key of ['project_id','main_revision','source_commit','source_reference','inputs_digest','nonce'])if(typeof request[key]!=='string'||!request[key])throw new Error(`恢复请求缺少 ${key}。`);
 for(const key of ['source_seq','seq_boundary','epoch_boundary','source_epoch_high_water','requested_at','expires_at'])if(typeof request[key]!=='number'||!Number.isSafeInteger(request[key])||request[key]<0)throw new Error(`恢复请求 ${key} 不是可精确显示的整数。`);
 if(!Number.isFinite(new Date(request.requested_at as number).getTime())||!Number.isFinite(new Date(request.expires_at as number).getTime())||(request.expires_at as number)<=(request.requested_at as number))throw new Error('恢复请求原期限无效。');
 return value as unknown as RecoveryView;
}
export function decodeRebuildReport(value:unknown):RebuildReport {
 if(!record(value)||value.status!=='rebuilt'||typeof value.commit!=='string'||!/^([a-f0-9]{40}|[a-f0-9]{64})$/.test(value.commit)||!Number.isSafeInteger(value.seq)||!Number.isSafeInteger(value.epoch)||value.reason!==null)throw new Error('恢复结果缺少真实 Git 边界或高水位；不能推断恢复完成。');
 return value as unknown as RebuildReport;
}
export const recoveryClient:RecoveryClient={
 load:async(token,id)=>decodeRecoveryView(await invoke('load_rebuild_request',{token,id}),id),
 confirm:async(token,id)=>decodeRebuildReport(await invoke('confirm_rebuild',{token,id})),
};
