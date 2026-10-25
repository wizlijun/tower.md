<script lang="ts">
  // Spec 12.1, FR-58: why a task waits for a person, and the person's way
  // out. There is no pass: a choice only lets the delegate try again, sends
  // the card back or frees the task for someone else; a review ends only by
  // a verdict, never by waiting.
  import type { InterruptInput, VerdictInput } from '../lib/api'
  import type { DelegationControls } from '../lib/delegation'
  import { reviewStageLabels, type RuntimeInterrupt, type RuntimeReview, type RuntimeTask } from '../lib/runtime'
  let { task, reviews = [], controls }: { task: RuntimeTask; reviews?: RuntimeReview[]; controls?: DelegationControls } = $props()
  // Shape and text as well as colour.
  const kinds: Record<RuntimeInterrupt['kind'], { symbol: string; label: string }> = {
    question: { symbol: '?', label: '等你回答问题' },
    review: { symbol: '⚖', label: '等你评审' },
    gate_failed: { symbol: '✕', label: '验证 3 次未过' },
    missing_info: { symbol: '∅', label: '委托卡缺信息，任务未派出' },
    forbidden: { symbol: '⊘', label: '改动被路径守卫拦截' },
    run_error: { symbol: '‼', label: '运行出错' },
    budget: { symbol: '¤', label: '预算用完' },
  }
  const choices: { choice: InterruptInput['choice']; label: string; hint: string }[] = [
    { choice: 'retry', label: '让 Agent 再修', hint: '同一运行继续，失败计数清零' },
    { choice: 'rewrite', label: '改写任务', hint: '结束运行、释放模块，任务回到待定稿' },
    { choice: 'reassign', label: '重派', hint: '结束运行、释放模块，任务回到就绪' },
  ]
  const verdicts: { verdict: VerdictInput['verdict']; label: string }[] = [
    { verdict: 'passed', label: '通过' }, { verdict: 'changes_requested', label: '要求修改' }, { verdict: 'rejected', label: '驳回' },
  ]
  let interrupt = $derived(task.interrupt ?? null)
  // G2 refused a tool request before it ran (the daemon's reason prefix): not a path-guard stop.
  const TOOL_BLOCKED = 'G2 拦截危险命令或敏感文件'
  let label = $derived(interrupt?.kind === 'forbidden' && interrupt.reason.startsWith(TOOL_BLOCKED)
    ? { symbol: '⊘', label: '危险命令或敏感文件被拦截' } : interrupt ? kinds[interrupt.kind] : null)
  let live = $derived(interrupt?.kind === 'question' || interrupt?.kind === 'review')
  let locked = $derived(!controls || controls.disabled || controls.busy || Boolean(controls.pending))
  // Builtin checks (`core.verify`) and the path guard cannot be waived; a declared gate can, under 检查.
  let waivable = $derived(interrupt?.kind === 'gate_failed' && Boolean(interrupt.gate) && interrupt.gate !== 'core.verify')
  let note = $state('')
  let comments = $state<Record<string, string>>({})
  function decide(choice: InterruptInput['choice']) {
    if (!interrupt || locked) return
    controls!.onAction({ kind: 'interrupt', object: task.input.id,
      data: { choice, interrupt: interrupt.opened, ...(note.trim() ? { note: note.trim() } : {}) } })
    note = ''
  }
  function judge(review: RuntimeReview, verdict: VerdictInput['verdict']) {
    const comment = (comments[review.id] ?? '').trim()
    if (locked || (verdict !== 'passed' && !comment)) return
    controls!.onAction({ kind: 'verdict', object: review.id, data: { verdict, ...(comment ? { comment } : {}) } })
  }
</script>

{#if interrupt}
  <section class="task-interrupt" aria-label="任务中断">
    <p class="verification-status verification-failed">
      <span aria-hidden="true">{label!.symbol}</span> 中断 · {label!.label}{interrupt.gate ? `（${interrupt.gate}）` : ''}
    </p>
    <p class="panel-description">{interrupt.reason}</p>
    {#if live}
      <p class="panel-description">{interrupt.kind === 'review' ? '给出评审结论后任务自动继续。' : '回答问题后任务自动继续。'}不会超时放行。</p>
    {:else}
      <fieldset class="draft-edit-fields" disabled={locked}>
        <label>处理说明（可选）<input maxlength="2000" bind:value={note} /></label>
        <div class="button-row" role="group" aria-label="处理选项">
          {#each choices as option (option.choice)}
            <button type="button" title={option.hint} disabled={locked} onclick={() => decide(option.choice)}>{option.label}</button>
          {/each}
        </div>
      </fieldset>
      <p class="panel-description">
        {waivable ? `也可以在下方“检查”一栏豁免 ${interrupt.gate}（默认 7 天，最长 30 天）。` : '这类中断不能豁免。'}没有“直接通过”：处理不算完成，进度只认证据。
      </p>
    {/if}
  </section>
{/if}

{#each reviews as review (review.id)}
  <section class="task-review" aria-label="评审 {review.id}">
    <h3><span aria-hidden="true">⚖</span> {reviewStageLabels[review.stage] ?? review.stage} · 待你评审</h3>
    <p>{review.summary}</p>
    {#if review.artifacts.length}<p class="panel-description">材料：{review.artifacts.join('、')}</p>{/if}
    {#if review.version}<p class="panel-description">绑定已验证的 tree {review.version.slice(0, 12)}</p>{/if}
    <fieldset class="draft-edit-fields" disabled={locked}>
      <label>意见（要求修改或驳回时必填）<textarea rows="2" maxlength="4000" bind:value={comments[review.id]}></textarea></label>
      <div class="button-row" role="group" aria-label="评审结论">
        {#each verdicts as option (option.verdict)}
          <button type="button" class={option.verdict === 'passed' ? 'primary-button' : ''}
            disabled={locked || (option.verdict !== 'passed' && !(comments[review.id] ?? '').trim())}
            onclick={() => judge(review, option.verdict)}>{option.label}</button>
        {/each}
      </div>
    </fieldset>
  </section>
{/each}
