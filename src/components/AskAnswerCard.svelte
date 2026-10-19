<script lang="ts">
 import {onDestroy,untrack} from 'svelte';
 import {failureMessage} from '../lib/draft';
 import {askAnswerClient,validAskHoldId,decodeAskPending,decodeAskAnswerView,decodePreparedAskAnswer,decodeAskAnswerOutcome,sameAskBinding,type AskAnswerClient,type AskAnswerView,type PreparedAskAnswer,type AskAnswerResponse} from '../lib/ask-answer';
 let {token,client=askAnswerClient,holdId='',onresolved}:{token:string;client?:AskAnswerClient;holdId?:string;onresolved?:()=>void}=$props();
 let entry=$state('');let view=$state<AskAnswerView|null>(null);let pending=$state<PreparedAskAnswer[]>([]);let history=$state<PreparedAskAnswer[]>([]);
 let text=$state('');let result=$state<AskAnswerResponse|null>(null);let busy=$state(false);let error=$state('');let invalid=$state(false);let queueKnown=$state(false);
 let generation=0;let seenToken:string|undefined;
 const canAnswer=$derived(view!==null&&view.current_validity.state==='current'&&view.actions.includes('answer')&&view.current_hold.reply===null&&queueKnown&&pending.length===0&&!invalid&&result===null);
 const allExpired=$derived(view!==null&&view.recipients.length>0&&view.recipients.every(r=>!r.actionable));
 function reset(){generation++;view=null;pending=[];history=[];text='';result=null;busy=false;error='';invalid=false;queueKnown=false;}
 let seenHold:string|undefined;
 $effect(()=>{const selected=token;const id=holdId;if(selected!==seenToken||id!==seenHold){seenToken=selected;seenHold=id;untrack(()=>{reset();entry=id;if(id)void load();});}});
 onDestroy(()=>{generation++;});
 const current=(g:number,selected:string,id:string)=>g===generation&&selected===token&&entry.trim()===id;
 function changed(value:string){reset();entry=value;}
 async function load(){
  if(busy)return;const id=entry.trim();const selected=token;reset();entry=id;
  if(!validAskHoldId(id)){error='请粘贴 CLI 返回的问答 Hold ID。';return;}
  const g=++generation;busy=true;
  try{
   const originals=decodeAskPending(await client.pending(selected,id),id);if(!current(g,selected,id))return;pending=originals;queueKnown=true;
   const loaded=decodeAskAnswerView(await client.load(selected,id),id);if(!current(g,selected,id))return;view=loaded;history=loaded.original_requests;
  }catch(failure){if(current(g,selected,id)){error=failureMessage(failure);invalid=true;}}
  finally{if(current(g,selected,id))busy=false;}
 }
 async function send(original:PreparedAskAnswer,g:number,selected:string){
  const outcome=decodeAskAnswerOutcome(await client.submit(selected,original.request_id),original.request_id,original);
  if(!current(g,selected,original.hold))return;
  if(outcome.state==='confirmed'&&outcome.response){result=outcome.response;pending=pending.filter(p=>p.request_id!==original.request_id);invalid=true;onresolved?.();}
  else{
   error=outcome.error?failureMessage(outcome.error):'回答尚未确认，请重试原请求。';
   if(outcome.state==='conflict'||outcome.state==='rejected'){pending=pending.filter(p=>p.request_id!==original.request_id);invalid=true;}
  }
 }
 async function answer(){
  if(busy||!canAnswer||!view||!text.trim())return;
  const selected=token;const id=view.opening.obj;const ticket=view.answer_ticket;const binding=view.binding;const actor=view.human_actor;const answerText=text;const g=++generation;busy=true;error='';let original:PreparedAskAnswer|null=null;
  try{
   original=decodePreparedAskAnswer(await client.prepare(selected,ticket,answerText),id);if(!current(g,selected,id))return;
   if(!sameAskBinding(original.binding,binding)||original.human_actor!==actor||original.text!==answerText)throw new Error('已保存请求与刚读取的原件、回答或署名不同，不会自动提交。');
   pending=[original];await send(original,g,selected);
  }catch(failure){
   if(current(g,selected,id)){
    error=failureMessage(failure);invalid=true;
    try{const originals=decodeAskPending(await client.pending(selected,id),id);if(current(g,selected,id)){pending=originals.length?originals:original?[original]:[];queueKnown=true;}}
    catch{if(current(g,selected,id))queueKnown=false;}
   }
  }finally{if(current(g,selected,id))busy=false;}
 }
 async function retry(original:PreparedAskAnswer){
  if(busy)return;const selected=token;const g=++generation;busy=true;error='';
  try{await send(original,g,selected);}catch(failure){if(current(g,selected,original.hold))error=failureMessage(failure);}
  finally{if(current(g,selected,original.hold))busy=false;}
 }
