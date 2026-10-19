import {expect,test,vi} from 'vitest';
import {decodeTaskProgress,taskProgressClient,progressStateLabel,evidenceResultLabel} from './task-progress';
vi.mock('@tauri-apps/api/core',()=>({invoke:vi.fn()}));
import {invoke} from '@tauri-apps/api/core';
export function progressFixture(){return {project_id:'project',common_dir:'/owned/.git',revision:'a'.repeat(40),captured_at:1800000000000,through_seq:12,floor_seq:0,state:'recorded' as const,source:{kind:'hub_journal' as const},tasks:[{type:'Task' as const,id:'T',title:'真实任务',module:'A',state:'verifying',future:{keep:[2,1]}}],runs:[{type:'Run' as const,id:'R',task:'T',state:'verifying',cost_usd:null}],evidence:[{type:'Evidence' as const,id:'E',task:'T',run:'R',result:'passed',commit:'b'.repeat(40)}],diagnostics:[]};}
test('读取只调用固定原生命令，宽容保留原始扩展与未知费用',async()=>{const raw=progressFixture();vi.mocked(invoke).mockResolvedValueOnce(raw);expect(await taskProgressClient.load('selected')).toBe(raw);expect(invoke).toHaveBeenLastCalledWith('load_task_progress',{token:'selected'});expect(raw.runs[0].cost_usd).toBeNull();expect(raw.tasks[0].future.keep).toEqual([2,1]);});
test('拒绝缺身份、伪造水位、重复对象和把恢复来源当当前运行',()=>{for(const changed of [{project_id:''},{through_seq:null},{floor_seq:13},{state:'recovery_required'},{source:{kind:'none'}},{tasks:[progressFixture().tasks[0],progressFixture().tasks[0]]}])expect(()=>decodeTaskProgress({...progressFixture(),...changed})).toThrow();});
test('恢复历史和无来源保持各自身份，不补造当前水位或Run',()=>{const history={...progressFixture(),state:'recovery_required',source:{kind:'recovery_snapshot',commit:'c'.repeat(40),reference:'main'},floor_seq:null,runs:[],evidence:[]};expect(decodeTaskProgress(history).state).toBe('recovery_required');const empty={...history,state:'unavailable',source:{kind:'none'},through_seq:null,tasks:[]};expect(decodeTaskProgress(empty).through_seq).toBeNull();});
test('验证通过不等于已合并，未知状态保原文',()=>{expect(evidenceResultLabel('passed')).toBe('验证通过');expect(progressStateLabel('verifying')).not.toBe('已合并');expect(progressStateLabel('merged')).toBe('已合并');expect(progressStateLabel('future-state')).toBe('future-state');});

test('实际事件窗口保留未知JSON，但拒绝水位缺口和历史当前中断',()=>{
 const raw={...progressFixture(),holds:[],events_floor_seq:11,events:[{seq:12,eid:'event',obj:'T',type:'future.event',t:'2026-10-19T01:00:00Z',data:null}]};
 expect(decodeTaskProgress(raw).events?.[0].data).toBeNull();
 expect(()=>decodeTaskProgress({...raw,events_floor_seq:10})).toThrow();
 expect(()=>decodeTaskProgress({...raw,state:'recovery_required',source:{kind:'recovery_snapshot',commit:'c'.repeat(40),reference:'main'},floor_seq:null,runs:[],evidence:[]})).toThrow();
});

test('只接受固定本机订阅并等待真实open，reset断开并交还重读',async()=>{
 const {subscribeTaskProgress}=await import('./task-progress');
 let socket:FakeSocket|undefined;
 class FakeSocket {
  onopen:((event:Event)=>void)|null=null;onmessage:((event:MessageEvent)=>void)|null=null;onerror:((event:Event)=>void)|null=null;onclose:((event:CloseEvent)=>void)|null=null;
  constructor(public url:string){socket=this;}
  close=vi.fn(()=>this.onclose?.({} as CloseEvent));
 }
 vi.stubGlobal('WebSocket',FakeSocket);
 try{
  vi.mocked(invoke).mockResolvedValueOnce({url:'ws://127.0.0.1:2222/app/events?directory=%2Fowned&instance=actual&after=12',instance:'actual',project_id:'project',revision:'a'.repeat(40)});
  const changed=vi.fn(),error=vi.fn();let connected=false;
  const pending=subscribeTaskProgress('selected',12,changed,error,{project_id:'project',revision:'a'.repeat(40),daemon_instance:'actual'}).then(stop=>{connected=true;return stop;});
  await vi.waitFor(()=>expect(socket).toBeDefined());expect(connected).toBe(false);socket!.onopen?.(new Event('open'));const stop=await pending;
  const frame={kind:'changed',through_seq:13,floor_seq:0,revision:'a'.repeat(40)};socket!.onmessage?.({data:JSON.stringify(frame)} as MessageEvent);expect(changed).toHaveBeenCalledWith(frame);
  socket!.onmessage?.({data:JSON.stringify({kind:'reset',reason:'hub 已重建'})} as MessageEvent);expect(error.mock.calls[0][0].message).toBe('hub 已重建');expect(error.mock.calls[0][0].code).toBe('task_progress_stream_reset');expect(socket!.close).toHaveBeenCalledTimes(1);stop();
 }finally{vi.unstubAllGlobals();}
});

test('不同实例的descriptor不能接到旧状态，也拒绝非本机URL',async()=>{
 const {subscribeTaskProgress}=await import('./task-progress');
 for(const url of ['ws://example.invalid/app/events?directory=%2Fowned&instance=new&after=12','ws://127.0.0.1:2222/app/events?directory=%2Fowned&instance=new&after=12']){
  vi.mocked(invoke).mockResolvedValueOnce({url,instance:'new',project_id:'project',revision:'a'.repeat(40)});
  await expect(subscribeTaskProgress('selected',12,vi.fn(),vi.fn(),{project_id:'project',revision:'a'.repeat(40),daemon_instance:'old'})).rejects.toThrow();
 }
});
