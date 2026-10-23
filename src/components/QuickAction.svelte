<script lang="ts">
  import { onDestroy } from 'svelte'
  import type { RuntimeAttention } from '../lib/runtime'
  import { reviewTimer } from '../lib/delegation'
  // FR-138: approve, answer or confirm on the card itself. Each button sends
  // the same human request the detail view would; nothing passes by itself.
  let { item, disabled = false, onApprove, onAnswer, onConfirm, timer = reviewTimer() }: {
    item: RuntimeAttention
    disabled?: boolean
    onApprove?: (item: RuntimeAttention) => void
    onAnswer?: (item: RuntimeAttention, text: string) => void
    onConfirm?: (item: RuntimeAttention, reviewMs: number) => void
    /** How long the card was visible before the person confirmed (FR-202). */
    timer?: { elapsed: () => number; dispose: () => void }
  } = $props()
  onDestroy(() => timer.dispose())
  let text = $state('')
  let question = $derived(item.question)
  let slow = $derived(question?.targets.some((target) => ['L0', 'L1', 'L2'].includes(target.layer)) ?? false)
</script>

{#if question && onAnswer}
  {#if slow}<p class="panel-description">! 涉及人定的内容，回答将转为待批准提案。</p>{/if}
  {#if question.draft.options.length}
    <div class="button-row" role="group" aria-label="一步回答">{#each question.draft.options as option, index}
      <button {disabled} onclick={() => onAnswer(item, option)}>回答：{option}{index === question.draft.recommended ? '（推荐）' : ''}</button>
    {/each}</div>
  {:else}
    <form class="button-row" onsubmit={(event) => { event.preventDefault(); if (text.trim()) onAnswer(item, text) }}>
      <input aria-label="回答内容" bind:value={text} maxlength="32768" {disabled} />
      <button type="submit" disabled={disabled || !text.trim()}>回答</button>
    </form>
  {/if}
{:else if item.approve && onApprove}
  <div class="button-row"><button class="primary-button" {disabled} onclick={() => onApprove(item)}>批准</button></div>
{:else if item.confirm && onConfirm}
  {#if item.confirm.unproven.length}<p class="panel-description">{item.confirm.unproven.length} 条标准未证实；确认时会如实记下。</p>{/if}
  <div class="button-row"><button class="primary-button" {disabled} onclick={() => onConfirm(item, timer.elapsed())}>确认完成</button></div>
{/if}
