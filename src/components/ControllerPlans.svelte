<script lang="ts">
  // Spec 12.5, FR-67: the person's global or module input goes to the
  // controller, which previews a plan. Nothing in it runs until the person
  // confirms it here; "我来接管" runs nothing. There is no timeout: a plan
  // nobody looks at stays a preview. Slow-layer drafts are text only.
  import type { ControllerPlan, PlanStep } from '../lib/api'

  let { projection, module = null, disabled = false, busy = false, error = null, result = null, pending = false,
    onRequest, onResolve, onRetry, onClearModule }: {
    projection: unknown
    module?: string | null
    disabled?: boolean
    busy?: boolean
    error?: string | null
    result?: string | null
    pending?: boolean
    onRequest: (text: string) => void
    onResolve: (plan: string, decision: 'confirm' | 'take_over') => void
    onRetry?: () => void
    onClearModule?: () => void
  } = $props()

  let text = $state('')

  const statusLabels: Record<ControllerPlan['status'], string> = {
    consulting: '总控正在拟计划', previewed: '待你确认', failed: '没能生成，待你处理', confirmed: '已确认执行', taken_over: '已由你接管',
  }

  /** Newest first; open plans before resolved ones. */
  let plans = $derived.by(() => {
    const group = (projection as { ControllerPlan?: unknown } | null | undefined)?.ControllerPlan
    if (!group || typeof group !== 'object') return [] as ControllerPlan[]
    const open = (plan: ControllerPlan) => plan.status === 'consulting' || plan.status === 'previewed' || plan.status === 'failed'
    return (Object.values(group) as ControllerPlan[])
      .filter((plan) => plan && typeof plan.id === 'string')
      .sort((a, b) => Number(open(b)) - Number(open(a)) || b.requested_at.localeCompare(a.requested_at))
  })

  function scopeLabel(plan: ControllerPlan): string {
    if (plan.scope.kind === 'module') return `模块 ${plan.scope.module}`
    if (plan.scope.kind === 'baseline') return `新基线 ${plan.scope.baseline}`
    return '全局'
  }

  function stepLabel(step: PlanStep): string {
    switch (step.action) {
      case 'create': return `新建任务 ${step.task.input.id}：${step.task.input.title}（${step.task.input.module}）`
      case 'priority': return `把 ${step.task} 的优先级改为 ${step.priority}`
      case 'pause': return `暂停 ${step.task}`
      case 'resume': return `继续 ${step.task}`
      case 'rebase': return `把 ${step.task} 改按 ${step.baseline} 执行`
    }
  }

  function affected(plan: ControllerPlan): number {
    return new Set((plan.steps ?? []).map((step) => step.action === 'create' ? step.task.input.id : step.task)).size
  }

  function submit(event: SubmitEvent) {
    event.preventDefault()
    const value = text.trim()
    if (!value) return
    onRequest(value)
    text = ''
  }
</script>

<section aria-label="总控的计划">
  <h3>交给总控</h3>
  <form onsubmit={submit}>
    <label>
      {module ? `对模块 ${module} 说` : '对整个项目说'}
      <textarea bind:value={text} rows="2" placeholder="例如：先把登录相关的任务排到前面" disabled={disabled || busy}></textarea>
    </label>
    <button type="submit" disabled={disabled || busy || !text.trim()}>交给总控</button>
    {#if module && onClearModule}<button type="button" onclick={onClearModule}>改为对整个项目</button>{/if}
  </form>
  <p class="panel-description">总控先给出计划预览；你确认后才执行，不确认就不执行。</p>
  {#if busy || error || result}
    <div role="status" class="panel-description">
      {busy ? '正在提交…' : error ? `未确认：${error}` : result}
      {#if error && pending && onRetry}<button onclick={onRetry}>重试原请求</button>{/if}
    </div>
  {/if}
  {#each plans as plan (plan.id)}
    <article aria-label={`计划 ${plan.id}`}>
      <h4>{scopeLabel(plan)} · {statusLabels[plan.status] ?? plan.status}</h4>
      <p>{plan.text}</p>
      {#if plan.summary}<p>{plan.summary}</p>{/if}
      {#if plan.status === 'failed' && plan.reason}<p role="alert">没能生成计划：{plan.reason}</p>{/if}
      {#if plan.steps?.length}
        <p class="panel-description">涉及 {affected(plan)} 项任务</p>
        <ol>{#each plan.steps as step}<li>{stepLabel(step)}</li>{/each}</ol>
      {/if}
      {#if plan.drafts?.length}
        <details open><summary>人定内容草稿（{plan.drafts.length}）：只供参考，确认也不会执行</summary>
          <ul>{#each plan.drafts as draft}<li><strong>{draft.target}</strong>：{draft.text}</li>{/each}</ul>
        </details>
      {/if}
      {#if plan.notes?.length}<ul class="panel-description">{#each plan.notes as note}<li>{note}</li>{/each}</ul>{/if}
      <p class="panel-description">
        {plan.agent ? `拟定：${plan.agent}` : ''}{plan.agent ? ` · 费用 ${plan.cost_usd == null ? '未知' : `$${plan.cost_usd.toFixed(4)}`}` : ''}
        {plan.resolved_by ? `${plan.agent ? ' · ' : ''}处理：${plan.resolved_by}` : ''}
      </p>
      {#if plan.status === 'previewed'}
        <button onclick={() => onResolve(plan.id, 'confirm')} disabled={disabled || busy}>确认执行</button>
      {/if}
      {#if plan.status === 'consulting' || plan.status === 'previewed' || plan.status === 'failed'}
        <button onclick={() => onResolve(plan.id, 'take_over')} disabled={disabled || busy}>我来接管</button>
      {/if}
    </article>
  {/each}
</section>
