<script lang="ts">
 import {onDestroy} from 'svelte';
 import {failureMessage} from '../lib/draft';
 import {taskReviewClient,validTaskReviewHold,decodeTaskReviewView,decodePreparedTaskReview,decodeTaskReviewPending,decodeTaskReviewOutcome,sameTaskReviewBinding,taskReviewVerdictLabel,terminalTaskReview,type TaskReviewClient,type TaskReviewView,type PreparedTaskReview,type TaskReviewResponse,type TaskReviewVerdict} from '../lib/task-review';
 let {token,client=taskReviewClient}:{token:string;client?:TaskReviewClient}=$props();
 let entry=$state('');let view=$state<TaskReviewView|null>(null);let pending=$state<PreparedTaskReview[]>([]);let history=$state<PreparedTaskReview[]>([]);
 let result=$state<TaskReviewResponse|null>(null);let busy=$state(false);let error=$state('');let invalid=$state(false);let queueKnown=$state(false);
 let verdict=$state<TaskReviewVerdict|''>('');let comment=$state('');let generation=0;let seenToken:string|undefined;
 const canAct=$derived(view!==null&&view.current_validity.state==='current'&&!terminalTaskReview(view)&&queueKnown&&pending.length===0&&!invalid&&result===null);
 function reset(){generation++;view=null;pending=[];history=[];result=null;busy=false;error='';invalid=false;queueKnown=false;verdict='';comment='';}
 $effect(()=>{if(token!==seenToken){seenToken=token;reset();entry='';}});
 onDestroy(()=>{generation++;});
 function changed(value:string){reset();entry=value;}
 const current=(g:number,selected:string,id:string)=>g===generation&&selected===token&&entry.trim()===id;
 async function load(){
  if(busy)return;const id=entry.trim();const selected=token;reset();entry=id;
  if(!validTaskReviewHold(id)){error='请粘贴 CLI 返回的评审 Hold ID，不能填写目录或路径。';return;}
  const g=++generation;busy=true;
  try{
   const originals=decodeTaskReviewPending(await client.pending(selected,id),id);if(!current(g,selected,id))return;pending=originals;queueKnown=true;
   const loaded=decodeTaskReviewView(await client.load(selected,id),id);if(!current(g,selected,id))return;view=loaded;history=loaded.original_requests;
  }catch(failure){if(current(g,selected,id)){error=failureMessage(failure);invalid=true;}}
  finally{if(current(g,selected,id))busy=false;}
 }
 async function send(original:PreparedTaskReview,g:number,selected:string){
  const outcome=decodeTaskReviewOutcome(await client.submit(selected,original.request_id),original.request_id,original);
  if(!current(g,selected,original.hold))return;
  if(outcome.state==='confirmed'&&outcome.response){result=outcome.response;pending=pending.filter(item=>item.request_id!==original.request_id);invalid=true;}
  else{
   error=outcome.error?failureMessage(outcome.error):'原任务评审请求尚未确认，请重试原请求。';
   if(outcome.state==='conflict'||outcome.state==='rejected'){pending=pending.filter(item=>item.request_id!==original.request_id);invalid=true;}
  }
 }
 async function decide(){
  if(busy||!canAct||!view||!verdict)return;const selected=token;const id=view.opening.obj;const ticket=view.review_ticket;const binding=view.binding;const actor=view.human_actor;const chosen=verdict;const text=comment;const g=++generation;busy=true;error='';let original:PreparedTaskReview|null=null;
  try{
   const prepared=decodePreparedTaskReview(await client.prepare(selected,ticket,chosen,text),id);if(!current(g,selected,id))return;
   if(!sameTaskReviewBinding(prepared.binding,binding)||prepared.human_actor!==actor||prepared.verdict!==chosen||prepared.comment!==text)throw new Error('已保存请求与刚审阅的绑定、署名或意见不一致；不会自动提交。');
   // The native queue owns this UUID and immutable body from this point onward.
   original=prepared;pending=[prepared];history=[...history.filter(item=>item.request_id!==prepared.request_id),prepared];
   await send(prepared,g,selected);
  }catch(failure){
   if(current(g,selected,id)){
    error=failureMessage(failure);invalid=true;
    if(!original){
     // Preparation may have saved a request before its response was lost.
     // Read that queue; never prepare another identity to resolve uncertainty.
     queueKnown=false;
     try{const originals=decodeTaskReviewPending(await client.pending(selected,id),id);if(current(g,selected,id)){pending=originals;queueKnown=true;}}
     catch(queueFailure){if(current(g,selected,id))error+=`\n原请求队列尚未核实：${failureMessage(queueFailure)}`;}
    }
   }
  }finally{if(current(g,selected,id))busy=false;}
 }
 async function retry(original:PreparedTaskReview){
  if(busy)return;const selected=token;const g=++generation;busy=true;error='';
  try{await send(original,g,selected);}catch(failure){if(current(g,selected,original.hold))error=failureMessage(failure);}
  finally{if(current(g,selected,original.hold))busy=false;}
 }
</script>

