<script lang="ts">
  import { onDestroy } from 'svelte'
  import { projectClient, type ScanJob, type ScanReport, type CanvasNode, type ProjectClient, type ProjectSelection, type ProjectView } from './lib/api'
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
  let scanGeneration = 0
  let scanTimer: ReturnType<typeof setTimeout> | undefined
  let scanning = $state(false)
  let scanError = $state<string | null>(null)
  let report = $state<ScanReport | null>(null)
  let candidate = $state(false)
  let showScan = $state(false)
  let displayed = $derived(candidate && report ? report.view : view)
  let selectedModule = $derived(candidate && report && selected
    ? report.modules.find((module) => module.id === selected?.object_id || module.id === selected?.id)
    : undefined)

  function stopPolling() {
    scanGeneration += 1
    clearTimeout(scanTimer)
    scanTimer = undefined
    scanning = false
  }

  onDestroy(stopPolling)

  function selectCandidate(value: boolean) {
    candidate = value
    fitVersion += 1
    closeOverlays()
  }

  async function receiveScan(job: ScanJob, token: string, request: number) {
    if (request !== scanGeneration || token !== project?.token) return
    if (job.status === 'ready') {
      if (!job.report) throw new Error('扫描结果缺少候选报告，请重试。')
      report = job.report
      scanning = false
      scanError = null
      selectCandidate(true)
    } else if (job.status === 'failed') {
      throw new Error(job.error || '扫描未完成，请重试。')
    } else {
      scanTimer = setTimeout(async () => {
        scanTimer = undefined
        try {
          await receiveScan(await client.pollScan(token, job.id), token, request)
        } catch (cause) {
          failScan(cause, request)
        }
      }, 2000)
    }
  }

  function failScan(cause: unknown, request: number) {
    if (request !== scanGeneration) return
    scanning = false
    scanError = String(cause)
  }

  async function startScan() {
    if (!project || !view?.docs_dir || busy || scanning || failedSelection) return
    menu = null
    stopPolling()
    const request = scanGeneration
    const token = project.token
    scanning = true
    scanError = null
    try {
      await receiveScan(await client.startScan(token), token, request)
    } catch (cause) {
      failScan(cause, request)
    }
  }

  function closeOverlays() {
    menu = null
    selected = null
    showProject = false
    showScan = false
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
      stopPolling()
      report = null
      candidate = false
      scanError = null
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
    stopPolling()
    if (candidate) selectCandidate(false)
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
      y: Math.max(8, Math.min(event.clientY, window.innerHeight - 390)),
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
    showScan = false
    selected = node
  }
</script>

<svelte:window onkeydown={keydown} />

