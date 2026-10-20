import { expect, test } from 'vitest';
import { savedFixture } from './draft.fixture';
import {serialized,publicationFixture} from './review-publication.fixture';
import { decodeReviewView, decodePreparedReview, decodeReviewOutcome, type ReviewView, type PreparedReview, type ReviewOutcome } from './review';
function fixture() {
 const saved=savedFixture();saved.draft.bundle.target_baseline='towermd/B1';
 const carrier={mode:'100644',sha256:'e'.repeat(64),text:'---\ntype: Proposal\nid: proposal\n---\n完整 input、impact、run 与 fences 原文\n',future:{keep:[2,1]}};
 saved.draft.files={'docs/tower/proposals/proposal.md':carrier};
 const binding={project_id:saved.project_id,object_id:saved.id,content_hash:saved.draft.content_hash,target_baseline:'towermd/B1',draft_revision:saved.revision};
 const original:PreparedReview={request_id:'01900000-0000-7000-8000-000000000001',binding,verdict:'approved',comment:'完整审阅',human_actor:'human:owner'};
 const view:ReviewView={binding,review_ticket:'01900000-0000-7000-8000-000000000002',human_actor:'human:owner',current_baseline:'towermd/B0',main_revision:'d'.repeat(40),draft:saved.draft,diff:[{path:'docs/tower/proposals/proposal.md',before:null,after:carrier.text}],interface_modules:[],publish_blockers:[]};
 const outcome:ReviewOutcome={state:'confirmed',request_id:original.request_id,error:null,response:{receipt:{request_id:original.request_id,body_hash:'f'.repeat(64),result_ref:{approval_eid:'actual-approval-eid',binding,actor:'human:owner',verdict:'approved',comment:original.comment}},current_validity:{state:'current'},publication:{state:'recorded',created:publicationFixture(original,'actual-approval-eid')}}};
 return {view,original,outcome,carrier};
}
test('Bn沿完整五字段binding与原FrozenDraft，ProposalCarrier和扩展原样保留',()=>{
 const {view,original,carrier}=fixture();
 expect(decodeReviewView(view,'draft-1')).toBe(view);
 expect(view.draft.files).toEqual({'docs/tower/proposals/proposal.md':carrier});
 expect(decodePreparedReview(original,'draft-1')).toBe(original);
});
test('禁止错ID、被替换hash/目标与未声明的新授权绑定字段',()=>{
 const {view}=fixture();
 for(const binding of [{...view.binding,object_id:'other'},{...view.binding,content_hash:'b'.repeat(64)},{...view.binding,target_baseline:'towermd/B0'},{...view.binding,subject_hash:'a'.repeat(64)}])expect(()=>decodeReviewView({...view,binding},'draft-1')).toThrow();
 expect(()=>decodeReviewView({...view,draft:{...view.draft,files:{bad:{text:'只有摘要'}}}},'draft-1')).toThrow();
});
test('画布main/baseline与审阅原件不一致或条目缺raw时拒绝显示可审批视图',()=>{
 const {view}=fixture();const saved=savedFixture();const source={canvas_view:saved.canvas_view,object_views:saved.object_views};
 const comparison={state:'available',main_revision:'e'.repeat(40),baseline:'towermd/B0',baseline_canvas:source,draft_canvas:source,diagnostics:[]};
 expect(()=>decodeReviewView({...view,canvas_comparison:comparison},'draft-1')).toThrow();
 expect(()=>decodeReviewView({...view,canvas_comparison:{...comparison,main_revision:view.main_revision,draft_canvas:{...source,canvas_view:{...source.canvas_view,nodes:[null]}}}},'draft-1')).toThrow();
});
test('原回执逐一比较所有binding字段、人工身份、原意见和verdict',()=>{
 const {outcome,original}=fixture();expect(decodeReviewOutcome(outcome,original)).toBe(outcome);
 for(const key of Object.keys(original.binding) as (keyof typeof original.binding)[]){const binding={...original.binding,[key]:key==='target_baseline'?'towermd/B2':key==='content_hash'?'b'.repeat(64):key==='draft_revision'?'e'.repeat(40):'other'};expect(()=>decodeReviewOutcome({...outcome,response:{...outcome.response!,receipt:{...outcome.response!.receipt,result_ref:{...outcome.response!.receipt.result_ref,binding}}}},original)).toThrow();}
 for(const changed of [{actor:'human:other'},{comment:'另一条意见'},{verdict:'rejected'}])expect(()=>decodeReviewOutcome({...outcome,response:{...outcome.response!,receipt:{...outcome.response!.receipt,result_ref:{...outcome.response!.receipt.result_ref,...changed}}}},original)).toThrow();
});
test('有批准回执仍不能以缺提交或错误目标声称出版完成，uncertain不携成功回执',()=>{
 const {outcome,original}=fixture();
 const created=outcome.response!.publication!.created!;
 for(const publication of [{state:'recorded'}, {state:'recorded',target:'towermd/B1',commit:'b'.repeat(40)},...[
  {commit:undefined},{commit:null},{target:undefined},{target:'towermd/B2'},{approval_eid:'another-approval'},{publication_id:'another-publication'},{content_hash:'d'.repeat(64)},
  {p3_receipt:undefined},
  {p3_receipt:{...created.p3_receipt,request_id:'another-receipt'}},
  {p3_receipt:{...created.p3_receipt,result_ref:{...created.p3_receipt.result_ref,target:'towermd/B2'}}},
 ].map(changed=>({state:'recorded',created:{...created,...changed}}))])expect(()=>decodeReviewOutcome({...outcome,response:{...outcome.response!,publication}},original)).toThrow();
 expect(()=>decodeReviewOutcome({...outcome,state:'uncertain'},original)).toThrow();
});

test('actual serialized BaselineCreated flattens result inside recorded.created and preserves its durable receipt',()=>{
 const approval=serialized.approval;
 const original:PreparedReview={request_id:approval.p3_request.request_id,binding:approval.approval.binding,verdict:'approved',comment:approval.approval.comment,human_actor:approval.approval.actor};
 const outcome={state:'confirmed',request_id:original.request_id,error:null,response:{receipt:approval.p3_receipt,current_validity:{state:'current'},publication:{state:'recorded',created:serialized.created}}};
 expect(decodeReviewOutcome(outcome,original)).toBe(outcome);
 expect(outcome.response.publication.created).not.toHaveProperty('result');
 expect(outcome.response.publication.created.commit).toBe('c46104384cec8aa4c78446b50b4250b2d966db14');
 expect(outcome.response.publication.created.p3_receipt.result_ref.commit).toBe(outcome.response.publication.created.commit);
});
