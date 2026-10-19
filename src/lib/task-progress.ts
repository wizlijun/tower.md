import {invoke} from '@tauri-apps/api/core';

export interface ProgressTask {type:'Task';id:string;state:string;title?:string;module?:string;[key:string]:unknown}
export interface ProgressRun {type:'Run';id:string;task:string;state:string;host?:string;[key:string]:unknown}
export interface ProgressEvidence {type:'Evidence';id:string;task?:string;run?:string;kind?:string;result?:string;commit?:string;[key:string]:unknown}
export interface TaskProgressView {
 project_id:string;common_dir:string;revision:string;captured_at:number;through_seq:number|null;floor_seq:number|null;
 state:'recorded'|'recovery_required'|'unavailable';
 source:{kind:'hub_journal'}|{kind:'recovery_snapshot';commit:string;reference:string}|{kind:'none'};
 tasks:ProgressTask[];runs:ProgressRun[];evidence:ProgressEvidence[];diagnostics:string[];
}
export interface TaskProgressClient {load(token:string):Promise<TaskProgressView>}
const record=(value:unknown):value is Record<string,unknown>=>value!==null&&typeof value==='object'&&!Array.isArray(value);
const text=(value:unknown):value is string=>typeof value==='string'&&value.length>0;
const integer=(value:unknown):value is number=>typeof value==='number'&&Number.isSafeInteger(value)&&value>=0;
const oid=(value:unknown)=>typeof value==='string'&&/^([a-f0-9]{40}|[a-f0-9]{64})$/.test(value);
export function decodeTaskProgress(value:unknown):TaskProgressView {
 if(!record(value)||!text(value.project_id)||!text(value.common_dir)||!oid(value.revision)||!integer(value.captured_at)||!record(value.source)
  ||!Array.isArray(value.tasks)||!Array.isArray(value.runs)||!Array.isArray(value.evidence)||!Array.isArray(value.diagnostics)||!value.diagnostics.every(item=>typeof item==='string'))throw new Error('任务进度缺少实际项目、来源或记录水位。');
 const coherent=value.state==='recorded'?value.source.kind==='hub_journal'&&integer(value.through_seq)&&integer(value.floor_seq)&&value.floor_seq<=value.through_seq
  :value.state==='recovery_required'?value.source.kind==='recovery_snapshot'&&oid(value.source.commit)&&['main','refs/towermd/snapshot'].includes(String(value.source.reference))&&integer(value.through_seq)&&value.floor_seq===null&&value.runs.length===0&&value.evidence.length===0
  :value.state==='unavailable'&&value.source.kind==='none'&&value.through_seq===null&&value.floor_seq===null&&value.tasks.length===0&&value.runs.length===0&&value.evidence.length===0;
 if(!coherent||!value.tasks.every(item=>record(item)&&item.type==='Task'&&text(item.id)&&text(item.state))
  ||!value.runs.every(item=>record(item)&&item.type==='Run'&&text(item.id)&&text(item.task)&&text(item.state))
  ||!value.evidence.every(item=>record(item)&&item.type==='Evidence'&&text(item.id)))throw new Error('任务进度的记录来源、对象或水位不一致。');
 for(const items of [value.tasks,value.runs,value.evidence])if(new Set(items.map(item=>(item as Record<string,unknown>).id)).size!==items.length)throw new Error('任务进度存在重复对象 ID。');
 return value as unknown as TaskProgressView;
}
export const taskProgressClient:TaskProgressClient={load:async(token)=>decodeTaskProgress(await invoke('load_task_progress',{token}))};
export function progressStateLabel(state:string):string {
 const labels:Record<string,string>={proposed:'待领取',ready:'就绪',running:'运行中',held:'等待处理',verifying:'验证与收尾中',merged:'已合并',blocked:'受阻',released:'已释放',ended:'已结束',archived:'历史归档',expired:'已到期'};
 return labels[state]??state;
}
export function evidenceResultLabel(result:unknown):string {
 const labels:Record<string,string>={passed:'验证通过',failed:'验证失败',no_tests:'未配置测试'};
 return typeof result==='string'?(labels[result]??result):'结果未提供';
}
