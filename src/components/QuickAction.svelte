<script lang="ts">
  import { onDestroy } from 'svelte'
  import type { RuntimeAttention } from '../lib/runtime'
  import { reviewTimer } from '../lib/delegation'
  // FR-138: approve, answer or confirm on the card itself. Each button sends
  // the same human request the detail view would; nothing passes by itself.
  let { item, disabled = false, onApprove, onAnswer, onConfirm, onFinishFollowUp, onSeen, onReview, onPermission, timer = reviewTimer() }: {
    item: RuntimeAttention
    disabled?: boolean
    onApprove?: (item: RuntimeAttention) => void
    onAnswer?: (item: RuntimeAttention, text: string) => void
    onConfirm?: (item: RuntimeAttention, reviewMs: number) => void
    /** FR-134: complete a due follow-up. */
    onFinishFollowUp?: (item: RuntimeAttention) => void
    /** FR-136: "看过了" on an object that came back for its rhythm or a connector. */
    onSeen?: (item: RuntimeAttention) => void
    /** FR-186: keep a due decision (to a later day, or for good) or void it; rewriting is answering anew. */
    onReview?: (item: RuntimeAttention, outcome: 'keep' | 'void', reviewOn?: string) => void
    /** FR-63: allow this one tool use, or deny it. There is no standing allow. */
    onPermission?: (item: RuntimeAttention, decision: 'allow' | 'deny') => void
    /** How long the card was visible before the person confirmed (FR-202). */
    timer?: { elapsed: () => number; dispose: () => void }
  } = $props()
  onDestroy(() => timer.dispose())
  let text = $state('')
  let nextReview = $state('')
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
{:else if item.followUp && onFinishFollowUp}
  <div class="button-row"><button {disabled} onclick={() => onFinishFollowUp(item)}>完成跟进</button></div>
{:else if item.decisionReview && onReview}
  <form class="button-row" aria-label="复查决策" onsubmit={(event) => { event.preventDefault(); onReview(item, 'keep', nextReview || undefined) }}>
    <label>下次复查（可不填）<input type="date" bind:value={nextReview} {disabled} /></label>
    <button type="submit" {disabled}>保留</button>
    <button type="button" {disabled} onclick={() => onReview(item, 'void')}>作废</button>
  </form>
  <p class="panel-description">要改写，就重新回答这个问题，记为新的模块决策。</p>
{:else if item.permission && onPermission}
  <div class="button-row" role="group" aria-label="授权">
    <button {disabled} onclick={() => onPermission(item, 'allow')}>本次允许</button>
    <button {disabled} onclick={() => onPermission(item, 'deny')}>拒绝</button>
  </div>
  <p class="panel-description">只对这一次有效；10 分钟内不回答即拒绝。</p>
{:else if item.resurfaced && onSeen}
  <div class="button-row"><button {disabled} onclick={() => onSeen(item)}>看过了</button></div>
{/if}
