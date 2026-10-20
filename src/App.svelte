<script lang="ts">
  import { projectClient, type CanvasNode, type ProjectClient, type ProjectSelection, type ProjectView } from './lib/api'
  import ProjectCanvas from './components/ProjectCanvas.svelte'

  let { client = projectClient }: { client?: ProjectClient } = $props()
  let project = $state<ProjectSelection | null>(null)
  let failedSelection = $state<ProjectSelection | null>(null)
  let view = $state<ProjectView | null>(null)
  let error = $state<string | null>(null)
  let busy = $state(false)
  let selected = $state<CanvasNode | null>(null)
  let showProject = $state(false)
  let menu = $state<{ x: number; y: number } | null>(null)
  let fitVersion = $state(0)
  let generation = 0

  function closeOverlays() {
    menu = null
    selected = null
    showProject = false
  }

  async function load(selection: ProjectSelection, request: number) {
    try {
      const loaded = await client.loadProject(selection.token)
      if (request !== generation) return
      const changedProject = project?.path !== selection.path
      project = selection
      view = loaded
      error = null
      failedSelection = null
      if (changedProject) fitVersion += 1
      closeOverlays()
    } catch (cause) {
      if (request === generation) {
        error = String(cause)
        failedSelection = selection
      }
    } finally {
      if (request === generation) busy = false
    }
  }

  async function chooseProject() {
    const request = ++generation
    menu = null
    busy = true
    try {
      const selection = await client.chooseProject()
      if (request !== generation) return
      if (!selection) {
        busy = false
        return
      }
      // Keep the last successful canvas until its replacement is fully loaded.
      await load(selection, request)
    } catch (cause) {
      if (request === generation) {
        error = String(cause)
        busy = false
      }
    }
  }

  async function refresh() {
    menu = null
    if (!project) {
      await chooseProject()
      return
    }
    const request = ++generation
    busy = true
    await load(project, request)
  }

  async function retry() {
    if (!failedSelection) {
      await refresh()
      return
    }
    const request = ++generation
    busy = true
    await load(failedSelection, request)
  }

  function contextMenu(event: MouseEvent) {
    event.preventDefault()
    menu = {
      x: Math.max(8, Math.min(event.clientX, window.innerWidth - 224)),
      y: Math.max(8, Math.min(event.clientY, window.innerHeight - 240)),
    }
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === 'Escape') closeOverlays()
    if (event.metaKey || event.ctrlKey) {
      if (event.key.toLowerCase() === 'o') {
        event.preventDefault()
        void chooseProject()
      } else if (event.key.toLowerCase() === 'r') {
        event.preventDefault()
        void refresh()
      }
    }
    if (event.shiftKey && event.key === 'F10') {
      event.preventDefault()
      menu = { x: 24, y: 24 }
    }
  }

  function openNode(node: CanvasNode) {
    menu = null
    showProject = false
    selected = node
  }
</script>

<svelte:window onkeydown={keydown} />

<main class="canvas-shell" aria-label="Tower.md 画布" oncontextmenu={contextMenu}>
  {#if view && view.nodes.length > 0}
    <ProjectCanvas {view} {fitVersion} onOpen={openNode} />
  {:else}
    <div class="canvas-empty">
      {#if view}
        <span class="canvas-empty-mark" aria-hidden="true">◇</span>
        <h1>{view.title}</h1>
        <p>当前提交还没有可显示的画布节点。</p>
        <button class="subtle-button" onclick={() => { showProject = true }}>{view.unplaced.length ? `查看未摆放模块（${view.unplaced.length}）` : '查看项目说明'}</button>
      {:else}
        <span class="canvas-empty-mark" aria-hidden="true">◇</span>
        <h1>Tower.md</h1>
        <p>从仓库里的卡片与关系开始。</p>
        <button class="primary-button" onclick={chooseProject} disabled={busy}>打开项目</button>
        <span class="canvas-empty-hint">⌘ / Ctrl + O</span>
      {/if}
    </div>
  {/if}

  {#if busy}
    <div class="canvas-notice" role="status">正在读取项目…</div>
  {/if}

  {#if error}
    <section class="canvas-error" role="alert" aria-label="项目读取失败">
      <strong>项目读取失败</strong>
      <p>{error}</p>
      <div class="button-row">
        <button onclick={retry} disabled={busy}>重试</button>
        <button onclick={chooseProject} disabled={busy}>选择其他项目</button>
        {#if view}<button onclick={() => { error = null }}>关闭</button>{/if}
      </div>
    </section>
  {/if}

  {#if menu}
    <div class="canvas-menu" role="menu" aria-label="画布菜单" style:left={`${menu.x}px`} style:top={`${menu.y}px`}>
      <button role="menuitem" onclick={chooseProject}>{project ? '切换项目' : '打开项目'} <span>⌘ O</span></button>
      <button role="menuitem" onclick={refresh} disabled={!project}>刷新 <span>⌘ R</span></button>
      <button role="menuitem" onclick={() => { fitVersion += 1; menu = null }} disabled={!view?.nodes.length}>适应画布</button>
      <button role="menuitem" onclick={() => { selected = null; showProject = true; menu = null }} disabled={!view}>项目说明</button>
      <button role="menuitem" onclick={() => { menu = null }}>关闭菜单</button>
    </div>
  {/if}

  {#if (selected || showProject) && view}
    <aside class="canvas-panel" aria-label={selected ? '节点详情' : '项目说明'}>
      <div class="panel-heading">
        <h2>{selected?.title || view.title}</h2>
        <button class="close-button" aria-label="关闭详情" onclick={closeOverlays}>×</button>
      </div>
      {#if selected}
        <dl class="source-list">
          <dt>类型</dt><dd>{selected.object_type || selected.kind}</dd>
          {#if selected.object_id}<dt>对象</dt><dd>{selected.object_id}</dd>{/if}
          {#if selected.file}<dt>文件</dt><dd>{selected.file}</dd>{/if}
          {#if selected.url}<dt>链接</dt><dd>{selected.url}</dd>{/if}
        </dl>
        <pre class="document-content">{selected.content || '此节点没有正文。'}</pre>
      {:else}
        <p class="panel-description">当前显示仓库提交中的画布。可平移、缩放和查看原文。</p>
        <p class="panel-description">这是只读预览；慢层编辑与批准尚未接入。</p>
        <dl class="source-list">
          <dt>项目</dt><dd>{project?.path}</dd>
          <dt>文档目录</dt><dd>{view.docs_dir || '尚未初始化'}</dd>
          <dt>画布</dt><dd>{view.nodes.length} 个节点 · {view.edges.length} 条关系</dd>
        </dl>
        {#if view.unplaced.length}
          <h3>未摆放模块</h3>
          <p class="panel-description">这些模块还没有画布节点。</p>
          <div class="unplaced-list">
            {#each view.unplaced as node}
              <button onclick={() => openNode(node)}>{node.title}</button>
            {/each}
          </div>
        {/if}
        {#if view.diagnostics.length}
          <h3>读取说明</h3>
          <ul class="diagnostics">{#each view.diagnostics as diagnostic}<li>{diagnostic}</li>{/each}</ul>
        {/if}
        <button onclick={chooseProject}>切换项目</button>
      {/if}
      <div class="source-footnote">
        <span>来源 · {view.reference}</span>
        <code>{view.revision}</code>
      </div>
    </aside>
  {/if}
</main>
