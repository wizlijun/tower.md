<script lang="ts">
  import type { FrameTemplate } from '../lib/api'
  import { FRAME_RULES_CHARS, FRAME_TITLE_CHARS, TEMPLATE_MIME, frameTitleError, bindable, layoutLabel, rulesTextError, sourcePathError, usable, type FrameChoice } from '../lib/frames'
  import { setByLabels, type SetBy } from '../lib/naming'


  let { templates, loading = false, loadError = null, busy = false, pending = null, receipt = null, dropped = null, converting = null,
    onCreate, onRetry, onAbandon, onReload, onCancelConvert }: {
    templates: FrameTemplate[] | null
    loading?: boolean
    loadError?: string | null
    busy?: boolean
    /** An unconfirmed write; it is only ever retried as it was sent. */
    pending?: { what: string; error: string | null; sending: boolean } | null
    receipt?: string | null
    /** A template the person dragged onto the canvas, waiting for a name. */
    dropped?: string | null
    /** FR-144 方式 2: how many picked nodes wait to become a frame. */
    converting?: number | null
    onCreate: (choice: FrameChoice) => Promise<boolean>
    onRetry: () => void
    onAbandon: () => void
    onReload: () => void
    onCancelConvert?: () => void
  } = $props()

  let chosen = $state<string | null>(null)
  let title = $state('')
  let rules = $state('')
  let setBy = $state<SetBy>('human')
  let mode = $state<'free' | 'bound' | 'mixed'>('free')
  let sourcePath = $state('')
  let createFile = $state(false)
  let text = $state('')
  let template = $derived(templates?.find((entry) => entry.id === chosen) ?? null)
  // FR-163: a text-source template shows a repository file; its cards cannot come from picked nodes.
  let sourced = $derived(!!template?.file_types)
  let pathError = $derived(template && sourced ? sourcePathError(template, sourcePath) : null)
  function fits(entry: FrameTemplate): boolean {
    return usable(entry) && !(converting && entry.file_types)
  }
  let titleError = $derived(title ? frameTitleError(title) : null)
  let rulesError = $derived(rulesTextError(rules))

  function choose(entry: FrameTemplate) {
    if (!fits(entry)) return
    chosen = entry.id
    title = entry.title ?? entry.id
    rules = ''
    setBy = entry.default_set_by ?? 'human'
    mode = 'free'
    sourcePath = ''
    createFile = false
    text = ''
  }

  $effect(() => {
    const id = dropped
    if (!id || !templates) return
    const entry = templates.find((candidate) => candidate.id === id)
    if (entry && entry.id !== chosen) choose(entry)
  })

  async function create() {
    if (!template || busy || pending || frameTitleError(title) || rulesError || pathError) return
    const choice: FrameChoice = { template, title: title.trim(), rules: rules.trim(), setBy }
    if (sourced) choice.source = { path: sourcePath.trim(), create: createFile }
    if (!converting && !sourced && mode !== 'free' && bindable(template)) choice.mode = mode
    else if (!converting && !sourced && text.trim()) choice.text = text
    if (await onCreate(choice)) chosen = null
  }
</script>

