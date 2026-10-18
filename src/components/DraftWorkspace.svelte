<script lang="ts">
  import { onDestroy } from 'svelte';
  import CanvasView from './CanvasView.svelte';
  import ReviewPanel from './ReviewPanel.svelte';
  import DraftEditForm from './DraftEditForm.svelte';
  import { draftClient, failureMessage, type DraftClient, type DraftEdit, type SavedDraft, type PreparedEdit, type EditOutcome } from '../lib/draft';
  import { rawRecord } from '../lib/canvas-flow';
  let { token, client = draftClient, onloaded, candidateId }: { token:string; client?:DraftClient; onloaded?:(saved:SavedDraft|null)=>void; candidateId?:string } = $props();
  let reviewOpen=$state(false);let id=$state('');let saved=$state<SavedDraft|null>(null);let busy=$state(false);let error=$state('');let notice=$state('');
  let pending=$state<PreparedEdit[]>([]);let queueKnown=$state(false);let selected=$state('');let edge=$state(false);let connection=$state<{from:string;to:string}|null>(null);
  let serial=0;onDestroy(()=>{serial++;});
  const selectedEntry=$derived(saved?(edge?saved.canvas_view.edges:saved.canvas_view.nodes).find(entry=>rawRecord(entry.raw)?.id===selected):undefined);
  const selectedObject=$derived(saved?.object_views.find(object=>object.id===rawRecord(rawRecord(selectedEntry?.raw)?.towermd)?.obj));
  async function read(generation:number,draftId:string){
    const value=await client.load(token,draftId);if(generation!==serial)return;
    saved=value;onloaded?.(value);queueKnown=false;
    pending=await client.pending(token,draftId);if(generation!==serial)return;queueKnown=true;
  }
  async function openDraft(draftId:string){if(busy||!draftId)return;
    const generation=++serial;busy=true;error='';notice='';
    try{await read(generation,draftId);}catch(failure){if(generation===serial)error=failureMessage(failure);}finally{if(generation===serial)busy=false;}
  }
  async function open(event?:SubmitEvent){event?.preventDefault();await openDraft(id.trim());}
  async function openCandidate(){if(busy||!candidateId)return;id=candidateId;await openDraft(candidateId);}
  async function accept(outcome:EditOutcome,generation:number,draftId:string){
    if(generation!==serial)return;
    if(outcome.state==='confirmed'&&outcome.response){
      const result=outcome.response.receipt.result_ref;const delivery=result.delivery;
      notice=outcome.response.current_validity.state==='historical_only'?'原请求已确认；当前草稿已变化，显示最新内容。':delivery.state==='local_saved_remote_pending'?`本地已保存 · 远端待推：${delivery.reason??''}`:delivery.state==='remote_saved'?'草稿已保存并推送；尚未批准。':'草稿本地已保存；尚未批准。';
      await read(generation,draftId);
    }else{
      error=outcome.error?failureMessage(outcome.error):'原请求尚未确认。';
      if(outcome.state==='conflict'){await read(generation,draftId);if(generation===serial)notice='草稿发生冲突，已刷新。请重新审阅后再编辑；未自动重放本次操作。';}
      else if(outcome.state==='rejected'){pending=await client.pending(token,draftId);}
    }
  }
  async function edit(value:DraftEdit){
    if(busy||!saved||saved.current_validity.state!=='current'||pending.length||!queueKnown)return;
    const draft=saved;const generation=++serial;busy=true;error='';notice='';
    try{
      const original=await client.prepare(token,draft.id,draft.revision,value);if(generation!==serial)return;
      pending=[original];
      const outcome=await client.submit(token,original.request_id);await accept(outcome,generation,draft.id);
    }catch(failure){if(generation===serial){error=failureMessage(failure);if(rawRecord(failure)?.code==='request_conflict'){await read(generation,draft.id);notice='草稿已刷新，请重新审阅；本次编辑没有自动重放。';}}}
    finally{if(generation===serial)busy=false;}
  }
  async function retry(original:PreparedEdit){
    if(busy||!saved)return;const draftId=saved.id;const generation=++serial;busy=true;error='';notice='';
    try{await accept(await client.submit(token,original.request_id),generation,draftId);}
    catch(failure){if(generation===serial)error=failureMessage(failure);}finally{if(generation===serial)busy=false;}
  }
  async function closeReview(){reviewOpen=false;if(!saved)return;const g=++serial;busy=true;error='';try{await read(g,saved.id);}catch(failure){if(g===serial)error=failureMessage(failure);}finally{if(g===serial)busy=false;}}
  function close(){if(busy)return;serial++;saved=null;reviewOpen=false;selected='';pending=[];queueKnown=false;onloaded?.(null);}
