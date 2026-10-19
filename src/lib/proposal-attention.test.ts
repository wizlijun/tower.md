import {expect,test,vi} from 'vitest';
import {bindProposalReview,decodeProposalAttention} from './proposal-attention';
import {attentionItems} from './live-overview';
import {decodeTaskProgress} from './task-progress';
import {proposalFixture,progressFixture,proposalReview} from './proposal-attention.test-fixtures';
import type {ReviewClient} from './review';
test('真实提案枚举要求 review_pending 和完整5字段/影响，历史投影不能带当前提案',()=>{
 const item=proposalFixture();expect(decodeProposalAttention([item],'project',5)).toEqual([item]);
 for(const change of [(v:typeof item)=>v.binding.project_id='other',(v:typeof item)=>v.created_seq=6,(v:typeof item)=>v.binding.content_hash='missing',(v:typeof item)=>v.impact.tasks.push('T0')]){const wrong=structuredClone(item);change(wrong);expect(()=>decodeProposalAttention([wrong],'project',5)).toThrow();}
 expect(()=>decodeProposalAttention([{...item,state:'publication_pending'}],'project',5)).toThrow();const unknown=structuredClone(item);Object.assign(unknown.binding,{skip:true});expect(()=>decodeProposalAttention([unknown],'project',5)).toThrow();
 const history=progressFixture();history.state='recovery_required';history.source={kind:'recovery_snapshot',commit:'a'.repeat(40),reference:'refs/towermd/snapshot'};history.floor_seq=null;expect(()=>decodeTaskProgress(history)).toThrow(/历史快照/);
});
test('中断先于提案，提案按影响任务数排序；256事件窗口不制造关注项',()=>{
 const view=progressFixture([proposalFixture('small',1),proposalFixture('large',3)]);view.tasks=[{type:'Task',id:'blocked',state:'blocked',module:'red'}];view.holds=[{type:'Hold',id:'gate',kind:'gatefailed',gate:'G3',task:'task',opened_ms:10}];expect(attentionItems(view).map(item=>item.id)).toEqual(['gate','large','small','module:red']);
 view.proposals=[];view.events=[{seq:5,eid:'unqualified',obj:'tail-only-proposal',type:'proposal.created',t:'2026-10-19T00:00:00Z',data:{reason:'不能只靠事件造卡'}}];expect(attentionItems(view).some(item=>item.kind==='proposal')).toBe(false);
});
test('已点击5字段在第二次完整review读取发生变化时停止，不自动prepare新决定',async()=>{
 const expected=proposalFixture().binding;const actual=proposalReview();const client:ReviewClient={load:vi.fn(async()=>actual),pending:vi.fn(async()=>[]),prepare:vi.fn(),submit:vi.fn()};const bound=bindProposalReview(expected,client);expect(await bound.load('selected',expected.object_id)).toBe(actual);
 for(const key of ['project_id','object_id','content_hash','target_baseline','draft_revision'] as const){const wrong=proposalReview();wrong.binding[key]=key==='target_baseline'?'towermd/B2':'different';vi.mocked(client.load).mockResolvedValueOnce(wrong);await expect(bound.load('selected',expected.object_id)).rejects.toThrow(/已变化/);}
 expect(client.prepare).not.toHaveBeenCalled();expect(client.submit).not.toHaveBeenCalled();
});
