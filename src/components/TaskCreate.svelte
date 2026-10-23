<script module lang="ts">
  import type { HumanTaskDraft } from '../lib/api'
  export type TaskModule = { id: string; title: string; paths: string[] }
  export type TaskDraft = HumanTaskDraft
</script>

<script lang="ts">
  import { untrack } from 'svelte'
  import { buildBrief, checkpointLine, lines } from '../lib/delegation'
  let { modules, onSubmit, frozen = null, busy = false, error = null, backlog = null, onRetry }: {
    modules: TaskModule[]
    onSubmit: (draft: TaskDraft) => void
    frozen?: TaskDraft | null
    busy?: boolean
    error?: string | null
    /** FR-210: a hint next to the assign button; never blocks. */
    backlog?: { count: number; threshold: number } | null
    onRetry: () => void
  } = $props()
  const id = `T-${crypto.randomUUID()}`
  let moduleId = $state(untrack(() => modules[0]?.id ?? ''))
  let allowedPaths = $state<string[]>(untrack(() => [...new Set(modules[0]?.paths ?? [])]))
  let title = $state('')
  let why = $state('')
  let doneWhen = $state('')
  let limits = $state('')
  let checkpoints = $state('')
  let selfCheck = $state('')
  let touches = $state<string[]>([])
  let dependencies = $state('')
  let decisions = $state('')
  let priority = $state(0)
  let availablePaths = $derived([...new Set(modules
    .filter((entry) => entry.id === moduleId || touches.includes(entry.id))
    .flatMap((entry) => entry.paths))])
  let card = $derived(buildBrief({ why, done_when: doneWhen, limits, checkpoints, self_check: selfCheck }))
  let valid = $derived(Boolean(title.trim() && modules.some((entry) => entry.id === moduleId)
    && allowedPaths.length && allowedPaths.every((path) => availablePaths.includes(path))
    && !card.invalid.length && Number.isInteger(priority) && priority >= -2147483648 && priority <= 2147483647))
  let assignable = $derived(valid && card.brief.done_when.length > 0)

  function chooseModule(value: string) {
    moduleId = value
    touches = []
    allowedPaths = [...new Set(modules.find((entry) => entry.id === value)?.paths ?? [])]
  }
  function toggleTouch(value: string, checked: boolean) {
    touches = checked ? [...touches, value] : touches.filter((entry) => entry !== value)
    const retained = modules.filter((entry) => entry.id === moduleId || touches.includes(entry.id)).flatMap((entry) => entry.paths)
    allowedPaths = allowedPaths.filter((path) => retained.includes(path))
  }
  function submit(clarify: boolean) {
    if (!(clarify ? valid : assignable) || busy || frozen) return
    const { done_when, ...rest } = card.brief
    onSubmit({
      id, title: title.trim(), module: moduleId, allowed_paths: [...allowedPaths], touches: [...touches],
      acceptance: done_when, depends_on: lines(dependencies), decisions: lines(decisions), priority,
      ...rest, ...(clarify ? { clarify: true } : {}),
    })
  }
</script>