</script>

<section class="ask-answer-card" aria-label="本次问答">
 <h2>本次问答</h2>
 {#if !holdId}
 <p>在任务工作树运行 <code>towermd ask &lt;问题&gt;</code>，粘贴返回的 Hold ID 后读取。</p>
 <div class="ask-answer-loader"><label>CLI 返回的问答 Hold ID<input value={entry} oninput={(event)=>changed(event.currentTarget.value)} placeholder="H-…" autocomplete="off" spellcheck={false}/></label><button type="button" class="secondary" disabled={busy} onclick={load}>读取问答</button></div>
 {:else}<p>从画布打开的真实提问。请阅读问题后明确提交回答。</p>{/if}
 {#if holdId}<button type="button" class="secondary" disabled={busy} onclick={load}>重新读取原件</button>{/if}
 {#if busy}<p role="status">正在处理本次问答…</p>{/if}
 {#if error}<p class="draft-diagnostic" role="alert">{error}</p>{/if}
 {#if pending.length}
  <section class="ask-answer-pending" aria-label="原回答等待确认"><h3>原回答等待确认</h3><p>保留原回答与署名，重试不会创建另一份回答。</p>
   {#each pending.slice(0,3) as original}<div><p>{original.text}</p><p>署名：{original.human_actor}</p><button type="button" disabled={busy} onclick={()=>retry(original)}>重试原请求</button><details><summary>原请求诊断</summary><code>{original.request_id}</code><pre>{JSON.stringify(original.binding,null,2)}</pre></details></div>{/each}
   {#if pending.length>3}<details><summary>其余原请求（{pending.length-3}）</summary>{#each pending.slice(3) as original}<p>{original.text}</p><button type="button" disabled={busy} onclick={()=>retry(original)}>重试原请求</button>{/each}</details>{/if}
  </section>
 {/if}
 {#if result}<section class="ask-answer-result" aria-label="回答提交结果">
  <h3>{result.current_validity.state==='historical_only'?'原回答回执已确认 · 仅历史记录':'回答已记录'}</h3>
  {#if result.current_validity.state==='historical_only'}<p>{result.current_validity.reason}</p>{/if}
  <p>{result.receipt.result_ref.reply.text}</p><p>署名：{result.receipt.result_ref.reply.actor}</p>
 </section>{/if}
 {#if view}
  <section aria-label="问答原件">
   <p class="ask-answer-validity">{view.current_validity.state==='current'?'● 当前提问':'◇ 历史提问 · 只读'}</p>
   {#if view.current_validity.state==='historical_only'}<p>{view.current_validity.reason}</p><p>当前版本不支持重建后的首次新答复；已保存的原请求仍可补确认。</p>{/if}
   <h3>{view.current_hold.body.question}</h3>
   {#if view.current_hold.body.options.length}<ul class="ask-answer-options">{#each view.current_hold.body.options as option,i}<li>{option}{#if view.current_hold.body.recommended===i}<span> · 建议选项</span>{/if}</li>{/each}</ul>{/if}
   {#if view.current_hold.body.context}<h4>上下文</h4><pre class="ask-answer-context">{view.current_hold.body.context}</pre>{/if}
   <p>模块：{view.current_hold.body.module}</p>
   <ul class="ask-answer-recipients">{#each view.recipients as r}<li>任务 {r.recipient.task} · 运行 {r.recipient.run}<span>{r.actionable?' · 当前收件运行':' · 已过期或仅历史记录'}</span></li>{/each}</ul>
   {#if allExpired}<p>收件运行已过期或仅历史记录。回答不会恢复运行。</p>{/if}
   <p>本机署名：<strong>{view.human_actor}</strong></p>
   {#if view.current_hold.reply}<section aria-label="此前答复"><h4>此前已记录的首个答复</h4><p>{view.current_hold.reply.text}</p><p>署名：{view.current_hold.reply.actor}</p></section>{/if}
   {#if canAnswer}<form onsubmit={(event)=>{event.preventDefault();void answer();}}><label>本次回答<textarea bind:value={text} disabled={busy}></textarea></label><button type="submit" disabled={busy||!text.trim()}>发送本次回答</button><p>仅回答本次问题，不生成长期决策或批准。</p></form>{/if}
   {#if invalid&&!result&&!pending.length}<p>请重新读取原件；不会自动创建另一份回答。</p>{/if}
   <details><summary>来源与绑定（诊断）</summary><pre>{JSON.stringify({project_id:view.project_id,opening:view.opening,binding:view.binding,recipients:view.recipients},null,2)}</pre></details>
  </section>
 {/if}
 {#if history.length}<details><summary>本机保存的原回答请求（{history.length}）</summary><p>请求记录不代表当前有效答复。</p>{#each history as original}<p>{original.text} · {original.human_actor}</p>{/each}</details>{/if}
</section>
