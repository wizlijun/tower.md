<script module lang="ts">
  export type TaskModule = { id: string; title: string; paths: string[] }
  export type TaskDraft = {
    id: string; title: string; module: string; allowed_paths: string[]; touches: string[]
    acceptance: string[]; depends_on: string[]; decisions: string[]; priority: number
  }
</script>

<script lang="ts">
  import { untrack } from 'svelte'
  let { modules, onSubmit, frozen = null, busy = false, error = null, onRetry }: {
    modules: TaskModule[]
    onSubmit: (draft: TaskDraft) => void
    frozen?: TaskDraft | null
    busy?: boolean
    error?: string | null
    onRetry: () => void
  } = $props()
  const id = `T-${crypto.randomUUID()}`
  let moduleId = $state(untrack(() => modules[0]?.id ?? ''))
  let allowedPaths = $state<string[]>(untrack(() => [...new Set(modules[0]?.paths ?? [])]))
  let title = $state('')
  let acceptance = $state('')
  let touches = $state<string[]>([])
  let dependencies = $state('')
  let decisions = $state('')
  let priority = $state(0)
  let availablePaths = $derived([...new Set(modules
    .filter((entry) => entry.id === moduleId || touches.includes(entry.id))
    .flatMap((entry) => entry.paths))])
  let valid = $derived(Boolean(title.trim() && acceptance.trim() && modules.some((entry) => entry.id === moduleId)
    && allowedPaths.length && allowedPaths.every((path) => availablePaths.includes(path))
    && Number.isInteger(priority) && priority >= -2147483648 && priority <= 2147483647))

  function lines(value: string): string[] {
    return [...new Set(value.split('\n').map((line) => line.trim()).filter(Boolean))]
  }
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
  function submit() {
    if (!valid || busy || frozen) return
    onSubmit({
      id, title: title.trim(), module: moduleId, allowed_paths: [...allowedPaths], touches: [...touches],
      acceptance: lines(acceptance), depends_on: lines(dependencies), decisions: lines(decisions), priority,
    })
  }
</script>

<p class="panel-description">任务绑定当前已批准基线。创建不会启动 Agent，也不会批准结构变更。</p>
{#if error}<p role="alert">{error}</p>{/if}
{#if frozen}
  <dl class="source-list">
    <dt>任务</dt><dd>{frozen.title} · {frozen.id}</dd>
    <dt>模块</dt><dd>{frozen.module}</dd>
    <dt>授权路径</dt><dd>{frozen.allowed_paths.join('\n')}</dd>
    <dt>验收条件</dt><dd>{frozen.acceptance.join('\n')}</dd>
  </dl>
  <details><summary>原始任务输入</summary><pre class="document-content">{JSON.stringify(frozen, null, 2)}</pre></details>
  <p class="panel-description">提交结果尚待确认。重试沿用原请求号和全部原输入。</p>
  <button class="primary-button" disabled={busy} onclick={() => { if (!busy) onRetry() }}>{busy ? '正在确认…' : '重试原提交'}</button>
{:else}
  <form class="draft-edit-form" onsubmit={(event) => { event.preventDefault(); submit() }}>
    <fieldset class="draft-edit-fields" disabled={busy}>
      <label>任务标题<input bind:value={title} required /></label>
      <label>所属模块<select value={moduleId} onchange={(event) => chooseModule(event.currentTarget.value)} required>
        {#if !modules.length}<option value="">暂无已批准模块</option>{/if}
        {#each modules as entry}<option value={entry.id}>{entry.title} · {entry.id}</option>{/each}
      </select></label>
      <label>验收条件<textarea rows="3" bind:value={acceptance} placeholder="每行一条可验证的条件" required></textarea></label>
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
      <button class="primary-button" type="submit" disabled={!valid || busy}>{busy ? '正在提交…' : '创建任务'}</button>
    </fieldset>
  </form>
{/if}
