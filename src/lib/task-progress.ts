import {invoke} from '@tauri-apps/api/core';

export interface ProgressTask {type:'Task';id:string;state:string;title?:string;module?:string;[key:string]:unknown}
export interface ProgressRun {type:'Run';id:string;task:string;state:string;host?:string;[key:string]:unknown}
export interface ProgressEvidence {type:'Evidence';id:string;task?:string;run?:string;kind?:string;result?:string;commit?:string;[key:string]:unknown}
export interface ProgressEvent {seq:number;eid:string;obj:string;type:string;t:string;data:unknown;[key:string]:unknown}
export interface TaskProgressView {
 project_id:string;common_dir:string;revision:string;captured_at:number;through_seq:number|null;floor_seq:number|null;
 state:'recorded'|'recovery_required'|'unavailable';
 source:{kind:'hub_journal'}|{kind:'recovery_snapshot';commit:string;reference:string}|{kind:'none'};
 tasks:ProgressTask[];runs:ProgressRun[];evidence:ProgressEvidence[];diagnostics:string[];
 daemon_instance?:string;holds?:Record<string,unknown>[];events?:ProgressEvent[];events_floor_seq?:number|null;
}
export interface ProgressChange {kind:'changed';through_seq:number;floor_seq:number;revision:string}
export interface TaskProgressClient {
 load(token:string):Promise<TaskProgressView>;
 subscribe?(token:string,after:number,onChange:(change:ProgressChange)=>void,onError:(error:Error)=>void,expected?:{project_id:string;revision:string;daemon_instance?:string}):Promise<()=>void>;
}
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
 if(value.holds!==undefined&&(!Array.isArray(value.holds)||!value.holds.every(item=>record(item)&&item.type==='Hold'&&text(item.id))))throw new Error('任务中断记录格式不完整。');
 if(value.events!==undefined){
  if(!Array.isArray(value.events)||value.events.length>256||!value.events.every(item=>record(item)&&integer(item.seq)&&text(item.eid)&&text(item.obj)&&text(item.type)&&text(item.t)&&Object.hasOwn(item,'data')))throw new Error('运行时间线格式不完整。');
  const events=value.events as ProgressEvent[];
  if(events.length&&(!integer(value.events_floor_seq)||value.state!=='recorded'||!integer(value.through_seq)||events[0].seq!==value.events_floor_seq+1||events.at(-1)!.seq!==value.through_seq||events.some((event,i)=>i>0&&event.seq!==events[i-1].seq+1)))throw new Error('运行时间线缺少连续水位。');
 }
 if(value.state!=='recorded'&&((Array.isArray(value.holds)&&value.holds.length)||(Array.isArray(value.events)&&value.events.length)))throw new Error('历史快照不能冒充当前中断或事件。');
 return value as unknown as TaskProgressView;
}
export async function subscribeTaskProgress(token:string,after:number,onChange:(change:ProgressChange)=>void,onError:(error:Error)=>void,expected?:{project_id:string;revision:string;daemon_instance?:string}):Promise<()=>void>{
 if(!integer(after))throw new Error('任务进度订阅水位无效。');
 const descriptor=await invoke<unknown>('open_task_progress_stream',{token,after});
 if(!record(descriptor)||!text(descriptor.url)||!text(descriptor.instance)||!text(descriptor.project_id)||!oid(descriptor.revision))throw new Error('缺少本机事件连接身份。');
 if(expected&&(descriptor.project_id!==expected.project_id||descriptor.revision!==expected.revision||(expected.daemon_instance!==undefined&&descriptor.instance!==expected.daemon_instance)))throw new Error('项目来源或本机服务已改变，请重新读取状态。');
 const url=new URL(descriptor.url);
 if(url.protocol!=='ws:'||url.hostname!=='127.0.0.1'||!url.port||url.username||url.password||url.hash||url.pathname!=='/app/events'||url.searchParams.get('instance')!==descriptor.instance||url.searchParams.get('after')!==String(after)||!url.searchParams.get('directory')||[...url.searchParams.keys()].some(key=>!['directory','instance','after'].includes(key)))throw new Error('事件连接不是所选项目的固定本机读取路径。');
 const socket=new WebSocket(url.href);let closed=false;let cursor=after;
 const stop=()=>{closed=true;socket.close();};
 const fail=(message:string)=>{if(closed)return;stop();onError(new Error(message));};
 socket.onmessage=event=>{
  if(closed)return;
  try{
   const frame:unknown=JSON.parse(String(event.data));
   if(record(frame)&&frame.kind==='reset'){fail(typeof frame.reason==='string'?frame.reason:'事件水位已改变，正在重新读取');return;}
   if(!record(frame)||frame.kind!=='changed'||!integer(frame.through_seq)||!integer(frame.floor_seq)||frame.floor_seq>frame.through_seq||frame.through_seq<cursor||!oid(frame.revision))throw new Error('事件通知格式或水位无效。');
   cursor=frame.through_seq;onChange(frame as unknown as ProgressChange);
  }catch(error){fail(error instanceof Error?error.message:'事件读取失败');}
 };
 socket.onerror=()=>fail('与本机服务的实时连接中断');
 socket.onclose=()=>{if(!closed){closed=true;onError(new Error('实时连接已关闭，显示最后已核实记录'));}};
 return new Promise<()=>void>((resolve,reject)=>{
  const timer=setTimeout(()=>{if(!closed){stop();reject(new Error('实时连接超时'));}},5000);
  socket.onopen=()=>{clearTimeout(timer);if(!closed)resolve(stop);};
  const error=socket.onerror;const close=socket.onclose;
  socket.onerror=event=>{clearTimeout(timer);error?.call(socket,event);reject(new Error('实时连接失败'));};
  socket.onclose=event=>{clearTimeout(timer);close?.call(socket,event);reject(new Error('实时连接已关闭'));};
 });
}
export const taskProgressClient:TaskProgressClient={load:async(token)=>decodeTaskProgress(await invoke('load_task_progress',{token})),subscribe:subscribeTaskProgress};
export function progressStateLabel(state:string):string {
 const labels:Record<string,string>={proposed:'待领取',ready:'就绪',running:'运行中',held:'等待处理',verifying:'验证与收尾中',merged:'已合并',blocked:'受阻',released:'已释放',ended:'已结束',archived:'历史归档',expired:'已到期'};
 return labels[state]??state;
}
export function evidenceResultLabel(result:unknown):string {
 const labels:Record<string,string>={passed:'验证通过',failed:'验证失败',no_tests:'未配置测试'};
 return typeof result==='string'?(labels[result]??result):'结果未提供';
}