<section class="task-review-card" aria-label="任务人工评审">
 <h2>任务评审</h2>
 <p>在任务工作树中运行 <code>towermd review request --gate G1 --content &lt;计划文件&gt;</code> 或 <code>towermd review request --gate G4</code>，粘贴返回的 Hold ID 后读取。</p>
 <div class="task-review-loader"><label>CLI 返回的评审 Hold ID<input value={entry} oninput={(event)=>changed(event.currentTarget.value)} placeholder="H-…" autocomplete="off" spellcheck={false}/></label><button type="button" class="secondary" disabled={busy} onclick={load}>读取任务评审</button></div>
 {#if busy}<p role="status">正在处理原任务评审请求…</p>{/if}
 {#if error}<p class="draft-diagnostic" role="alert">{error}</p>{/if}
 {#if pending.length}
  <section class="task-review-pending" aria-label="原任务评审请求"><h3>原请求等待确认</h3><p>保留以下原 UUID、意见和署名。重试不会创建另一份评审。</p>
   {#each pending as original}<div><p><strong>{taskReviewVerdictLabel(original.verdict)}请求</strong> · {original.human_actor}</p><code>{original.request_id}</code><p>{original.comment}</p><details><summary>原请求完整绑定</summary><pre>{JSON.stringify(original.binding,null,2)}</pre></details><button type="button" disabled={busy} onclick={()=>retry(original)}>重试原请求</button></div>{/each}
  </section>
 {/if}
 {#if result}
  <section class="task-review-result" aria-label="任务评审提交结果">
   {#if result.current_validity.state==='historical_only'}<h3>原评审回执已确认 · 仅历史效力</h3><p>{result.current_validity.reason}</p>
   {:else}<h3>{taskReviewVerdictLabel(result.receipt.result_ref.verdict)}已记录</h3>{/if}
   <p>原请求：<code>{result.receipt.request_id}</code></p><p>署名：<strong>{result.receipt.result_ref.fact.actor}</strong></p><p>意见：{result.receipt.result_ref.comment}</p><p>实际答复事件：<code>{result.receipt.result_ref.fact.answer_eid}</code></p>
   <details><summary>完整原回执 · 只读</summary><pre>{JSON.stringify(result,null,2)}</pre></details>
  </section>
 {/if}
 {#if view}
  <section aria-label="任务评审原件">
   <p class="task-review-validity">{view.current_validity.state==='current'?'● 当前评审':'◇ 历史评审 · 只读'}</p>
   {#if view.current_validity.state==='historical_only'}<p>{view.current_validity.reason}</p>{/if}
   <p>本机署名身份：<strong>{view.human_actor}</strong></p>
   <dl class="review-binding"><dt>项目</dt><dd><code>{view.project_id}</code></dd><dt>运行 Run</dt><dd><code>{view.binding.run}</code></dd><dt>任务 Task</dt><dd><code>{view.binding.task}</code></dd><dt>基线</dt><dd><code>{view.binding.baseline}</code></dd><dt>原开立事件</dt><dd><code>{view.opening.eid}</code></dd><dt>实际序号</dt><dd><code>{view.opening.seq}</code></dd><dt>来源提交</dt><dd><code>{view.opening.data.hold.body.source_commit??'未提供'}</code></dd><dt>来源文件</dt><dd><code>{view.opening.data.hold.body.source_path??'未提供'}</code></dd></dl>
   <h3>完整租约绑定（fences）</h3><div class="task-review-fences"><table><thead><tr><th>租约</th><th>运行</th><th>epoch</th></tr></thead><tbody>{#each view.binding.fences as fence}<tr><td><code>{fence.name}</code></td><td><code>{fence.holder}</code></td><td><code>{fence.epoch}</code></td></tr>{/each}</tbody></table></div>
   {#if view.binding.subject.kind==='plan'}<h3>G1 · 完整计划原文</h3><p>SHA-256：<code>{view.binding.subject.sha256}</code></p><pre class="task-review-plan" aria-label="完整计划原文">{view.opening.data.hold.body.plan_content}</pre>
   {:else}<h3>G4 · 实际提交 tree</h3><code class="task-review-tree" aria-label="实际评审 tree">{view.binding.subject.tree}</code>{/if}
   {#if terminalTaskReview(view)}<section class="task-review-previous"><h3>此前已记录：{view.current_hold.body.verdict?taskReviewVerdictLabel(view.current_hold.body.verdict):'原人工答复'}</h3>{#if view.current_hold.reply}<p>署名：{view.current_hold.reply.actor}</p><p>{view.current_hold.reply.text}</p>{/if}</section>{/if}
   {#if canAct}<form onsubmit={(event)=>{event.preventDefault();void decide();}}>
    <p>本次结论只对应以上计划或 tree，修改后需要重新评审。</p>
    <fieldset disabled={busy}><legend>本次评审结论</legend><div class="task-review-verdicts"><label><input type="radio" name="task-review-verdict" bind:group={verdict} value="approved"/>批准</label><label><input type="radio" name="task-review-verdict" bind:group={verdict} value="changes_requested"/>需要修改</label><label><input type="radio" name="task-review-verdict" bind:group={verdict} value="rejected"/>拒绝</label></div><label>本次评审意见<textarea bind:value={comment}></textarea></label></fieldset>
    <button type="submit" disabled={busy||!verdict}>保存并提交本次评审</button>
   </form>{/if}
   {#if invalid&&!result&&!pending.length}<p role="status">请重新读取原件再作决定；不会自动创建或提交另一份评审。</p>{/if}
   <details><summary>完整原件与 Hold 记录 · 只读</summary><pre>{JSON.stringify({opening:view.opening,current_hold:view.current_hold},null,2)}</pre></details>
  </section>
 {/if}
 {#if history.length}<details><summary>本机保存的原评审请求（{history.length}）</summary><p>本机请求记录不代表有效批准。</p>{#each history as original}<div class="task-review-history"><code>{original.request_id}</code><p>{taskReviewVerdictLabel(original.verdict)}请求 · {original.human_actor}</p><p>{original.comment}</p><pre>{JSON.stringify(original.binding,null,2)}</pre></div>{/each}</details>{/if}
</section>
