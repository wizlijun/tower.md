<script lang="ts">
 import {onDestroy,untrack} from 'svelte';
 import {failureMessage} from '../lib/draft';
 import {decodeTaskRecheckView,decodePreparedTaskRecheck,decodeTaskRecheckPending,decodeTaskRecheckOutcome,sameTaskRecheckSubject,type TaskRecheckClient,type TaskRecheckView,type PreparedTaskRecheck,type TaskRecheckResponse} from '../lib/task-recheck';
 let {token,taskId,client,onresolved}:{token:string;taskId:string;client:TaskRecheckClient;onresolved?:()=>void}=$props();
 let view=$state<TaskRecheckView|null>(null);let pending=$state<PreparedTaskRecheck[]>([]);let history=$state<PreparedTaskRecheck[]>([]);let result=$state<TaskRecheckResponse|null>(null);
 let busy=$state(false);let error=$state('');let invalid=$state(false);let queueKnown=$state(false);let comment=$state('');let generation=0;const blocked=new Set<string>();
 let seenToken='';let seenTask='';
 const canAct=$derived(view!==null&&view.binding.publication_ids.length>0&&queueKnown&&!pending.length&&!invalid&&result===null);
 const current=(g:number,selected:string,id:string)=>g===generation&&selected===token&&id===taskId;
 function reset(){generation++;view=null;pending=[];history=[];result=null;busy=false;error='';invalid=false;queueKnown=false;comment='';}
 $effect(()=>{const selected=token;const id=taskId;if(selected!==seenToken||id!==seenTask){seenToken=selected;seenTask=id;untrack(()=>{reset();blocked.clear();void load();});}});
 onDestroy(()=>{generation++;});
 async function load(){
  if(busy)return;reset();const selected=token,id=taskId,g=++generation;busy=true;
  try{
   const originals=decodeTaskRecheckPending(await client.pending(selected,id),id);if(!current(g,selected,id))return;pending=originals;queueKnown=true;
   const loaded=decodeTaskRecheckView(await client.load(selected,id),id);if(!current(g,selected,id))return;view=loaded;history=loaded.original_requests;
  }catch(failure){if(current(g,selected,id)){error=failureMessage(failure);invalid=true;}}
  finally{if(current(g,selected,id))busy=false;}
 }
 async function send(original:PreparedTaskRecheck,g:number,selected:string,id:string){
  const outcome=decodeTaskRecheckOutcome(await client.submit(selected,original.request_id),original.request_id,original);if(!current(g,selected,id))return;
  if(outcome.state==='confirmed'&&outcome.response){result=outcome.response;pending=pending.filter(item=>item.request_id!==original.request_id);invalid=true;onresolved?.();}
  else{error=outcome.error?failureMessage(outcome.error):'原复核请求尚未确认，请重试原 UUID。';if(outcome.state==='conflict'||outcome.state==='rejected'){pending=pending.filter(item=>item.request_id!==original.request_id);invalid=true;}}
 }
 async function resolve(){
  if(busy||!canAct||!view)return;const selected=token,id=taskId,displayed=view,text=comment,g=++generation;busy=true;error='';let original:PreparedTaskRecheck|null=null;
  try{
   const prepared=decodePreparedTaskRecheck(await client.prepare(selected,displayed.recheck_ticket,text),id);if(!current(g,selected,id))return;
   if(!sameTaskRecheckSubject(prepared.subject,displayed)||prepared.human_actor!==displayed.human_actor||prepared.comment!==text){blocked.add(prepared.request_id);throw new Error('已保存请求与所读原件、署名或意见不同；不会提交，请重新核对原请求。');}
   original=prepared;pending=[prepared];history=[...history.filter(item=>item.request_id!==prepared.request_id),prepared];await send(prepared,g,selected,id);
  }catch(failure){if(current(g,selected,id)){error=failureMessage(failure);invalid=true;if(!original){queueKnown=false;try{const originals=decodeTaskRecheckPending(await client.pending(selected,id),id);if(current(g,selected,id)){pending=originals;queueKnown=true;}}catch(queueFailure){if(current(g,selected,id))error+=`\n原请求队列未核实：${failureMessage(queueFailure)}`;}}}}
  finally{if(current(g,selected,id))busy=false;}
 }
 async function retry(original:PreparedTaskRecheck){if(busy||blocked.has(original.request_id))return;const selected=token,id=taskId,g=++generation;busy=true;error='';try{await send(original,g,selected,id);}catch(failure){if(current(g,selected,id))error=failureMessage(failure);}finally{if(current(g,selected,id))busy=false;}}
