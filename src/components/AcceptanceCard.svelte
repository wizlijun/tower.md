<script lang="ts">
  import { onDestroy } from 'svelte'
  import type { AcceptanceCard, DelegationEvidence, DelegationObject } from '../lib/api'
  import type { DelegationIntent } from '../lib/delegation'
  import { evidenceKinds, formatDuration, localTime, reviewTimer } from '../lib/delegation'
  let { object, card, disabled = false, onAction, timer = reviewTimer() }: {
    object: DelegationObject
    card: AcceptanceCard
    disabled?: boolean
    onAction: (intent: DelegationIntent) => void
    /** Counts only while the App is visible (FR-202); the parent re-keys it per attempt. */
    timer?: { elapsed: () => number; dispose: () => void }
  } = $props()
  onDestroy(() => timer.dispose())
  let used = $state<number[]>([])
  let usedNote = $state('')
  let takeaway = $state('')
  let sendingBack = $state(false)
  let reason = $state('')
  let verdict = $derived(object.attempts.find((attempt) => attempt.id === card.attempt)?.verdict ?? null)
  let canUse = $derived(object.phase === 'claimed_done' || object.phase === 'done')
  let confirmLabel = $derived(object.human_can_complete && object.phase === 'claimed_done' ? '确认完成' : '已验收')

  function evidenceText(entry: DelegationEvidence): string {
    const result = entry.source_kind === 'check' ? entry.passed === false ? '未通过' : entry.passed ? '通过' : '' : ''
    return [result, entry.actor, entry.version && `版本 ${entry.version.slice(0, 12)}`, localTime(entry.at), entry.note]
      .filter(Boolean).join(' · ')
  }
  function recordUsed() {
    if (!used.length || disabled) return
    onAction({ kind: 'used', object: object.id, data: { criteria: [...used].sort((a, b) => a - b), ...(usedNote.trim() ? { note: usedNote.trim() } : {}) } })
    used = []
    usedNote = ''
  }
  function confirm() {
    if (!card.can_judge || disabled) return
    onAction({ kind: 'confirm', object: object.id, data: {
      review_ms: timer.elapsed(), unproven: [...card.unproven], ...(takeaway.trim() ? { takeaway: takeaway.trim() } : {}),
    } })
  }
  function sendBack() {
    if (!card.can_judge || disabled || !reason.trim()) return
    onAction({ kind: 'send_back', object: object.id, data: { reason: reason.trim(), review_ms: timer.elapsed() } })
  }
</script>

{#snippet evidenceList(entries: DelegationEvidence[])}
  <ul class="evidence-list">
    {#each entries as entry (entry.id)}
      <li>
        <span class="evidence-mark evidence-{entry.source_kind}" class:evidence-failed={entry.passed === false}>
          <span aria-hidden="true">{evidenceKinds[entry.source_kind].symbol}</span> {evidenceKinds[entry.source_kind].label}
        </span>
        <span class="evidence-detail">{evidenceText(entry)}</span>
      </li>
    {/each}
  </ul>
{/snippet}

<section class="acceptance-card" aria-label="验收卡">
  <h3>验收卡 · {card.attempt}</h3>
  {#if object.phase === 'claimed_done'}<p class="verification-status verification-pending">! 已声称、未确认。受托者自述不等于完成。</p>{/if}
  <ol class="acceptance-rows">
    {#each card.rows as row (row.index)}
      <li class="acceptance-row">
        <span class="acceptance-text">{row.index + 1}. {row.text}</span>
        {#if row.unproven}
          <span class="evidence-mark evidence-unproven"><span aria-hidden="true">?</span> 未证实</span>
        {:else}
          {@render evidenceList(row.evidence)}
        {/if}
      </li>
    {/each}
    {#if card.whole.length}
      <li class="acceptance-row">
        <span class="acceptance-text">整体证据（不证明单条标准）</span>
        {@render evidenceList(card.whole)}
      </li>
    {/if}
  </ol>
  {#if !card.rows.length}<p class="panel-description">委托卡没有完成标准。</p>{/if}
  {#if card.unproven.length}<p class="panel-description">{card.unproven.length} 条标准未证实；确认时会如实记下。</p>{/if}

  {#if verdict}
    <p class="verdict-summary {verdict.kind === 'confirmed' ? 'verification-passed' : 'verification-failed'}">
      {verdict.kind === 'confirmed' ? '✓ 已确认' : '↩ 已退回'} · {verdict.event.actor} · {localTime(verdict.event.at)} · 验收时长 {formatDuration(verdict.review_ms)}
      {#if verdict.reason}<br />退回理由：{verdict.reason}{/if}
      {#if verdict.takeaway}<br />改变的看法：{verdict.takeaway.text}{/if}
    </p>
  {/if}

  {#if canUse && card.rows.length}
    <details class="draft-edit-form">
      <summary>我用过</summary>
      <fieldset class="draft-edit-fields" {disabled}>
        {#each card.rows as row (row.index)}
          <label class="draft-file-option"><input type="checkbox" checked={used.includes(row.index)}
            onchange={(event) => { used = event.currentTarget.checked ? [...used, row.index] : used.filter((entry) => entry !== row.index) }} />{row.index + 1}. {row.text}</label>
        {/each}
        <label>说明<input bind:value={usedNote} placeholder="怎么用的（可选）" /></label>
        <button type="button" disabled={!used.length || disabled} onclick={recordUsed}>记下我用过</button>
      </fieldset>
    </details>
  {/if}

  {#if card.can_judge}
    <fieldset class="draft-edit-fields draft-edit-form" {disabled}>
      <label>这次改变了什么看法<input bind:value={takeaway} placeholder="可选" /></label>
      <div class="button-row">
        <button class="primary-button" type="button" disabled={disabled} onclick={confirm}>{confirmLabel}</button>
        <button type="button" disabled={disabled} aria-expanded={sendingBack} onclick={() => { sendingBack = !sendingBack }}>退回</button>
      </div>
      {#if sendingBack}
        <label>退回理由<textarea rows="2" bind:value={reason} placeholder="哪里不符合完成标准（必填）"></textarea></label>
        <button type="button" disabled={disabled || !reason.trim()} onclick={sendBack}>确认退回</button>
      {/if}
    </fieldset>
  {:else if !verdict}
    <p class="panel-description">{object.human_can_complete ? '受托者声称完成后才能验收。' : '检查程序完成并合入 main 后才能验收。'}</p>
  {/if}
  <p class="panel-description">验收时长只计 App 内</p>
</section>
