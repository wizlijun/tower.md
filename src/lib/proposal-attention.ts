import {reviewClient,type ReviewBinding,type ReviewClient} from './review';
export interface ProposalImpact {intents:string[];modules:string[];interfaces:string[];acceptances:string[];tasks:string[];paths:string[]}
export interface ProgressProposal {state:'review_pending';binding:ReviewBinding;created_seq:number;created_at_ms:number;reason:string;recommendation:string;impact:ProposalImpact}
export interface ProposalReviewIntent {key:number;binding:ReviewBinding}
const record=(value:unknown):value is Record<string,unknown>=>value!==null&&typeof value==='object'&&!Array.isArray(value);
const exact=(value:Record<string,unknown>,keys:string[])=>Object.keys(value).length===keys.length&&keys.every(key=>Object.hasOwn(value,key));
const text=(value:unknown):value is string=>typeof value==='string'&&value.trim().length>0;
const integer=(value:unknown):value is number=>typeof value==='number'&&Number.isSafeInteger(value)&&value>=0;
const hash=(value:unknown)=>typeof value==='string'&&/^[a-f0-9]{64}$/.test(value);
const oid=(value:unknown)=>typeof value==='string'&&/^([a-f0-9]{40}|[a-f0-9]{64})$/.test(value);
const ids=(value:unknown):value is string[]=>Array.isArray(value)&&value.every(text)&&new Set(value).size===value.length;
function binding(value:unknown):value is ReviewBinding {
 return record(value)&&exact(value,['project_id','object_id','content_hash','target_baseline','draft_revision'])&&text(value.project_id)&&typeof value.object_id==='string'&&value.object_id.length<=160&&/^[A-Za-z0-9_.-]+$/.test(value.object_id)&&!['.','..'].includes(value.object_id)&&hash(value.content_hash)&&typeof value.target_baseline==='string'&&/^towermd\/B[1-9]\d*$/.test(value.target_baseline)&&oid(value.draft_revision);
}
export function sameProposalBinding(a:ReviewBinding,b:ReviewBinding):boolean {
 return a.project_id===b.project_id&&a.object_id===b.object_id&&a.content_hash===b.content_hash&&a.target_baseline===b.target_baseline&&a.draft_revision===b.draft_revision;
}
export function decodeProposalAttention(value:unknown,project:string,through:number):ProgressProposal[] {
 if(!Array.isArray(value))throw new Error('未决提案缺少完整只读枚举。');
 const results=value.map(item=>{
  if(!record(item)||item.state!=='review_pending'||!binding(item.binding)||item.binding.project_id!==project||!integer(item.created_seq)||item.created_seq===0||item.created_seq>through||!integer(item.created_at_ms)||!text(item.reason)||!text(item.recommendation)||!record(item.impact)||!exact(item.impact,['intents','modules','interfaces','acceptances','tasks','paths'])||!Object.values(item.impact).every(ids))throw new Error('提案关注缺少真实项目、完整绑定、影响或当前未决资格。');
  return item as unknown as ProgressProposal;
 });
 if(new Set(results.map(item=>item.binding.object_id)).size!==results.length)throw new Error('提案关注存在重复对象。');
 return results;
}
/** Keep the existing native reader; a changed subject cannot silently replace
 * the five-field binding clicked in the attention list. */
export function bindProposalReview(expected:ReviewBinding,client:ReviewClient=reviewClient):ReviewClient {
 return {
  load:async(token,id)=>{const value=await client.load(token,id);if(!sameProposalBinding(value.binding,expected))throw new Error('提案的项目、对象、内容、目标基线或草稿版本已变化；请回到实时态势重新核对。不会替换已点击的原件。');return value;},
  prepare:(token,ticket,verdict,comment)=>client.prepare(token,ticket,verdict,comment),
  pending:(token,id)=>client.pending(token,id),submit:(token,id)=>client.submit(token,id),
 };
}
