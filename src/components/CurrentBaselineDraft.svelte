<script lang="ts">
  import { onDestroy } from 'svelte';
  import { currentDraftClient, type CurrentDraftClient, type CurrentBaseline, type PreparedCreation } from '../lib/current-draft';
  import { failureMessage } from '../lib/draft';
  let { token, client = currentDraftClient, oncreated }: { token:string; client?:CurrentDraftClient; oncreated:(id:string)=>Promise<void> } = $props();
  let confirmedDraftId=$state('');let opened=$state(false);let busy=$state(false);let error=$state('');let current=$state<CurrentBaseline|null>(null);let pending=$state<PreparedCreation[]>([]);let queueKnown=$state(false);let generation=0;
  onDestroy(()=>{generation++;});
  async function load(){if(busy)return;opened=true;const g=++generation;busy=true;error='';current=null;queueKnown=false;
    try{const originals=await client.pending(token);if(g!==generation)return;pending=originals;queueKnown=true;const value=await client.load(token);if(g!==generation)return;current=value;}
    catch(failure){if(g===generation)error=failureMessage(failure);}finally{if(g===generation)busy=false;}
  }
  async function send(original:PreparedCreation,g:number){
    const outcome=await client.submit(token,original.request_id);if(g!==generation)return;
    if(outcome.state==='confirmed'&&outcome.response){
      const result=outcome.response.receipt.result_ref;
      if(outcome.request_id!==original.request_id||result.draft_id!==original.draft_id){error='保存回执与原创建请求不一致；请重试原请求。';return;}
      confirmedDraftId=result.draft_id;
      pending=pending.filter(value=>value.request_id!==original.request_id);
      await oncreated(result.draft_id);
    }else{
      error=outcome.error?failureMessage(outcome.error):'原创建请求尚未确认，请重试原请求。';
      if(outcome.state==='conflict'||outcome.state==='rejected'){const originals=await client.pending(token);if(g!==generation)return;pending=originals;current=null;}
    }
  }
  async function create(){if(busy||confirmedDraftId||!current||!queueKnown||pending.length)return;const subject=current;const g=++generation;busy=true;error='';
    try{const original=await client.prepare(token,subject.baseline,subject.main_revision);if(g!==generation)return;pending=[original];await send(original,g);}
    catch(failure){if(g===generation){error=failureMessage(failure);queueKnown=false;current=null;}}finally{if(g===generation)busy=false;}
  }
  async function retry(original:PreparedCreation){if(busy)return;const g=++generation;busy=true;error='';try{await send(original,g);}catch(failure){if(g===generation)error=failureMessage(failure);}finally{if(g===generation)busy=false;}}
  async function openConfirmed(){if(busy||!confirmedDraftId)return;const g=++generation;busy=true;error='';try{await oncreated(confirmedDraftId);}catch(failure){if(g===generation)error=failureMessage(failure);}finally{if(g===generation)busy=false;}}
</script>
<section class="draft-pending" aria-label="从当前基线创建草稿">
  {#if !opened}<button type="button" onclick={()=>void load()}>创建或恢复变更草稿</button>{:else}
    <h3>从当前基线创建变更草稿</h3>
    <p>复制已批准的慢层对象与画布。编辑先保存到私有草稿，不改变当前基线。后续基线出版与提案流程尚未开放。</p>
    {#if busy}<p role="status">正在处理原创建请求…</p>{/if}
    {#if error}<p class="draft-diagnostic" role="alert">{error}</p>{/if}
    {#if pending.length}<section aria-label="原创建请求等待确认"><p>以下原请求保留原 UUID、身份和来源绑定；重试不会另建草稿。</p>{#each pending as original}<div><strong>{original.human_actor}</strong><code>{original.request_id}</code><p>来源：{original.baseline} · <code>{original.main_revision}</code></p><button type="button" disabled={busy} onclick={()=>retry(original)}>重试原创建请求</button></div>{/each}</section>{/if}
    {#if confirmedDraftId}<section aria-label="已创建草稿"><p>草稿已耐久保存，当前基线未改变：<code>{confirmedDraftId}</code></p><button type="button" disabled={busy} onclick={()=>void openConfirmed()}>打开已创建草稿</button></section>{/if}
    {#if current}<dl class="review-binding"><dt>当前已认证基线</dt><dd>{current.baseline}</dd><dt>固定 main 提交</dt><dd><code>{current.main_revision}</code></dd></dl><button type="button" disabled={busy||!!confirmedDraftId||!queueKnown||pending.length>0} onclick={()=>void create()}>确认创建变更草稿</button>{:else if queueKnown&&!busy&&!error}<p>尚无已认证基线。请先让已有 Agent 会话提交 S6 草稿，并在 App 审阅 B0。</p>{/if}
    <button type="button" class="secondary" disabled={busy} onclick={()=>void load()}>重新读取当前基线与原请求</button>
    <button type="button" class="secondary" disabled={busy} onclick={()=>{opened=false;generation++;}}>收起</button>
  {/if}
</section>
