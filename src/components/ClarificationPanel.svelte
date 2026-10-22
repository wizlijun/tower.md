<script lang="ts">
  import { untrack } from 'svelte'
  import type { DelegationObject, FinalizeInput } from '../lib/api'
  import { buildBrief, checkpointLine, delegateLabel, localTime } from '../lib/delegation'
  let { object, disabled = false, onFinalize }: {
    object: DelegationObject
    disabled?: boolean
    onFinalize: (input: FinalizeInput) => void
  } = $props()
  // Prefilled once from the delegate's draft; the person's edits win (FR-201).
  const initial = untrack(() => object)
  const draft = initial.clarification?.draft ?? null
  const questions = (draft?.questions ?? []).slice(0, 3)
  let why = $state(initial.brief.why ?? '')
  let doneWhen = $state((draft?.done_when.length ? draft.done_when : initial.brief.done_when).join('\n'))
  let limits = $state(initial.brief.limits ?? '')
  let checkpoints = $state((initial.brief.checkpoints ?? []).map(checkpointLine).join('\n'))
  let selfCheck = $state(initial.brief.self_check ?? '')
  let answers = $state<string[]>(questions.map((_, index) => initial.clarification?.answers[index] ?? ''))
  let card = $derived(buildBrief({ why, done_when: doneWhen, limits, checkpoints, self_check: selfCheck }))
  let ready = $derived(card.brief.done_when.length > 0 && !card.invalid.length)

  function finalize() {
    if (!ready || disabled) return
    const given = answers.map((answer) => answer.trim())
    onFinalize({ brief: card.brief, ...(given.some(Boolean) ? { answers: given } : {}) })
  }
</script>

<section aria-label="澄清与定稿">
  <h3>澄清与定稿</h3>
  {#if draft && initial.clarification}
    <p class="panel-description">受托者复述 · {initial.clarification.event.actor} · {draft.self_reported ? delegateLabel(draft.self_reported) : '未自报'} · {localTime(initial.clarification.event.at)}</p>
    <blockquote class="delegate-draft">{draft.restated}</blockquote>
    {#if draft.done_when.length}
      <p class="panel-description">受托者起草的完成标准（已填入下方，可修改）：</p>
      <ol class="diagnostics">{#each draft.done_when as item}<li>{item}</li>{/each}</ol>
    {/if}
  {:else}
    <p class="panel-description">{object.phase === 'clarifying' ? '等待受托者复述与提问（towermd dev clarify）；也可以直接定稿。' : '委托卡尚无完成标准，补写后定稿。'}</p>
  {/if}
  <form class="draft-edit-form" onsubmit={(event) => { event.preventDefault(); finalize() }}>
    <fieldset class="draft-edit-fields" {disabled}>
      {#each questions as question, index}
        <label>问题 {index + 1}：{question}<textarea rows="2" bind:value={answers[index]} placeholder="你的回答（可留空）"></textarea></label>
      {/each}
      <label>目的<textarea rows="2" bind:value={why}></textarea></label>
      <label>完成标准<textarea rows="3" bind:value={doneWhen} placeholder="每行一条可验证的标准（必填）"></textarea></label>
      <label>边界<textarea rows="2" bind:value={limits}></textarea></label>
      <label>检查点<textarea rows="2" bind:value={checkpoints} placeholder="每行一条；行尾可写 @ 2026-10-25 18:00"></textarea></label>
      {#if card.invalid.length}<p role="alert">检查点时间无效：{card.invalid.join('；')}</p>{/if}
      <label>交付前自检<textarea rows="2" bind:value={selfCheck}></textarea></label>
      {#if !card.brief.done_when.length}<p class="panel-description">补写完成标准后才能委托。</p>{/if}
      <button class="primary-button" type="submit" disabled={!ready || disabled}>定稿并委托</button>
    </fieldset>
  </form>
</section>
