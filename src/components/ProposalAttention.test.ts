import {readonlyControlsFixture} from '../lib/interventions.fixture';
import {cleanup,fireEvent,render,screen,within} from '@testing-library/svelte';
import {afterEach,expect,test,vi} from 'vitest';
import LiveWorkspace from './LiveWorkspace.svelte';
import DraftWorkspace from './DraftWorkspace.svelte';
import {proposalFixture,progressFixture,proposalSaved,proposalReview} from '../lib/proposal-attention.test-fixtures';
import type {DraftClient} from '../lib/draft';
import type {ReviewClient} from '../lib/review';
import type {ProjectOverview} from '../lib/project';
afterEach(cleanup);
const overview:ProjectOverview={source:'baseline',revision:'b'.repeat(40),title:'project',nodes:[],edges:[],diagnostics:[]};
test('首屏只有3条，更多仍带原完整binding直接打开同一路径，不读就清掉',async()=>{
 const items=[proposalFixture('one',1),proposalFixture('two',2),proposalFixture('three',3),proposalFixture('four',4)];const onproposal=vi.fn();const progress=progressFixture(items);render(LiveWorkspace,{interventionClient:readonlyControlsFixture(),token:'selected',overview,client:{load:async()=>progress},onproposal});
 const focus=await screen.findByRole('region',{name:'需要你关注'});await within(focus).findByText('4 项待关注记录');expect(focus.querySelectorAll('.live-attention-item')).toHaveLength(3);expect(within(focus).queryByText('调整接口 one')).toBeNull();expect(onproposal).not.toHaveBeenCalled();
 await fireEvent.click(within(focus).getByRole('button',{name:'更多（1）'}));await fireEvent.click(within(focus).getByRole('button',{name:'◇ 待批提案 · 调整接口 one'}));expect(onproposal).toHaveBeenCalledWith(items[0]);expect(focus.querySelectorAll('.live-attention-item')).toHaveLength(3);expect(within(focus).getByText('4 项待关注记录')).toBeTruthy();
});
test('saved draft 第一次完整读取不匹配点击原件时保留诊断，不打开审批或自动登记',async()=>{
 const value=proposalSaved();value.revision='e'.repeat(40);const client:DraftClient={load:vi.fn(async()=>value),pending:vi.fn(async()=>[]),prepare:vi.fn(),submit:vi.fn()};const reviewClient:ReviewClient={load:vi.fn(async()=>proposalReview()),pending:vi.fn(async()=>[]),prepare:vi.fn(),submit:vi.fn()};const onreviewerror=vi.fn();
 render(DraftWorkspace,{token:'selected',client,reviewClient,reviewIntent:{key:1,binding:proposalFixture().binding},onreviewerror});await screen.findByRole('alert');expect(onreviewerror).toHaveBeenCalledWith(expect.stringContaining('已变化'));expect(screen.queryByRole('region',{name:'基线人工审阅'})).toBeNull();expect(reviewClient.load).not.toHaveBeenCalled();expect(client.prepare).not.toHaveBeenCalled();expect(reviewClient.prepare).not.toHaveBeenCalled();
});
test('saved通过但Review二次读取漂移时不显示新版决定按钮，不自动重试',async()=>{
 const value=proposalSaved();const client:DraftClient={load:vi.fn(async()=>value),pending:vi.fn(async()=>[]),prepare:vi.fn(),submit:vi.fn()};const wrong=proposalReview();wrong.binding.content_hash='e'.repeat(64);const reviewClient:ReviewClient={load:vi.fn(async()=>wrong),pending:vi.fn(async()=>[]),prepare:vi.fn(),submit:vi.fn()};
 render(DraftWorkspace,{token:'selected',client,reviewClient,reviewIntent:{key:1,binding:proposalFixture().binding}});await screen.findByText(/不会替换已点击的原件/);expect(screen.queryByRole('button',{name:'批准此版本并出版'})).toBeNull();expect(reviewClient.load).toHaveBeenCalledTimes(1);expect(reviewClient.prepare).not.toHaveBeenCalled();expect(reviewClient.submit).not.toHaveBeenCalled();
});
