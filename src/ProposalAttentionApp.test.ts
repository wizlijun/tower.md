import {readonlyControlsFixture} from './lib/interventions.fixture';
import {cleanup,fireEvent,render,screen} from '@testing-library/svelte';
import {afterEach,expect,test,vi} from 'vitest';
import App from './App.svelte';
import {proposalFixture,progressFixture,proposalSaved,proposalReview} from './lib/proposal-attention.test-fixtures';
import type {DraftClient} from './lib/draft';
import type {ReviewClient} from './lib/review';
import type {ProjectOverview} from './lib/project';
afterEach(cleanup);
test('真实提案关注一键沿同load_draft与ReviewPanel读取完整差异，未明确决定不写',async()=>{
 const saved=proposalSaved();const client:DraftClient={load:vi.fn(async()=>saved),pending:vi.fn(async()=>[]),prepare:vi.fn(),submit:vi.fn()};const reviewClient:ReviewClient={load:vi.fn(async()=>proposalReview()),pending:vi.fn(async()=>[]),prepare:vi.fn(),submit:vi.fn()};
 const overview:ProjectOverview={source:'baseline',revision:'b'.repeat(40),title:'真实项目',nodes:[],edges:[],diagnostics:[]};const {container}=render(App,{interventionClient:readonlyControlsFixture(),choose:async()=>({token:'selected',displayPath:'/project',hasConfig:true}),load:async()=>overview,taskProgressClient:{load:async()=>progressFixture()},savedDraftClient:client,proposalReviewClient:reviewClient});
 await fireEvent.click(screen.getByRole('button',{name:'打开项目'}));await screen.findByRole('region',{name:'真实项目画布'});
 expect(screen.queryByRole('complementary')).toBeNull();await fireEvent.click(container.querySelector<HTMLElement>('.live-flow .svelte-flow__node[data-id="__towermd_project_view__"]')!);
 const proposal=await screen.findByRole('button',{name:/待批提案.*调整接口 proposal-1/});expect(client.load).not.toHaveBeenCalled();expect(reviewClient.load).not.toHaveBeenCalled();await fireEvent.click(proposal);
 await screen.findByRole('region',{name:'基线人工审阅'});await screen.findByText('候选接口完整原文<script>literal()</script>');expect(screen.getByText('原接口原文')).toBeTruthy();expect(container.querySelector('script')).toBeNull();expect(client.load).toHaveBeenCalledWith('selected',proposalFixture().binding.object_id);expect(reviewClient.load).toHaveBeenCalledWith('selected',proposalFixture().binding.object_id);expect(screen.getAllByRole('region',{name:'基线人工审阅'})).toHaveLength(1);
 expect(client.prepare).not.toHaveBeenCalled();expect(client.submit).not.toHaveBeenCalled();expect(reviewClient.prepare).not.toHaveBeenCalled();expect(reviewClient.submit).not.toHaveBeenCalled();
});
