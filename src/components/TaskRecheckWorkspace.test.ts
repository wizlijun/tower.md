import {readonlyControlsFixture} from '../lib/interventions.fixture';
import {cleanup,fireEvent,render,screen} from '@testing-library/svelte';
import {afterEach,expect,test,vi} from 'vitest';
import LiveWorkspace from './LiveWorkspace.svelte';
import type {TaskProgressView} from '../lib/task-progress';
import type {ProjectOverview} from '../lib/project';
import type {TaskRecheckClient} from '../lib/task-recheck';
import {recheckView,preparedRecheck,recheckOutcome} from '../lib/task-recheck.test-fixtures';
import {taskFlow} from '../lib/live-overview';
afterEach(cleanup);
const revision='a'.repeat(40);
const overview:ProjectOverview={source:'baseline',revision,title:'project',nodes:[],edges:[],diagnostics:[]};
function progress(marker:unknown):TaskProgressView{return {project_id:'project',common_dir:'/project/.git',revision,captured_at:1,through_seq:1,floor_seq:0,state:'recorded',source:{kind:'hub_journal'},tasks:[{type:'Task',id:'T',title:'真实 Task',module:'A',state:'ready',needs_recheck:marker}],runs:[],evidence:[],diagnostics:[]};}
function control():TaskRecheckClient{return {load:vi.fn(async()=>recheckView()),pending:vi.fn(async()=>[]),prepare:vi.fn(async()=>preparedRecheck()),submit:vi.fn(async()=>recheckOutcome())};}
test('Z2 仅真实 needs_recheck 标记给入口，打开只读不触发解除',async()=>{
 const recheckClient=control();render(LiveWorkspace,{interventionClient:readonlyControlsFixture(),token:'selected',overview,client:{load:async()=>progress(true)},recheckClient});await screen.findByText(/任务 1/);
 await fireEvent.contextMenu(screen.getByRole('region',{name:'真实项目画布'}));await fireEvent.click(screen.getByRole('menuitem',{name:'态势与关注'}));await fireEvent.click(screen.getByText('未绑定画布模块的任务'));await fireEvent.click(screen.getByRole('button',{name:'真实 Task · 就绪'}));await screen.findByText('↻ 需复核 · 当前基线影响尚待人工核对');expect(recheckClient.load).not.toHaveBeenCalled();
 await fireEvent.click(screen.getByRole('button',{name:'打开复核原件'}));await screen.findByLabelText('当前 Task 完整文本');expect(recheckClient.load).toHaveBeenCalledWith('selected','T');expect(recheckClient.prepare).not.toHaveBeenCalled();expect(recheckClient.submit).not.toHaveBeenCalled();
});
test('字符串标记不造复核入口；真实快层角标变化不移动节点',async()=>{
 render(LiveWorkspace,{interventionClient:readonlyControlsFixture(),token:'selected',overview,client:{load:async()=>progress('true')},recheckClient:control()});await screen.findByText(/任务 1/);await fireEvent.contextMenu(screen.getByRole('region',{name:'真实项目画布'}));await fireEvent.click(screen.getByRole('menuitem',{name:'态势与关注'}));await fireEvent.click(screen.getByText('未绑定画布模块的任务'));await fireEvent.click(screen.getByRole('button',{name:'真实 Task · 就绪'}));expect(screen.queryByRole('button',{name:'打开复核原件'})).toBeNull();
 const positions=new Map<string,{x:number;y:number}>();const before=taskFlow(progress(false).tasks,positions);const after=taskFlow(progress(true).tasks,positions);expect(after.nodes[0].position).toEqual(before.nodes[0].position);expect(after.nodes[0].data.text).toContain('↻ 需复核');expect(before.nodes[0].data.text).not.toContain('↻ 需复核');
});
