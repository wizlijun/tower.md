import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, expect, test, vi } from 'vitest';
import LiveWorkspace from './LiveWorkspace.svelte';
import type { TaskProgressClient, TaskProgressView } from '../lib/task-progress';
import type { ProjectOverview } from '../lib/project';
import type { InterventionClient } from '../lib/interventions';
import { viewFixture, originalFixture, confirmedFixture } from '../lib/interventions.fixture';
const controls = (): InterventionClient => ({ load: async () => viewFixture(), pending: async () => [], prepare: async (_token, _ticket, action) => originalFixture(action), submit: async () => confirmedFixture(), requestResume: async () => {} });
afterEach(cleanup);

test('main变化后台请求新图，同版本才显示状态且更新overview不重建订阅', async () => {
  const revision = 'b'.repeat(40); const stop = vi.fn(); const onrevision = vi.fn();
  const progress: TaskProgressView = { project_id: 'project', common_dir: '/project/.git', revision, captured_at: 1800000000000, through_seq: 1, floor_seq: 0, state: 'recorded', source: { kind: 'hub_journal' }, tasks: [{ type: 'Task', id: 'T', title: '真实任务', module: 'A', state: 'verifying' }], runs: [], evidence: [], holds: [], events: [], diagnostics: [] };
  const overview: ProjectOverview = { source: 'baseline', revision: 'a'.repeat(40), title: '项目', diagnostics: [], edges: [], nodes: [{ id: 'module-card', obj: 'A', objectType: 'Module', title: '模块 A', kind: 'text', status: 'bound', text: '原始说明', x: 25, y: 60, width: 300, height: 180 }] };
  const client: TaskProgressClient = { load: vi.fn(async () => progress), subscribe: vi.fn(async () => stop) };
  const props = { token: 'selected', overview, client, onrevision, interventionClient: controls() };
  const rendered = render(LiveWorkspace, props);
  await waitFor(() => expect(onrevision).toHaveBeenCalledWith(revision));
  expect(screen.getByText('基线版本同步中')).toBeTruthy();
  expect(screen.queryByText(/0\/1 已合并/)).toBeNull();
  expect(client.load).toHaveBeenCalledTimes(1);
  await rendered.rerender({ ...props, overview: { ...overview, revision } });
  await waitFor(() => expect(screen.getAllByText(/0\/1 已合并/).length).toBeGreaterThan(0));
  expect(onrevision).toHaveBeenCalledTimes(1);
  expect(client.load).toHaveBeenCalledTimes(1);
  expect(client.subscribe).toHaveBeenCalledTimes(1);
  expect(stop).not.toHaveBeenCalled();
  rendered.unmount(); expect(stop).toHaveBeenCalledTimes(1);
});

test('任务工作树的目录HEAD与main分开，不因HEAD不同永久阻塞真实任务',async()=>{
 const revision='b'.repeat(40);const onrevision=vi.fn();const view:TaskProgressView={project_id:'project',common_dir:'/project/.git',revision,captured_at:1,through_seq:2,floor_seq:0,state:'recorded',source:{kind:'hub_journal'},tasks:[{type:'Task',id:'T',title:'分支上的真实任务',module:'A',state:'ready'}],runs:[],evidence:[],holds:[],events:[],diagnostics:[]};
 const overview:ProjectOverview={source:'repository',revision:'a'.repeat(40),authorityRevision:revision,title:'任务分支',nodes:[],edges:[],diagnostics:[]};
 render(LiveWorkspace,{token:'selected',overview,client:{load:async()=>view},onrevision,interventionClient:controls()});
 await screen.findByText(/任务 1/);expect(screen.queryByText('基线版本同步中')).toBeNull();expect(onrevision).not.toHaveBeenCalled();
});


test('真实关闭回看与模块依赖复用同一视图，旧未决问题与常驻输入不因下钻消失', async () => {
 const revision='b'.repeat(40); const control=controls(); const prepare=vi.spyOn(control,'prepare');
 const progress:TaskProgressView={project_id:'P',common_dir:'/project/.git',revision,daemon_instance:'one',captured_at:4000,through_seq:4,floor_seq:0,state:'recorded',source:{kind:'hub_journal'},
  tasks:[{type:'Task',id:'T-A',title:'前置任务',module:'M-1',state:'verifying'},{type:'Task',id:'T-B',title:'后续任务',module:'M-1',state:'ready',depends_on:['T-A']}],runs:[],evidence:[],
  holds:[{type:'Hold',id:'H-old',body:{kind:'ask',question:'关闭前仍未答的问题'},recipients:[{task:'T-A'}],reply:null,attention:true,opened_ms:1}],diagnostics:[],
  events:[{seq:4,eid:'actual-event',obj:'T-A',type:'task.state',data:{},t:'1970-01-01T00:00:03.000Z'}],events_floor_seq:3,
  observation:{state:'available',previous_close:{schema:1,project_id:'P',common_dir:'/project/.git',closed_at:2000,last_read:{revision,daemon_instance:'one',through_seq:3,floor_seq:0,captured_at:1000}}}};
 const overview:ProjectOverview={source:'baseline',revision:'a'.repeat(40),authorityRevision:revision,title:'项目',edges:[],diagnostics:[],nodes:[{id:'module-card',obj:'M-1',objectType:'Module',kind:'text',status:'bound',title:'实际模块',text:'原文',x:25,y:60,width:300,height:180}]};
 render(LiveWorkspace,{token:'selected',overview,client:{load:async()=>progress},interventionClient:control});
 await screen.findByText('关闭前仍未答的问题');
 expect(screen.getByText(/自上次关闭画布以来/)).toBeTruthy();
 await waitFor(()=>expect((screen.getByLabelText('插话内容') as HTMLTextAreaElement).disabled).toBe(false));
 const modules = screen.getByRole('region',{name:'模块状态与下钻'});
 await fireEvent.click(within(modules).getByRole('button',{name:/实际模块/}));
 expect(screen.getByText(/最长未完成依赖链.*T-A → T-B/)).toBeTruthy();
 expect(screen.getByLabelText('插话内容')).toBeTruthy();
 expect(screen.getByText('关闭前仍未答的问题')).toBeTruthy();
 expect(prepare).not.toHaveBeenCalled();
});
