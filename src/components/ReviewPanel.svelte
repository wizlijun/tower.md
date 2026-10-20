<script lang="ts">
 import {onMount,onDestroy} from 'svelte';
 import ReviewCanvas from './ReviewCanvas.svelte';
 import {reviewClient,publicationLabel,decodePreparedReview,decodeReviewPending,decodeReviewView,decodeReviewOutcome,sameReviewBinding,type ReviewClient,type ReviewView,type PreparedReview,type ReviewResponse,type Verdict} from '../lib/review';
 import {failureMessage} from '../lib/draft';
 let {token,id,client=reviewClient,onclose}:{token:string;id:string;client?:ReviewClient;onclose:()=>void}=$props();
 let view=$state<ReviewView|null>(null);let busy=$state(false);let error=$state('');let comment=$state('');let pending=$state<PreparedReview[]>([]);let blockedPrepared=$state<string[]>([]);let result=$state<ReviewResponse|null>(null);let lastOriginal=$state<PreparedReview|null>(null);let invalid=$state(false);let queueKnown=$state(false);let generation=0;
 onDestroy(()=>{generation++;});onMount(()=>{void load();});
 async function load(){if(busy)return;const g=++generation;busy=true;error='';invalid=false;queueKnown=false;view=null;
  try{const originals=decodeReviewPending(await client.pending(token,id),id);if(g!==generation)return;pending=originals;queueKnown=true;const value=decodeReviewView(await client.load(token,id),id);if(g!==generation)return;view=value;}
  catch(failure){if(g===generation)error=failureMessage(failure);}finally{if(g===generation)busy=false;}
 }
 async function send(original:PreparedReview,g:number){
  lastOriginal=original;const outcome=decodeReviewOutcome(await client.submit(token,original.request_id),original);if(g!==generation)return;
  if(outcome.state==='confirmed'&&outcome.response){result=outcome.response;const originals=decodeReviewPending(await client.pending(token,id),id);if(g!==generation)return;pending=originals;invalid=true;}
  else{error=outcome.error?failureMessage(outcome.error):'原审阅请求尚未确认。';if(outcome.state==='conflict'||outcome.state==='rejected'){invalid=true;const originals=decodeReviewPending(await client.pending(token,id),id);if(g!==generation)return;pending=originals;}}
 }
 async function decide(verdict:Verdict){if(busy||!view||invalid||!queueKnown||pending.length||result)return;const subject=view;const originalComment=comment;const g=++generation;busy=true;error='';
  try{const original=decodePreparedReview(await client.prepare(token,subject.review_ticket,verdict,originalComment),id);if(g!==generation)return;pending=[original];if(!sameReviewBinding(original.binding,subject.binding)||original.human_actor!==subject.human_actor||original.verdict!==verdict||original.comment!==originalComment){invalid=true;blockedPrepared=[...blockedPrepared,original.request_id];throw new Error('已保存原决定与刚审阅的完整版本、身份或意见不同；不会提交。');}await send(original,g);}
  catch(failure){if(g===generation){error=failureMessage(failure);queueKnown=false;if(typeof failure==='object'&&failure&&'code' in failure&&failure.code==='request_conflict')invalid=true;}}
  finally{if(g===generation)busy=false;}
 }
 async function retry(original:PreparedReview){if(busy||blockedPrepared.includes(original.request_id))return;const g=++generation;busy=true;error='';try{await send(original,g);}catch(failure){if(g===generation)error=failureMessage(failure);}finally{if(g===generation)busy=false;}}