</script>
<section class="draft-workspace" class:draft-open={saved!==null} aria-label="已保存的基线草稿">
  {#if candidateId&&!saved}<button type="button" disabled={busy} onclick={openCandidate}>打开此扫描的 S6 草稿</button>{/if}
  <form class="draft-open-form" onsubmit={open}><label>已有 S6 草稿 ID<input bind:value={id} disabled={busy} placeholder="由 baseline submit 返回的草稿 ID" required/></label><button type="submit" disabled={busy||!id.trim()}>打开已保存草稿</button>{#if saved}<button type="button" class="secondary" disabled={busy} onclick={close}>返回项目预览</button>{/if}</form>
  {#if !saved}<p class="status">已有会话中的 Agent 完成 S6 并提交后，可在这里读取。扫描候选保持只读，不自动生成意图或批准基线。</p>{/if}
  {#if busy}<p class="draft-diagnostic" role="status">正在处理草稿请求…</p>{/if}
  {#if error}<p class="draft-diagnostic" role="alert">{error}</p>{/if}
  {#if notice}<p class="draft-diagnostic" role="status">{notice}</p>{/if}
  {#if saved}
    <div class="draft-source"><strong>已保存草稿 · 不代表当前基线</strong><span>本机身份：{saved.human_actor}</span><code>{saved.revision}</code><span>目标 {saved.draft.bundle.target_baseline}</span><button class="secondary" type="button" disabled={busy||reviewOpen} onclick={()=>open()}>刷新草稿</button><button type="button" disabled={busy||pending.length>0||!queueKnown} onclick={()=>reviewOpen=true}>审阅与人工决定</button></div>
    {#if pending.length}<section class="draft-pending" aria-label="原请求等待确认"><p>原请求尚未确认，新编辑已暂停。重试保留原 UUID 与正文，不重新执行扫描。</p>{#each pending as original}<div><code>{original.request_id}</code><button type="button" disabled={busy} onclick={()=>retry(original)}>重试原请求</button></div>{/each}</section>{/if}
    {#if saved.current_validity.state==='historical_only'}<p class="draft-diagnostic" role="status">历史草稿 · 只读：{saved.current_validity.reason??'源或目标基线已变化，请重新准备草稿。'}</p>{/if}
    {#if reviewOpen}<ReviewPanel {token} id={saved.id} onclose={()=>void closeReview()}/>{:else}
    <div class="draft-main"><CanvasView {saved} editable={saved.current_validity.state==='current'&&!busy&&queueKnown&&pending.length===0} onedit={edit} onselect={(id,isEdge)=>{selected=id;edge=isEdge;connection=null;}} onrelation={(from,to)=>connection={from,to}}/>
      <aside class="draft-sidebar" aria-label="草稿编辑与原文"><DraftEditForm {saved} busy={saved.current_validity.state!=='current'||busy||!queueKnown||pending.length>0} {selected} {edge} {connection} onedit={edit}/>
        {#if selectedEntry}<details open><summary>选中条目原文 · 只读</summary><pre>{selectedObject?.text??JSON.stringify(selectedEntry.raw,null,2)}</pre></details>{/if}
        <details><summary>草稿来源与待确认内容</summary><p>源提交：{saved.draft.bundle.source_revision}</p><p>内容哈希：{saved.draft.content_hash}</p><pre>{JSON.stringify({questions:saved.draft.bundle.questions,risks:saved.draft.bundle.risks},null,2)}</pre></details>
      </aside>
    </div>
    {/if}
  {/if}
</section>
