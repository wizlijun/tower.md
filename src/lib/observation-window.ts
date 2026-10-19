import type { TaskProgressView } from './task-progress';
import { eventsOf } from './live-overview';
const record=(value:unknown):value is Record<string,unknown>=>value!==null&&typeof value==='object'&&!Array.isArray(value);
const integer=(value:unknown):value is number=>typeof value==='number'&&Number.isSafeInteger(value)&&value>=0;
export interface PreviousClose {schema:1;project_id:string;common_dir:string;closed_at:number;last_read:{revision:string;daemon_instance:string;through_seq:number|null;floor_seq:number|null;captured_at:number}}
export type ObservationWindow={state:'available';previous_close:PreviousClose|null}|{state:'unavailable';reason:string};
export function decodeObservation(value:unknown,view:Pick<TaskProgressView,'project_id'|'common_dir'>):ObservationWindow {
 if(record(value)&&value.state==='unavailable'&&typeof value.reason==='string')return value as ObservationWindow;
 if(!record(value)||value.state!=='available')return {state:'unavailable',reason:'当前客户端没有提供上次关闭画布记录。'};
 if(value.previous_close===null)return {state:'available',previous_close:null};
 const previous=value.previous_close;
 if(!record(previous)||previous.schema!==1||previous.project_id!==view.project_id||previous.common_dir!==view.common_dir||!integer(previous.closed_at)||!record(previous.last_read)||!integer(previous.last_read.captured_at)||previous.closed_at<previous.last_read.captured_at||typeof previous.last_read.revision!=='string'||typeof previous.last_read.daemon_instance!=='string'||!(previous.last_read.through_seq===null||integer(previous.last_read.through_seq))||!(previous.last_read.floor_seq===null||integer(previous.last_read.floor_seq)))return {state:'unavailable',reason:'上次观察记录与当前项目或关闭时间不一致。'};
 return value as unknown as ObservationWindow;
}
export function changesSinceClose(view:TaskProgressView){
 const window=decodeObservation(view.observation,view);
 if(window.state==='unavailable')return {label:'自上次查看以来',events:[],diagnostics:[window.reason],closedAt:null,coverage:'unavailable' as const};
 const previous=window.previous_close;
 if(!previous)return {label:'首次查看此项目',events:[],diagnostics:['尚无上次关闭记录；当前任务与未决问题照常显示。'],closedAt:null,coverage:'first_open' as const};
 if(previous.closed_at>view.captured_at)return {label:'自上次查看以来',events:[],diagnostics:['本机时间早于上次关闭时间，无法可靠确定变化窗口。'],closedAt:previous.closed_at,coverage:'unavailable' as const};
 const events=eventsOf(view);const diagnostics:string[]=[];
 const coverage=view.state==='recorded'&&previous.last_read.daemon_instance===view.daemon_instance&&previous.last_read.through_seq!==null&&view.through_seq!==null&&previous.last_read.through_seq<=view.through_seq&&view.events_floor_seq!=null&&previous.last_read.through_seq>=view.events_floor_seq&&!events.some(event=>event.type==='hub.rebuilt')?'covered' as const:'partial' as const;
 if(coverage==='partial')diagnostics.push('当前事件窗口可能不覆盖离线期间的全部变化；以下仅列可核对记录，不推算全量进度。');
 if(events.some(event=>!Number.isFinite(Date.parse(event.t??''))))diagnostics.push('部分事件时间无法核对，不能纳入关闭时间之后的变化计数。');
 return {label:'自上次关闭画布以来',events:events.filter(event=>event.heartbeat!==true&&Number.isFinite(Date.parse(event.t??''))&&Date.parse(event.t!)>=previous.closed_at),diagnostics,closedAt:previous.closed_at,coverage};
}
