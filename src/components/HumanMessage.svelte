<script lang="ts">
  import type { AnswerDestination, QuestionCard } from '../lib/human'
  let { question, scopeLabel, destinations = [], frozen = null, busy = false, error = null, onSubmit, onRetry }: {
    question?: QuestionCard
    scopeLabel: string
    destinations?: { value: AnswerDestination; label: string }[]
    frozen?: { text: string; destination: AnswerDestination } | null
    busy?: boolean
    error?: string | null
    onSubmit: (text: string, destination: AnswerDestination) => void
    onRetry: () => void
  } = $props()
  let text = $state('')
  let destination = $state<AnswerDestination>('temporary')
  let slow = $derived(question?.targets.some((target) => ['L0', 'L1', 'L2'].includes(target.layer)) ?? false)
</script>

<p class="panel-description">{scopeLabel}</p>
{#if question}
  <p>{question.draft.text}</p>
  <dl class="source-list"><dt>任务</dt><dd>{question.binding.task}</dd><dt>运行</dt><dd>{question.binding.run}</dd></dl>
  {#if question.draft.context.length}
    <details><summary>相关上下文</summary><ul class="diagnostics">{#each question.draft.context as path}<li>{path}</li>{/each}</ul></details>
  {/if}
  {#if slow}<p class="verification-status verification-pending">! 涉及慢层结构，回答将转为待批准提案，不会直接放行。</p>{/if}
{/if}

{#if frozen}
  <pre class="document-content">{frozen.text}</pre>
  {#if question}<p class="panel-description">去向：{destinations.find((entry) => entry.value === frozen.destination)?.label ?? frozen.destination}</p>{/if}
  {#if error}<p role="alert">{error}</p><p class="panel-description">结果尚未确认。重试沿用原请求号与原输入。</p>{/if}
  <button class="primary-button" disabled={busy} onclick={onRetry}>{busy ? '正在确认…' : '重试原提交'}</button>
{:else}
  {#if error}<p role="alert">{error}</p>{/if}
  <form class="draft-edit-form" onsubmit={(event) => { event.preventDefault(); if (text.trim()) onSubmit(text, destination) }}>
    <fieldset class="draft-edit-fields" disabled={busy}>
      {#if question?.draft.options.length}
        <div class="button-row">{#each question.draft.options as option, index}
          <button type="button" onclick={() => { text = option }}>{option}{index === question.draft.recommended ? '（推荐）' : ''}</button>
        {/each}</div>
      {/if}
      <label>{question ? '回答内容' : '插话内容'}<textarea rows="5" bind:value={text} maxlength="32768" required></textarea></label>
      {#if question && destinations.length}
        <label>回答去向<select bind:value={destination}>{#each destinations as option}<option value={option.value}>{option.label}</option>{/each}</select></label>
      {/if}
      <button class="primary-button" type="submit" disabled={!text.trim()}>{question ? '提交回答' : '发送插话'}</button>
    </fieldset>
  </form>
{/if}
