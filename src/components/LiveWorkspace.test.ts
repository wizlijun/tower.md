import { cleanup, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, expect, test, vi } from 'vitest';
import LiveWorkspace from './LiveWorkspace.svelte';
import type { TaskProgressClient, TaskProgressView } from '../lib/task-progress';
import type { ProjectOverview } from '../lib/project';
afterEach(cleanup);

test('main变化后台请求新图，同版本才显示状态且更新overview不重建订阅', async () => {
  const revision = 'b'.repeat(40); const stop = vi.fn(); const onrevision = vi.fn();
  const progress: TaskProgressView = { project_id: 'project', common_dir: '/project/.git', revision, captured_at: 1800000000000, through_seq: 1, floor_seq: 0, state: 'recorded', source: { kind: 'hub_journal' }, tasks: [{ type: 'Task', id: 'T', title: '真实任务', module: 'A', state: 'verifying' }], runs: [], evidence: [], holds: [], events: [], diagnostics: [] };
  const overview: ProjectOverview = { source: 'baseline', revision: 'a'.repeat(40), title: '项目', diagnostics: [], edges: [], nodes: [{ id: 'module-card', obj: 'A', objectType: 'Module', title: '模块 A', kind: 'text', status: 'bound', text: '原始说明', x: 25, y: 60, width: 300, height: 180 }] };
  const client: TaskProgressClient = { load: vi.fn(async () => progress), subscribe: vi.fn(async () => stop) };
  const props = { token: 'selected', overview, client, onrevision };
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
 render(LiveWorkspace,{token:'selected',overview,client:{load:async()=>view},onrevision});
 await screen.findByText(/任务 1/);expect(screen.queryByText('基线版本同步中')).toBeNull();expect(onrevision).not.toHaveBeenCalled();
});