<div class="frame-tray-body">
  {#if pending}
    <div class="frame-tray-pending" role="status">
      <p>{pending.sending ? `正在提交：${pending.what}` : `尚未确认：${pending.what}`}</p>
      {#if pending.error}<p class="frame-tray-error" role="alert">{pending.error}</p>{/if}
      {#if !pending.sending}
        <div class="frame-tray-actions">
          <button class="primary-button" onclick={onRetry}>重试原请求</button>
          <button class="subtle-button" onclick={onAbandon}>放弃</button>
        </div>
      {/if}
    </div>
  {/if}
  {#if receipt}<p class="idea-input-note" role="status">{receipt}</p>{/if}
  {#if loading}
    <p class="frame-tray-note">正在读取模板…</p>
  {:else if loadError}
    <p class="frame-tray-error" role="alert">{loadError}</p>
    <button class="subtle-button" onclick={onReload}>重新读取</button>
  {:else if templates}
    {#if converting}
      <div class="frame-tray-pending" role="status">
        <p>选一个模板，把选中的 {converting} 个节点转为容器。核心对象在有状态的模板里跟随它的阶段，其他节点复制成你的卡片；画布上原来的节点不动。</p>
        <button class="subtle-button" onclick={() => onCancelConvert?.()}>不转了</button>
      </div>
    {:else}
      <p class="frame-tray-note">把模板拖到画布上，或选中后放到画布中央。容器只由你摆放、移动和缩放。</p>
    {/if}
    <ul class="frame-tray-list">
      {#each templates as entry (entry.id)}
        <li>
          <button class="frame-tray-template" class:is-chosen={entry.id === chosen} disabled={!fits(entry) || busy}
            draggable={fits(entry) && !busy}
            ondragstart={(event) => { event.dataTransfer?.setData(TEMPLATE_MIME, entry.id); if (event.dataTransfer) event.dataTransfer.effectAllowed = 'copy' }}
            onclick={() => choose(entry)}>
            <strong>{entry.title ?? entry.id}</strong>
            <span>{entry.layout ? layoutLabel(entry.layout) : '无法读取布局'} · {entry.origin === 'starter' ? '内置' : '已安装'} {entry.id}@{entry.version ?? '?'}</span>
            {#if entry.problems.length}<span class="frame-tray-error">不能使用：{entry.problems.join('；')}</span>{/if}
          </button>
        </li>
      {/each}
    </ul>
    {#if template}
      <form class="frame-tray-form" onsubmit={(event) => { event.preventDefault(); void create() }}>
        <label>名称<input bind:value={title} maxlength={FRAME_TITLE_CHARS * 2} /></label>
        {#if titleError}<p class="frame-tray-error">{titleError}</p>{/if}
        <label>规则（可选，给受托者看的一句话）<textarea bind:value={rules} rows="2" maxlength={FRAME_RULES_CHARS * 2}></textarea></label>
        {#if rulesError}<p class="frame-tray-error">{rulesError}</p>{/if}
        {#if sourced}
          <label>仓库里的文件（{(template.file_types ?? []).join('、')}）<input bind:value={sourcePath} placeholder={`docs/plan${template.file_types?.[0] ?? ''}`} /></label>
          <label><input type="checkbox" bind:checked={createFile} /> main 上没有就新建一份空图</label>
          {#if sourcePath && pathError}<p class="frame-tray-error">{pathError}</p>{/if}
        {/if}
        <label>由谁推进
          <select bind:value={setBy}>
            <option value="human">{setByLabels.human}</option>
            <option value="delegate">{setByLabels.delegate}</option>
          </select>
        </label>
        {#if bindable(template) && !converting && !sourced}
          <label>卡片来源
            <select bind:value={mode}>
              <option value="free">自由：卡片由人或受托者写</option>
              <option value="bound">绑定：卡片跟随全部核心对象</option>
              <option value="mixed">混合：跟随核心对象，也可加自由卡片</option>
            </select>
          </label>
        {/if}
        {#if !converting && !sourced && (mode === 'free' || !bindable(template))}
          <label>从文本导入（可选：Markdown 列表、标题、表格，或 Mermaid mindmap、flowchart、timeline、kanban）
            <textarea bind:value={text} rows="4" placeholder={'## 待办\n- 写说明\n- 打标签'}></textarea>
          </label>
          {#if text.trim()}<p class="frame-tray-note">按这个模板读成你的卡片；放不下的行会逐行指出，整段不建。</p>{/if}
        {/if}
        <div class="frame-tray-actions">
          <button class="primary-button" type="submit" disabled={busy || !!pending || !!frameTitleError(title) || !!rulesError || !!pathError}>
            {converting ? '转为容器' : dropped === template.id ? '放到拖放处' : '放到画布中央'}
          </button>
          <button class="subtle-button" type="button" onclick={() => { chosen = null }}>取消</button>
        </div>
      </form>
    {/if}
  {/if}
</div>
