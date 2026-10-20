import {afterEach,expect,test,vi} from 'vitest';
import {act,cleanup,fireEvent,render,screen,waitFor} from '@testing-library/svelte';
import TaskProgressPanel from './TaskProgressPanel.svelte';
import App from '../App.svelte';
import type {TaskProgressView} from '../lib/task-progress';
afterEach(cleanup);
function view():TaskProgressView{return {project_id:'P',common_dir:'/owned/.git',revision:'a'.repeat(40),captured_at:1800000000000,through_seq:42,floor_seq:0,state:'recorded',source:{kind:'hub_journal'},tasks:Array.from({length:4},(_,i)=>({type:'Task',id:`T-${i}`,title:`任务 ${i}`,module:'A',state:'verifying'})),runs:[{type:'Run',id:'R',task:'T-0',state:'verifying',cost_usd:null}],evidence:[{type:'Evidence',id:'E-real',task:'T-0',run:'R',result:'passed',commit:'b'.repeat(40)}],diagnostics:['记录状态不代表进程存活。']};}
test('明确读取与刷新，最多先展示三项，验证证据不冒充已合并',async()=>{const load=vi.fn(async()=>view());render(TaskProgressPanel,{token:'selected',client:{load}});expect(load).not.toHaveBeenCalled();await fireEvent.click(screen.getByRole('button',{name:'读取任务进度'}));await screen.findByText('任务 0');expect(screen.queryByText('任务 3')).toBeNull();expect(screen.getByText(/截至 seq 42/)).toBeTruthy();expect(screen.getByText(/验证通过/)).toBeTruthy();expect(screen.queryByText('已合并')).toBeNull();expect(screen.queryByRole('progressbar')).toBeNull();await fireEvent.click(screen.getByRole('button',{name:'查看全部（4）'}));expect(screen.getByText('任务 3')).toBeTruthy();await fireEvent.click(screen.getByRole('button',{name:'刷新任务进度'}));await waitFor(()=>expect(load).toHaveBeenCalledTimes(2));expect(load).toHaveBeenNthCalledWith(1,'selected');expect(load).toHaveBeenNthCalledWith(2,'selected');});
test('历史来源、无记录和真实读取失败明确区分，不保旧图伪装最新',async()=>{const history=view();history.state='recovery_required';history.source={kind:'recovery_snapshot',commit:'c'.repeat(40),reference:'main'};history.floor_seq=null;history.runs=[];history.evidence=[];const load=vi.fn().mockResolvedValueOnce(history).mockRejectedValueOnce(new Error('来源变化，请刷新'));render(TaskProgressPanel,{token:'selected',client:{load}});await fireEvent.click(screen.getByRole('button',{name:'读取任务进度'}));await screen.findByText(/快照历史 · 需恢复/);await fireEvent.click(screen.getByRole('button',{name:'刷新任务进度'}));await screen.findByRole('alert');expect(screen.queryByText('任务 0')).toBeNull();cleanup();const empty=view();empty.state='unavailable';empty.source={kind:'none'};empty.tasks=[];empty.runs=[];empty.evidence=[];empty.floor_seq=null;empty.through_seq=null;render(TaskProgressPanel,{token:'selected',client:{load:async()=>empty}});await fireEvent.click(screen.getByRole('button',{name:'读取任务进度'}));await screen.findByText(/尚无可展示的任务/);expect(screen.queryByText(/截至 seq/)).toBeNull();});
test('切项目清空状态并拒绝迟到的旧记录，切换不会自动读取',async()=>{let resolve!:(view:TaskProgressView)=>void;const load=vi.fn(()=>new Promise<TaskProgressView>(done=>resolve=done));const panel=render(TaskProgressPanel,{token:'old',client:{load}});await fireEvent.click(screen.getByRole('button',{name:'读取任务进度'}));await waitFor(()=>expect(load).toHaveBeenCalledTimes(1));await panel.rerender({token:'new',client:{load}});await act(()=>resolve(view()));expect(screen.queryByText('任务 0')).toBeNull();expect(screen.getByRole('button',{name:'读取任务进度'})).toBeTruthy();expect(load).toHaveBeenCalledTimes(1);});
test('已配置项目自动读取真实态势，诊断面板仅显式读取而不产生写入动作',async()=>{
 const load=vi.fn(async()=>view()); const stop=vi.fn(); const subscribe=vi.fn(async()=>stop);
 render(App,{choose:async()=>({token:'selected',displayPath:'/owned',hasConfig:true}),load:async()=>({source:'baseline' as const,revision:'a'.repeat(40),title:'项目',nodes:[],edges:[],diagnostics:[]}),taskProgressClient:{load,subscribe}});
 await fireEvent.click(screen.getByRole('button',{name:'打开项目'}));await screen.findByRole('region',{name:'真实项目画布'});await fireEvent.contextMenu(screen.getByRole('main'));await fireEvent.click(screen.getByRole('menuitem',{name:'项目工具'}));
 await screen.findByRole('region',{name:'真实项目画布'});
 await waitFor(()=>expect(load).toHaveBeenCalledTimes(1)); await waitFor(()=>expect(subscribe).toHaveBeenCalledTimes(1));
 expect(screen.queryByRole('region',{name:'任务进度'})).toBeNull();
 await fireEvent.click(screen.getByRole('button',{name:'任务记录诊断'}));
 const diagnostics=screen.getByRole('region',{name:'任务进度'}); expect(diagnostics).toBeTruthy();
 expect(load).toHaveBeenCalledTimes(1);
 await fireEvent.click(screen.getByRole('button',{name:'读取任务进度'}));await waitFor(()=>expect(load).toHaveBeenCalledTimes(2));
 expect(subscribe).toHaveBeenCalledTimes(1);expect(stop).not.toHaveBeenCalled();
 expect(load).toHaveBeenNthCalledWith(1,'selected');expect(load).toHaveBeenNthCalledWith(2,'selected');
 expect(screen.queryByRole('button',{name:/^(批准|确认合并|完成任务)$/})).toBeNull();expect(screen.queryByRole('progressbar')).toBeNull();
});
