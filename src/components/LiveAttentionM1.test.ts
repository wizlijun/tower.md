import {cleanup,fireEvent,render,screen,within} from '@testing-library/svelte';
import {afterEach,expect,test,vi} from 'vitest';
import LiveWorkspace from './LiveWorkspace.svelte';
import {readonlyControlsFixture} from '../lib/interventions.fixture';
import {progressFixture} from '../lib/proposal-attention.test-fixtures';
import {recheckView,preparedRecheck,recheckOutcome} from '../lib/task-recheck.test-fixtures';
import type {TaskRecheckClient} from '../lib/task-recheck';
import type {ProjectOverview} from '../lib/project';
import type {ProgressTask} from '../lib/task-progress';
afterEach(cleanup);
const overview:ProjectOverview={source:'baseline',revision:'b'.repeat(40),title:'project',nodes:[{id:'module',obj:'A',objectType:'Module',title:'实际模块',kind:'text',status:'bound',text:'原文',x:25,y:60,width:300,height:180}],edges:[],diagnostics:[]};
function recheck():TaskRecheckClient{return {load:vi.fn(async()=>recheckView()),pending:vi.fn(async()=>[]),history:vi.fn(async()=>[]),prepare:vi.fn(async()=>preparedRecheck()),submit:vi.fn(async()=>recheckOutcome())};}
test('首屏最多3项复核提示且可直接到同Task Z2原件，关闭前标记不被阅读或回看过滤',async()=>{
 const view=progressFixture([]);view.daemon_instance='one';view.floor_seq=5;view.events_floor_seq=5;view.observation={state:'available',previous_close:{schema:1,project_id:view.project_id,common_dir:view.common_dir,closed_at:100,last_read:{revision:view.revision,daemon_instance:'one',through_seq:3,floor_seq:0,captured_at:50}}};view.tasks=['T','T-2','T-3','T-4'].map((id,index):ProgressTask=>({type:'Task',id,title:index===0?'实际 Task 原件':`其他 Task ${index}`,module:'A',state:'ready',needs_recheck:true}));
 const client=recheck();const onhold=vi.fn();render(LiveWorkspace,{token:'selected',overview,client:{load:async()=>view},interventionClient:readonlyControlsFixture(),recheckClient:client,onhold});
 await screen.findByText(/4 项待关注记录/);expect(screen.queryByRole('region',{name:'需要你关注'})).toBeNull();await fireEvent.contextMenu(screen.getByRole('region',{name:'真实项目画布'}));await fireEvent.click(screen.getByRole('menuitem',{name:'态势与关注'}));const focus=screen.getByRole('region',{name:'需要你关注'});expect(focus.querySelectorAll('.live-attention-item')).toHaveLength(3);expect(within(focus).getByText('4 项待关注记录')).toBeTruthy();
 await fireEvent.click(within(focus).getByRole('button',{name:'更多（1）'}));expect(within(focus).getAllByRole('button',{name:/↻ 需复核/})).toHaveLength(4);
 await fireEvent.click(within(focus).getByRole('button',{name:/实际 Task 原件 需复核/}));await screen.findByRole('region',{name:'任务运行时间线'});
 expect(client.load).not.toHaveBeenCalled();expect(onhold).not.toHaveBeenCalled();
 await fireEvent.click(screen.getByRole('button',{name:'打开复核原件'}));await screen.findByLabelText('当前 Task 完整文本');expect(client.load).toHaveBeenCalledWith('selected','T');expect(client.prepare).not.toHaveBeenCalled();expect(client.submit).not.toHaveBeenCalled();
 expect(screen.getByRole('region',{name:'模块任务依赖图'})).toBeTruthy();
});
test('Forbidden只读卡直达真实Task诊断，历史原因可见且不进入Ask或Review提交',async()=>{
 const view=progressFixture([]);view.tasks=[{type:'Task',id:'T',title:'被拦截任务',module:'A',state:'running'}];
 const path='docs/<script>literal()</script>.md';const bytes=Array.from(path,char=>char.charCodeAt(0));
 const current={type:'Hold',id:'H-forbidden',kind:'forbidden',scope:'TaskBranch',gate:'G2',task:'T',run:'R',evidence:'E',commit:'c'.repeat(40),tree:'d'.repeat(40),authority:view.revision,baseline:'towermd/B1',fences:[{name:'module:A',holder:'R',epoch:1}],rejection:{kind:'protected',path:bytes},path_bytes:bytes,path,opened_ms:1};
 view.holds=[current,{...current,id:'H-history',kind:'gatefailed',gate:'G5',rejection:{kind:'not_latest_main'},path_bytes:null,path:null,source_state:'historical_only',source_reason:'原检查 Run/fences 已失效'}];
 const onhold=vi.fn();const client=recheck();const {container}=render(LiveWorkspace,{token:'selected',overview,client:{load:async()=>view},interventionClient:readonlyControlsFixture(),recheckClient:client,onhold});
 await screen.findByText(/1 项待关注记录/);await fireEvent.contextMenu(screen.getByRole('region',{name:'真实项目画布'}));await fireEvent.click(screen.getByRole('menuitem',{name:'态势与关注'}));const focus=screen.getByRole('region',{name:'需要你关注'});expect(within(focus).getByText('1 项待关注记录')).toBeTruthy();await fireEvent.click(within(focus).getByRole('button',{name:/⛔ 禁止/}));
 await screen.findByText('检查诊断原件');expect(screen.getByText('原检查 Run/fences 已失效')).toBeTruthy();expect(screen.getByText('路径：docs/<script>literal()</script>.md')).toBeTruthy();expect(container.querySelector('script')).toBeNull();
 expect(screen.queryByRole('region',{name:'当前中断处理'})).toBeNull();expect(onhold).not.toHaveBeenCalled();expect(client.load).not.toHaveBeenCalled();expect(client.prepare).not.toHaveBeenCalled();expect(client.submit).not.toHaveBeenCalled();
});