</script>
<section class="review-panel" aria-label="基线人工审阅">
 <div class="review-heading"><h2>审阅基线草稿</h2><button type="button" class="secondary" disabled={busy} onclick={onclose}>返回草稿</button><button type="button" class="secondary" disabled={busy} onclick={()=>{result=null;void load();}}>重新加载审阅</button></div>
 <p>人工决定绑定下面的完整版本。批准后仍需单独确认出版结果，才代表基线已经生效。</p>
 {#if busy}<p role="status">正在处理原审阅请求…</p>{/if}
 {#if error}<p class="draft-diagnostic" role="alert">{error}</p>{/if}
 {#if pending.length}<section class="draft-pending" aria-label="原审阅请求等待确认"><p>尚未确认的原请求保留原 UUID、身份和 binding；不会用当前草稿替换。</p>{#each pending as original}<div><strong>{original.verdict==='approved'?'批准':'拒绝'} · {original.human_actor}</strong><code>{original.request_id}</code><pre>{JSON.stringify(original.binding,null,2)}</pre><p>{original.comment}</p><button type="button" disabled={busy||blockedPrepared.includes(original.request_id)} onclick={()=>retry(original)}>重试原审阅请求</button></div>{/each}</section>{/if}
 {#if result}<section class="review-result" aria-label="人工决定与出版结果"><h3>{result.receipt.result_ref.verdict==='approved'?'人工批准已记录':'人工拒绝已记录'}</h3><p>署名：{result.receipt.result_ref.actor}</p><p>回执：{result.receipt.request_id}</p><pre>{JSON.stringify(result.receipt.result_ref.binding,null,2)}</pre>
 {#if result.current_validity.state==='historical_only'}<p>原回执只有历史效力：{result.current_validity.reason}</p>{/if}
 {#if result.receipt.result_ref.verdict==='approved'}<p>{publicationLabel(result.publication)}</p>{#if result.publication?.state==='recorded'&&result.publication.created}{@const created=result.publication.created}<section aria-label="基线出版回执"><dl><dt>已出版基线</dt><dd>{created.target}</dd><dt>真实提交</dt><dd><code>{created.commit}</code></dd><dt>出版回执</dt><dd><code>{created.p3_receipt.request_id}</code></dd></dl></section>{/if}{#if result.publication?.reason}<pre>{result.publication.reason}</pre>{/if}<pre>{JSON.stringify(result.publication??null,null,2)}</pre>{#if lastOriginal}<button type="button" disabled={busy} onclick={()=>lastOriginal&&retry(lastOriginal)}>重读原批准与出版状态</button>{/if}{/if}
 </section>{/if}
 {#if view}
 <p><strong>本机身份：{view.human_actor}</strong></p>
 <dl class="review-binding">{#each Object.entries(view.binding) as [key,value]}<dt>{key}</dt><dd><code>{value}</code></dd>{/each}<dt>当前基线</dt><dd>{view.current_baseline??'尚无已认证基线'}</dd></dl>
 {#if view.publish_blockers.length}<section class="draft-diagnostic"><h3>出版阻碍</h3>{#each view.publish_blockers as reason}<p>{reason}</p>{/each}</section>{/if}
 <ReviewCanvas comparison={view.canvas_comparison} interfaceModules={view.interface_modules}/>
 <section><h3>逐文件原文与变化</h3>{#each view.diff as diff}<details open><summary>{diff.path} · {diff.before===null?'新增':diff.after===null?'删除':diff.before===diff.after?'保留':'修改'}</summary><div class="review-diff"><div><h4>原文</h4><pre>{diff.before??'（源中不存在）'}</pre></div><div><h4>候选原文</h4><pre>{diff.after??'（删除）'}</pre></div></div></details>{/each}</section>
 <section><h3>验证与证据</h3>{#if view.verification?.state==='available'}<p>固定扫描：{view.verification.scan_id} · 报告：{view.verification.report_eid}</p><pre>{JSON.stringify(view.verification.report,null,2)}</pre>{#each view.verification.evidence as evidence}<details open><summary>模块 Evidence 原文</summary><pre>{JSON.stringify(evidence,null,2)}</pre></details>{/each}{:else}<p>验证证据不可用：{view.verification?.state==='unavailable'?view.verification.reason:'服务端尚未返回验证与证据正文；不能推断已通过或无测试。'}</p>{/if}</section>
 <section><h3>问题、风险与 Interface 影响</h3><pre>{JSON.stringify({questions:view.draft.bundle.questions,risks:view.draft.bundle.risks,interface_modules:view.interface_modules},null,2)}</pre></section>
 {#if view.original_requests?.length}<details><summary>本机原审阅请求记录</summary><p>本机记录不是批准证明。点击后由服务端原回执核实，不替换旧 binding。</p>{#each view.original_requests as original}<div><code>{original.request_id}</code><span>{original.verdict==='approved'?'批准请求':'拒绝请求'}</span><button type="button" disabled={busy||blockedPrepared.includes(original.request_id)} onclick={()=>retry(original)}>读取或协调原审阅结果</button></div>{/each}</details>{/if}
 <details><summary>完整审阅响应 · 只读</summary><pre>{JSON.stringify(view,null,2)}</pre></details>
 <label>审阅意见<textarea bind:value={comment} disabled={busy||pending.length>0||result!==null}></textarea></label>
 {#if invalid&&!result}<p role="status">此 binding 已失效或请求被拒绝。请重新加载并审阅；不会自动重放决定。</p>{/if}
 <div class="review-actions"><button type="button" disabled={busy||invalid||!queueKnown||pending.length>0||result!==null||view.publish_blockers.length>0} onclick={()=>decide('approved')}>批准此版本并出版</button><button type="button" class="secondary" disabled={busy||invalid||!queueKnown||pending.length>0||result!==null} onclick={()=>decide('rejected')}>拒绝此版本</button></div>
 {/if}
</section>