<p class="panel-description">委托卡固定 5 栏，只有完成标准必填。委托只把任务交给受托者领取，不批准结构变更；任务绑定当前已批准的版本。</p>
{#if error}<p role="alert">{error}</p>{/if}
{#if frozen}
  <dl class="source-list">
    <dt>任务</dt><dd>{frozen.title} · {frozen.id}</dd>
    <dt>模块</dt><dd>{frozen.module}</dd>
    <dt>授权路径</dt><dd>{frozen.allowed_paths.join('\n')}</dd>
    {#if frozen.why}<dt>目的</dt><dd>{frozen.why}</dd>{/if}
    <dt>完成标准</dt><dd>{frozen.acceptance.join('\n') || '待受托者澄清后由人定稿'}</dd>
    {#if frozen.limits}<dt>边界</dt><dd>{frozen.limits}</dd>{/if}
    {#if frozen.checkpoints?.length}<dt>检查点</dt><dd>{frozen.checkpoints.map(checkpointLine).join('\n')}</dd>{/if}
    {#if frozen.self_check}<dt>交付前自检</dt><dd>{frozen.self_check}</dd>{/if}
    {#if frozen.clarify}<dt>方式</dt><dd>先请受托者澄清</dd>{/if}
  </dl>
  <details><summary>原始任务输入</summary><pre class="document-content">{JSON.stringify(frozen, null, 2)}</pre></details>
  <p class="panel-description">提交结果尚待确认。重试沿用原请求号和全部原输入。</p>
  <button class="primary-button" disabled={busy} onclick={() => { if (!busy) onRetry() }}>{busy ? '正在确认…' : '重试原提交'}</button>
{:else}
  <form class="draft-edit-form" onsubmit={(event) => { event.preventDefault(); submit(false) }}>
    <fieldset class="draft-edit-fields" disabled={busy}>
      <label>任务标题<input bind:value={title} required /></label>
      <label>所属模块<select value={moduleId} onchange={(event) => chooseModule(event.currentTarget.value)} required>
        {#if !modules.length}<option value="">暂无已批准模块</option>{/if}
        {#each modules as entry}<option value={entry.id}>{entry.title} · {entry.id}</option>{/each}
      </select></label>
      <label>目的<textarea rows="2" bind:value={why} placeholder="为什么做这件事（可选）"></textarea></label>
      <label>完成标准<textarea rows="3" bind:value={doneWhen} placeholder="每行一条可验证的标准（必填）"></textarea></label>
      <label>边界<textarea rows="2" bind:value={limits} placeholder="哪些地方不许碰（可选）"></textarea></label>
      <label>检查点<textarea rows="2" bind:value={checkpoints} placeholder="每行一条；行尾可写 @ 2026-10-25 18:00 作为截止时间（本地时间）"></textarea></label>
      {#if card.invalid.length}<p role="alert">检查点时间无效：{card.invalid.join('；')}</p>{/if}
      <label>交付前自检<textarea rows="2" bind:value={selfCheck} placeholder="受托者报告完成前应自己核对什么（可选）"></textarea></label>
      <details>
        <summary>范围与依赖</summary>
        <p class="panel-description">默认授权所选模块的真实路径；可缩小范围。涉及其他模块时，先选择模块，再明确勾选所需路径。</p>
        {#if modules.length > 1}
          <p>涉及其他模块</p>
          {#each modules.filter((entry) => entry.id !== moduleId) as entry}
            <label class="draft-file-option"><input type="checkbox" checked={touches.includes(entry.id)} onchange={(event) => toggleTouch(entry.id, event.currentTarget.checked)} />涉及 {entry.title} · {entry.id}</label>
          {/each}
        {/if}
        <p>授权路径</p>
        <div class="draft-file-list">{#each availablePaths as path}
          <label class="draft-file-option"><input type="checkbox" checked={allowedPaths.includes(path)} onchange={(event) => { allowedPaths = event.currentTarget.checked ? [...allowedPaths, path] : allowedPaths.filter((entry) => entry !== path) }} />{path}</label>
        {/each}</div>
        {#if !availablePaths.length}<p class="panel-description">此模块没有可授权的路径。</p>{/if}
        <label>依赖任务 ID<textarea rows="2" bind:value={dependencies} placeholder="每行一个任务 ID；后端验证依赖有效性"></textarea></label>
        <label>依据决策 ID<textarea rows="2" bind:value={decisions} placeholder="每行一个已存在的决策 ID"></textarea></label>
        <label>优先级<input type="number" min="-2147483648" max="2147483647" step="1" bind:value={priority} required /></label>
      </details>
      {#if !card.brief.done_when.length}<p class="panel-description">补写完成标准后才能委托；也可以先请受托者澄清。</p>{/if}
      <div class="button-row">
        <button class="primary-button" type="submit" disabled={!assignable || busy}>{busy ? '正在提交…' : '委托'}</button>
        <button type="button" disabled={!valid || busy} onclick={() => submit(true)}>先澄清</button>
        {#if backlog}<span class="backlog-hint" role="note">先验收再托付 · {backlog.count} 件待验收</span>{/if}
      </div>
    </fieldset>
  </form>
{/if}
