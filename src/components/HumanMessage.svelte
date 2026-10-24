<script lang="ts">
  import { untrack } from 'svelte'
  import type { TempRuleEntry } from '../lib/api'
  import type { AnswerDestination, QuestionCard } from '../lib/human'
  let { question, scopeLabel, initialText = '', destinations = [], frozen = null, busy = false, error = null, commands = [], rules = [], onSubmit, onRetry }: {
    question?: QuestionCard
    scopeLabel: string
    initialText?: string
    destinations?: { value: AnswerDestination; label: string }[]
    frozen?: { text: string; destination: AnswerDestination } | null
    busy?: boolean
    error?: string | null
    /** FR-70: the quick commands this entry accepts. */
    commands?: { usage: string; does: string }[]
    /** FR-69: temporary rules already in force here. */
    rules?: TempRuleEntry[]
    /** FR-186: `reviewOn` (`YYYY-MM-DD`) only with a module decision. */
    onSubmit: (text: string, destination: AnswerDestination, reviewOn?: string) => void
    onRetry: () => void
  } = $props()
  let text = $state(untrack(() => initialText))
  let destination = $state<AnswerDestination>('temporary')
  let reviewOn = $state('')
  function submit() {
    if (!text.trim()) return
    if (destination === 'module_decision' && reviewOn) onSubmit(text, destination, reviewOn)
    else onSubmit(text, destination)
  }
  let slow = $derived(question?.targets.some((target) => ['L0', 'L1', 'L2'].includes(target.layer)) ?? false)
</script>

<p class="panel-description">{scopeLabel}</p>
{#if question}
  <p>{question.draft.text}</p>
  <dl class="source-list"><dt>任务</dt><dd>{question.binding.task}</dd><dt>运行</dt><dd>{question.binding.run}</dd></dl>
  {#if question.draft.context.length}
    <details><summary>相关上下文</summary><ul class="diagnostics">{#each question.draft.context as path}<li>{path}</li>{/each}</ul></details>
  {/if}
  {#if slow}<p class="verification-status verification-pending">! 涉及人定的内容，回答将转为待批准提案，不会直接放行。</p>{/if}
{/if}

{#if rules.length}
  <section aria-label="生效中的临时规则">
    <p class="panel-description">生效中的临时规则</p>
    <ul class="diagnostics">{#each rules as rule (rule.id)}<li>{rule.text}（{rule.until} 前有效）</li>{/each}</ul>
  </section>
{/if}

{#if frozen}
  <pre class="document-content">{frozen.text}</pre>
  {#if question}<p class="panel-description">去向：{destinations.find((entry) => entry.value === frozen.destination)?.label ?? frozen.destination}</p>{/if}
  {#if error}<p role="alert">{error}</p><p class="panel-description">结果尚未确认。重试沿用原请求号与原输入。</p>{/if}
  <button class="primary-button" disabled={busy} onclick={onRetry}>{busy ? '正在确认…' : '重试原提交'}</button>
{:else}
  {#if error}<p role="alert">{error}</p>{/if}
  <form class="draft-edit-form" onsubmit={(event) => { event.preventDefault(); submit() }}>
    <fieldset class="draft-edit-fields" disabled={busy}>
      {#if question?.draft.options.length}
        <div class="button-row">{#each question.draft.options as option, index}
          <button type="button" onclick={() => { text = option }}>{option}{index === question.draft.recommended ? '（推荐）' : ''}</button>
        {/each}</div>
      {/if}
      <label>{question ? '回答内容' : '插话内容'}<textarea rows="5" bind:value={text} maxlength="32768" required></textarea></label>
      {#if question && destinations.length}
        <label>回答去向<select bind:value={destination}>{#each destinations as option}<option value={option.value}>{option.label}</option>{/each}</select></label>
        {#if destination === 'module_decision'}
          <label>复查日期（可不填，到期后进关注清单）<input type="date" bind:value={reviewOn} /></label>
        {/if}
      {/if}
      <button class="primary-button" type="submit" disabled={!text.trim()}>{question ? '回答' : '插话'}</button>
    </fieldset>
  </form>
  {#if commands.length}
    <details><summary>快捷命令</summary>
      <dl class="source-list">{#each commands as command}<dt><code>{command.usage}</code></dt><dd>{command.does}</dd>{/each}</dl>
      <p class="panel-description">以 // 开头则原样发送。</p>
    </details>
  {/if}
{/if}