</script>
<section class="task-review-card" aria-label="Task 需复核原件">
 <h3>Task 需复核</h3><p>核对新基线对当前任务的影响。解除只绑定以下 Task 文本、完整 publication 集合和当前 Run/租约，不会批准其他评审或启动运行。</p>
 <button type="button" class="secondary" disabled={busy} onclick={load}>重新读取复核原件</button>
 {#if busy}<p role="status">正在核对原复核请求…</p>{/if}
 {#if error}<p class="draft-diagnostic" role="alert">{error}</p>{/if}
 {#if pending.length}<section aria-label="原复核请求"><h4>原请求等待确认</h4><p>重试保留原 UUID 和完整绑定。</p>{#each pending as original}<div class="live-record"><code>{original.request_id}</code><p>{original.human_actor} · {original.comment}</p><pre>{JSON.stringify(original.binding,null,2)}</pre><h4>该原请求核对的 Task 完整文本与原因</h4><pre>{original.subject.task_text}</pre><pre>{JSON.stringify(original.subject.reasons,null,2)}</pre><p>正文 SHA-256：<code>{original.body_hash}</code></p><button type="button" disabled={busy||blocked.has(original.request_id)} onclick={()=>retry(original)}>重试原复核请求</button>{#if blocked.has(original.request_id)}<p>此请求与已显示原件不同，已阻止提交。</p>{/if}</div>{/each}</section>{/if}
 {#if result}<section aria-label="复核原回执"><h4>{result.current_validity.state==='current'?'本次复核已记录 · 当前绑定有效':'原复核回执已确认 · 仅历史效力'}</h4>{#if result.current_validity.state==='historical_only'}<p>{result.current_validity.reason}</p>{/if}<p>任务角标以重新读取的真实状态为准。</p><p>原 UUID：<code>{result.receipt.request_id}</code></p><p>实际事件：<code>{result.receipt.result_ref.resolved_eid}</code></p><details><summary>完整原回执</summary><pre>{JSON.stringify(result,null,2)}</pre></details></section>{/if}
 {#if view}<section aria-label="完整 Task 复核绑定"><p>项目：<code>{view.project_id}</code> · 人工署名：<strong>{view.human_actor}</strong></p><p>Task：<code>{view.binding.task_id}</code> · 目标基线：<code>{view.binding.target_baseline}</code></p><p>Task SHA-256：<code>{view.binding.task_content_hash}</code></p>
  <h4>当前 Task 完整文本</h4><pre aria-label="当前 Task 完整文本">{view.task_text}</pre>
  <h4>完整 publication 原因（{view.binding.publication_ids.length}）</h4>{#each view.reasons as fact}<div class="live-record"><code>{fact.publication_id}</code><p>{fact.old_baseline} → {fact.new_baseline}</p><ul>{#each fact.reasons as reason}<li>{reason}</li>{/each}</ul></div>{:else}<p>当前已无待复核 publication；不会新建解除请求。</p>{/each}
  <h4>运行与完整租约绑定</h4>{#if view.binding.running}<p>Run：<code>{view.binding.running.run}</code></p><div class="task-review-fences"><table><thead><tr><th>租约</th><th>holder</th><th>epoch</th><th>到期时间（原始毫秒）</th></tr></thead><tbody>{#each view.binding.running.fences as fence}<tr><td><code>{fence.name}</code></td><td><code>{fence.holder}</code></td><td>{fence.epoch}</td><td>{view.binding.running.lease_expires_at[fence.name]}</td></tr>{/each}</tbody></table></div>{:else}<p>当前 Task 为 ready，无运行租约绑定。</p>{/if}
  {#if canAct}<form onsubmit={(event)=>{event.preventDefault();void resolve();}}><label>本次复核意见<textarea bind:value={comment} disabled={busy}></textarea></label><p>明确解除上述完整 publication 集合的需复核标记。文本、基线或任何租约值变化均需重新核对。</p><button type="submit" disabled={busy}>确认已复核并解除本次标记</button></form>{/if}
  <details><summary>完整已读原件</summary><pre>{JSON.stringify({binding:view.binding,task_text:view.task_text,reasons:view.reasons},null,2)}</pre></details>
 </section>{/if}
 {#if history.length}<details><summary>本机原复核请求（{history.length}）</summary><p>本机保存不代表已解除。</p>{#each history as original}<pre>{JSON.stringify(original,null,2)}</pre>{#if !pending.some(entry=>entry.request_id===original.request_id)}<button type="button" disabled={busy||blocked.has(original.request_id)} onclick={()=>retry(original)}>读取原复核回执</button>{/if}{/each}</details>{/if}
</section>