<main class="canvas-shell" class:is-candidate={candidate} aria-label="Tower.md 画布" oncontextmenu={contextMenu}>
  {#if displayed && displayed.nodes.length > 0}
    <ProjectCanvas view={displayed} {fitVersion} onOpen={openNode} />
  {:else}
    <div class="canvas-empty">
      {#if view}
        <span class="canvas-empty-mark" aria-hidden="true">◇</span>
        <h1>{view.title}</h1>
        <p>{candidate ? '本次扫描没有可显示的候选节点。' : '当前提交还没有可显示的画布节点。'}</p>
        {#if view.docs_dir}
          <button class="primary-button" onclick={startScan} disabled={busy || scanning || !!failedSelection}>扫描仓库</button>
        {/if}
        <button class="subtle-button" onclick={() => { closeOverlays(); showProject = true }}>{view.unplaced.length ? `查看未摆放模块（${view.unplaced.length}）` : '查看项目说明'}</button>
      {:else}
        <span class="canvas-empty-mark" aria-hidden="true">◇</span>
        <h1>Tower.md</h1>
        <p>从仓库里的卡片与关系开始。</p>
        <button class="primary-button" onclick={chooseProject} disabled={busy}>打开项目</button>
        <span class="canvas-empty-hint">⌘ / Ctrl + O</span>
      {/if}
    </div>
  {/if}

  {#if candidate}
    <button class="candidate-badge" onclick={() => { closeOverlays(); showScan = true }}>候选 · 尚未验证</button>
  {/if}

  {#if scanning}
    <div class="canvas-notice" role="status">正在分析已提交源码…</div>
  {:else if busy}
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

  {#if scanError}
    <section class="canvas-error" role="alert" aria-label="仓库扫描失败">
      <strong>仓库扫描失败</strong>
      <p>{scanError}</p>
      <div class="button-row">
        <button onclick={startScan} disabled={busy || scanning || !!failedSelection}>重新扫描</button>
        <button onclick={() => { scanError = null }}>关闭</button>
      </div>
    </section>
  {/if}

  {#if menu}
    <div class="canvas-menu" role="menu" aria-label="画布菜单" style:left={`${menu.x}px`} style:top={`${menu.y}px`}>
      <button role="menuitem" onclick={chooseProject}>{project ? '切换项目' : '打开项目'} <span>⌘ O</span></button>
      <button role="menuitem" onclick={refresh} disabled={!project}>刷新 <span>⌘ R</span></button>
      <button role="menuitem" onclick={() => { fitVersion += 1; menu = null }} disabled={!displayed?.nodes.length}>适应画布</button>
      <button role="menuitem" onclick={startScan} disabled={!view?.docs_dir || busy || scanning || !!failedSelection}>扫描仓库</button>
      {#if report}
        <button role="menuitem" onclick={() => selectCandidate(!candidate)}>{candidate ? '返回当前画布' : '查看候选'}</button>
        <button role="menuitem" onclick={() => { closeOverlays(); showScan = true }}>扫描说明</button>
      {/if}
      <button role="menuitem" onclick={() => { closeOverlays(); showProject = true }} disabled={!view}>项目说明</button>
      <button role="menuitem" onclick={() => { menu = null }}>关闭菜单</button>
    </div>
  {/if}

  {#if showScan && report}
    <aside class="canvas-panel" aria-label="扫描说明">
      <div class="panel-heading">
        <h2>扫描候选</h2>
        <button class="close-button" aria-label="关闭详情" onclick={closeOverlays}>×</button>
      </div>
      <p class="panel-description">这是根据已提交源码生成的候选结构，尚未验证或批准，也未写入当前画布。候选固定于下方来源提交；刷新当前画布不会重新扫描。</p>
      <dl class="source-list">
        <dt>覆盖率</dt><dd>{Math.round(report.coverage * 100)}% · {report.assigned_file_count} / {report.source_file_count} 个源码文件</dd>
        <dt>模块</dt><dd>{report.modules.length} 个候选模块</dd>
      </dl>
      <h3>验证命令 · 尚未验证</h3>
      <p class="panel-description">本次扫描没有执行命令；命令来自仓库配置，需要后续验证。</p>
      {#each report.commands as command}
        <div class="scan-command"><span>{command.directory}</span><code>{command.command}</code></div>
      {:else}<p class="panel-description">没有发现验证命令。</p>{/each}
      {#if report.modules.some((module) => module.suggested_task)}
        <h3>未发现验证命令的模块建议</h3>
        <ul class="diagnostics">{#each report.modules as module}{#if module.suggested_task}<li>{module.title}：{module.suggested_task}</li>{/if}{/each}</ul>
      {/if}
      {#if report.unassigned.length}
        <h3>未归属源码</h3>
        <ul class="diagnostics">{#each report.unassigned as file}<li>{file}</li>{/each}</ul>
      {/if}
      <h3>历史热点与共同修改</h3>
      <p class="panel-description">热点统计文件出现在历史提交中的次数；共同修改表示两个模块出现在同一历史提交中，不代表调用依赖。</p>
      {#if report.hotspots.length}<ul class="diagnostics">{#each report.hotspots as hotspot}<li>{hotspot.path} · {hotspot.commits} 次提交</li>{/each}</ul>{/if}
      {#if report.couplings.length}<ul class="diagnostics">{#each report.couplings as coupling}<li>{coupling.from} ↔ {coupling.to} · {coupling.commits} 次共同提交</li>{/each}</ul>{/if}
      {#if !report.hotspots.length && !report.couplings.length}<p class="panel-description">没有可显示的历史统计。</p>{/if}
      {#if report.diagnostics.length}<h3>扫描说明</h3><ul class="diagnostics">{#each report.diagnostics as diagnostic}<li>{diagnostic}</li>{/each}</ul>{/if}
      <div class="source-footnote"><span>来源 · {report.reference}</span><code>{report.revision}</code><span>源码树</span><code>{report.tree}</code></div>
    </aside>
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
        {#if selectedModule}
          <dl class="source-list"><dt>置信度</dt><dd>{Math.round(selectedModule.confidence * 100)}%</dd><dt>依据</dt><dd>{selectedModule.rationale}</dd></dl>
          <p class="panel-description">候选模块 · 尚未验证与批准</p>
        {/if}
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
        <span>来源 · {selected && candidate ? displayed?.reference : view.reference}</span>
        <code>{selected && candidate ? displayed?.revision : view.revision}</code>
      </div>
    </aside>
  {/if}
</main>
