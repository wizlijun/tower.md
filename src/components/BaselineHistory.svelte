<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import { currentDraftClient, type CurrentBaseline, type CurrentDraftClient } from '../lib/current-draft';
  import { failureMessage, type SavedDraft } from '../lib/draft';
  let { token, client = currentDraftClient, draft = null }: { token:string; client?:CurrentDraftClient; draft?:SavedDraft|null } = $props();
  let current = $state<CurrentBaseline|null>(null);
  let busy = $state(false);
  let loaded = $state(false);
  let error = $state('');
  let generation = 0;
  const labels = $derived(current ? Array.from({length:Number(current.baseline.slice(9))+1}, (_,index)=>`towermd/B${index}`) : []);
  const historicalDraft = $derived(draft !== null && (draft.current_validity.state === 'historical_only' || labels.includes(draft.draft.bundle.target_baseline)));
  $effect(() => {
    const selected = token; const source = client;
    untrack(() => void load(selected,source));
    return () => { generation++; };
  });
  onDestroy(() => { generation++; });
  async function load(selected = token, source = client) {
    const serial = ++generation;
    current=null; loaded=false; busy=true; error='';
    try {
      const value=await source.load(selected);
      if(serial!==generation || selected!==token)return;
      if(value!==null) {
        const match=typeof value.baseline==='string' && /^towermd\/B(0|[1-9]\d*)$/.exec(value.baseline);
        if(!match || !Number.isSafeInteger(Number(match[1])) || !/^([a-f0-9]{40}|[a-f0-9]{64})$/.test(value.main_revision))throw new Error('基线链来源不完整或版本号无效，请重新读取。');
      }
      current=value; loaded=true;
    } catch(failure) {
      if(serial===generation && selected===token)error=failureMessage(failure);
    } finally {
      if(serial===generation && selected===token)busy=false;
    }
  }
</script>

<section aria-label="基线版本链">
  <h2>基线版本链</h2>
  {#if busy}<p role="status">正在读取已核验基线链…</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
  <button class="secondary" type="button" disabled={busy} onclick={()=>void load()}>{error?'重试读取基线链':'重新读取基线链'}</button>
  {#if loaded}
    {#if current}
      <p>已核验基线链 · 来源 main：<code>{current.main_revision}</code></p>
      <ol aria-label="已核验基线版本">{#each labels as label}<li><code>{label}</code></li>{/each}</ol>
    {:else}<p>尚无已核验基线。</p>{/if}
  {/if}
  {#if draft}
    <section aria-label="上次读取草稿">
      <h3>上次读取草稿{historicalDraft?' · 历史草稿':''}</h3>
      <dl class="review-binding">
        <dt>草稿 ID</dt><dd><code>{draft.id}</code></dd>
        <dt>草稿版本</dt><dd><code>{draft.revision}</code></dd>
        <dt>草稿目标</dt><dd><code>{draft.draft.bundle.target_baseline}</code></dd>
        <dt>上次读取效力</dt><dd>{draft.current_validity.state==='current'?'当次读取有效':'当次仅具历史效力'}</dd>
      </dl>
      {#if draft.current_validity.state==='historical_only'}<p>{draft.current_validity.reason}</p>{/if}
      <p>这是已读取的草稿记录；是否仍可编辑或批准须重新读取原件。</p>
    </section>
  {/if}
</section>
