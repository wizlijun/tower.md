<script lang="ts">
  import { onDestroy } from 'svelte'
  import { projectClient, type BaselineDraft, type DraftSummary, type ScanJob, type ScanReport, type CanvasNode, type ProjectClient, type ProjectSelection, type ProjectView } from './lib/api'
  import ProjectCanvas from './components/ProjectCanvas.svelte'
  import VerificationDetails from './components/VerificationDetails.svelte'
  import BaselineDetails from './components/BaselineDetails.svelte'
  import { moduleStatuses, updateCandidate, type CardStatus } from './lib/verification'

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
  let selectionToken: string | null = null
  let scanGeneration = 0
  let scanTimer: ReturnType<typeof setTimeout> | undefined
  let scanning = $state(false)
  let activeJob = $state<ScanJob | null>(null)
  let startPending = $state(false)
  let cancelling = $state(false)
  let showExecution = $state(false)
  let jobActive = $derived(activeJob?.status === 'running')
  let scanError = $state<string | null>(null)
  let report = $state<ScanReport | null>(null)
  let candidate = $state(false)
  let draft = $state<BaselineDraft | null>(null)
  let drafts = $state<DraftSummary[]>([])
  let showDrafts = $state(false)
  let showDraft = $state(false)
  let draftLoading = $state(false)
  let draftError = $state<string | null>(null)
  let draftGeneration = 0
  let detailReport = $derived(draft?.scan ?? (candidate ? report : null))
  let statuses: Record<string, CardStatus> = $derived(draft?.evidence_error
    ? Object.fromEntries(draft.scan.modules.map((module) => [module.id, 'unverifiable' as const]))
    : moduleStatuses(detailReport, !draft && jobActive && activeJob?.id === detailReport?.id))
  let showScan = $state(false)
  let displayed = $derived(draft?.view ?? (candidate && report ? report.view : view))
  let selectedModule = $derived(detailReport && selected
    ? detailReport.modules.find((module) => module.id === selected?.object_id || module.id === selected?.id)
    : undefined)

  function stopPolling() {
    scanGeneration += 1
    clearTimeout(scanTimer)
    scanTimer = undefined
    scanning = false
  }

  onDestroy(() => { stopPolling(); draftGeneration += 1; selectionToken = null })

  function selectCandidate(value: boolean) {
    draft = null
    candidate = value
    fitVersion += 1
    closeOverlays()
  }

  async function receiveScan(job: ScanJob, token: string, request: number) {
    if (request !== scanGeneration || token !== selectionToken) return
    activeJob = job
    if (job.report) {
      const firstReport = report?.id !== job.report.id
      report = updateCandidate(report, job.report)
      if (selected && candidate) selected = report.view.nodes.find((node) => node.id === selected?.id) ?? selected
      if (firstReport) selectCandidate(true)
    }
    if (job.status === 'running') {
      cancelling = cancelling || job.cancel_requested === true || job.phase === 'cancelling'
      scanning = true
      scanTimer = setTimeout(async () => {
        scanTimer = undefined
        try {
          await receiveScan(await client.pollScan(token, job.id), token, request)
        } catch (cause) {
          failScan(cause, request)
        }
      }, 2000)
      return
    }
    scanning = false
    cancelling = false
    if (job.report?.verification && !job.report.verification.cleanup_complete) throw new Error(`验证清理失败，不能接受通过结果。${job.error || ''}`)
    if (job.status === 'failed') throw new Error(job.error || '扫描未完成，请重试。')
    if (job.status === 'ready' && !job.report) throw new Error('扫描结果缺少候选报告，请重试。')
    scanError = null
  }

  function failScan(cause: unknown, request: number) {
    if (request !== scanGeneration) return
    scanning = false
    cancelling = false
    scanError = String(cause)
  }

  async function resumeScan() {
    if (!project || !activeJob || busy) return
    menu = null
    if (draft) selectCandidate(true)
    stopPolling()
    const request = scanGeneration
    const token = project.token
    scanning = true
    scanError = null
    try {
      await receiveScan(await client.pollScan(token, activeJob.id), token, request)
    } catch (cause) {
      failScan(cause, request)
    }
  }

  async function cancelScan() {
    if (!project || !activeJob || !jobActive || cancelling) return
    stopPolling()
    const request = scanGeneration
    const token = project.token
    const id = activeJob.id
    cancelling = true
    scanning = true
    scanError = null
    try {
      await receiveScan(await client.cancelScan(token, id), token, request)
    } catch (cause) {
      failScan(cause, request)
    }
  }

  async function startScan() {
    if (!project || !view?.docs_dir || busy || scanning || startPending || jobActive || failedSelection) return
    menu = null
    closeOverlays()
    if (draft) selectCandidate(false)
    stopPolling()
    const request = scanGeneration
    const token = project.token
    activeJob = null
    startPending = true
    scanning = true
    scanError = null
    try {
      const job = await client.startScan(token)
      // Refresh may stop observing before the start acknowledgement returns.
      // Keep the ID so the existing daemon job can be resumed, not duplicated.
      if (request !== scanGeneration && token === selectionToken) activeJob = job
      else await receiveScan(job, token, request)
    } catch (cause) {
      failScan(cause, request)
    } finally {
      if (token === selectionToken) startPending = false
    }
  }

  function closeOverlays() {
    draftGeneration += 1
    draftLoading = false
    showDrafts = false
    showDraft = false
    menu = null
    selected = null
    showProject = false
    showScan = false
    showExecution = false
  }

  async function listDrafts() {
    if (!project || busy || failedSelection) return
    closeOverlays()
    showDrafts = true
    drafts = []
    draftError = null
    draftLoading = true
    const request = draftGeneration
    const token = project.token
    try {
      if (!client.listBaselines) throw new Error('当前客户端不支持综合草稿，请更新 App。')
      const result = await client.listBaselines(token)
      if (request !== draftGeneration || token !== selectionToken) return
      drafts = result
    } catch (cause) {
      if (request === draftGeneration && token === selectionToken) draftError = String(cause)
    } finally {
      if (request === draftGeneration && token === selectionToken) draftLoading = false
    }
  }

  async function loadDraft(id: string) {
    if (!project || busy || failedSelection) return
    stopPolling()
    const request = ++draftGeneration
    const token = project.token
    draftLoading = true
    draftError = null
    try {
      if (!client.loadBaseline) throw new Error('当前客户端不支持综合草稿，请更新 App。')
      const loaded = await client.loadBaseline(token, id)
      if (request !== draftGeneration || token !== selectionToken) return
      const changed = draft?.id !== loaded.id
      // Reopening evidence for the same draft does not move its existing nodes.
      const previousNodes = new Map(draft?.view.nodes.map((node) => [node.id, node]) ?? [])
      draft = !changed && draft ? {
        ...loaded,
        view: { ...loaded.view, nodes: loaded.view.nodes.map((node) => {
          const previous = previousNodes.get(node.id)
          return previous ? { ...node, x: previous.x, y: previous.y, width: previous.width, height: previous.height } : node
        }) },
      } : loaded
      candidate = false
      if (changed) fitVersion += 1
      closeOverlays()
    } catch (cause) {
      if (request === draftGeneration && token === selectionToken) draftError = String(cause)
    } finally {
      if (request === draftGeneration && token === selectionToken) draftLoading = false
    }
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
    closeOverlays()
    busy = true
    try {
      const selection = await client.chooseProject()
      if (request !== generation) return
      if (!selection) {
        busy = false
        return
      }
      selectionToken = selection.token
      stopPolling()
      activeJob = null
      startPending = false
      cancelling = false
      report = null
      draft = null
      drafts = []
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
    closeOverlays()
    if (!project) {
      await chooseProject()
      return
    }
    stopPolling()
    if (candidate || draft) selectCandidate(false)
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
      y: Math.max(8, Math.min(event.clientY, window.innerHeight - 490)),
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
    closeOverlays()
    selected = node
  }
</script>

<svelte:window onkeydown={keydown} />

<main class="canvas-shell" class:is-candidate={candidate || !!draft} aria-label="Tower.md 画布" oncontextmenu={contextMenu}>
  {#if displayed && displayed.nodes.length > 0}
    <ProjectCanvas view={displayed} {statuses} {fitVersion} onOpen={openNode} />
  {:else}
    <div class="canvas-empty">
      {#if view}
        <span class="canvas-empty-mark" aria-hidden="true">◇</span>
        <h1>{view.title}</h1>
        <p>{draft ? '此综合草稿没有可显示的节点。' : candidate ? '本次扫描没有可显示的候选节点。' : '当前提交还没有可显示的画布节点。'}</p>
        {#if view.docs_dir}
          <button class="primary-button" onclick={startScan} disabled={busy || scanning || startPending || jobActive || !!failedSelection}>扫描并验证</button>
        {/if}
        <button class="subtle-button" onclick={() => { closeOverlays(); showProject = true }}>{view.unplaced.length ? `查看未摆放模块（${view.unplaced.length}）` : '查看项目说明'}</button>
        <button class="subtle-button" onclick={listDrafts} disabled={busy || !!failedSelection}>查看综合草稿</button>
      {:else}
        <span class="canvas-empty-mark" aria-hidden="true">◇</span>
        <h1>Tower.md</h1>
        <p>从仓库里的卡片与关系开始。</p>
        <button class="primary-button" onclick={chooseProject} disabled={busy}>打开项目</button>
        <span class="canvas-empty-hint">⌘ / Ctrl + O</span>
      {/if}
    </div>
  {/if}

  {#if draft}
    <button class="candidate-badge" onclick={() => { closeOverlays(); showDraft = true }}>综合草稿 · 未批准{draft.stale ? ' · 已过期' : ''}{draft.evidence_error ? ' · 证据不可复核' : ''}</button>
  {:else if candidate}
    <button class="candidate-badge" onclick={() => { closeOverlays(); showScan = true }}>候选 · {report?.verification ? '验证报告' : '尚未验证'}</button>
  {/if}

  {#if scanning}
    <div class="canvas-notice" role="status">{cancelling ? '正在取消并清理验证进程…' : activeJob?.phase === 'queued' ? '等待验证执行…' : activeJob?.phase === 'verifying' ? '正在验证固定提交…' : '正在分析已提交源码…'}</div>
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
        <button onclick={jobActive ? resumeScan : startScan} disabled={busy || scanning || !!failedSelection}>{jobActive ? '重新连接验证' : '重新扫描'}</button>
        <button onclick={() => { scanError = null }}>关闭</button>
      </div>
    </section>
  {/if}

  {#if menu}
    <div class="canvas-menu" role="menu" aria-label="画布菜单" style:left={`${menu.x}px`} style:top={`${menu.y}px`}>
      <button role="menuitem" onclick={chooseProject}>{project ? '切换项目' : '打开项目'} <span>⌘ O</span></button>
      <button role="menuitem" onclick={refresh} disabled={!project}>刷新 <span>⌘ R</span></button>
      <button role="menuitem" onclick={() => { fitVersion += 1; menu = null }} disabled={!displayed?.nodes.length}>适应画布</button>
      <button role="menuitem" onclick={startScan} disabled={!view?.docs_dir || busy || scanning || startPending || jobActive || !!failedSelection}>扫描并验证</button>
      <button role="menuitem" onclick={() => { closeOverlays(); showExecution = true }}>扫描与执行说明</button>
      <button role="menuitem" onclick={listDrafts} disabled={!project || busy || !!failedSelection}>查看综合草稿</button>
      {#if draft}
        <button role="menuitem" onclick={() => selectCandidate(false)}>返回当前画布</button>
        <button role="menuitem" onclick={() => { closeOverlays(); showDraft = true }}>综合说明</button>
      {/if}
      {#if jobActive}
        {#if !scanning}<button role="menuitem" onclick={resumeScan}>查看验证进度</button>{/if}
        <button role="menuitem" onclick={() => { menu = null; void cancelScan() }} disabled={cancelling || busy}>取消本次验证</button>
      {/if}
      {#if report}
        <button role="menuitem" onclick={() => selectCandidate(!candidate)}>{candidate ? '返回当前画布' : '查看候选'}</button>
        <button role="menuitem" onclick={() => { closeOverlays(); showScan = true }}>扫描说明</button>
      {/if}
      <button role="menuitem" onclick={() => { closeOverlays(); showProject = true }} disabled={!view}>项目说明</button>
      <button role="menuitem" onclick={() => { menu = null }}>关闭菜单</button>
    </div>
  {/if}

  {#if showDrafts}
    <aside class="canvas-panel" aria-label="综合草稿">
      <div class="panel-heading"><h2>综合草稿</h2><button class="close-button" aria-label="关闭详情" onclick={closeOverlays}>×</button></div>
      <p class="panel-description">Agent 使用 baseline submit 提交后，可在这里查看综合结构。草稿不会自动批准或执行命令。</p>
      {#if draftLoading}<p role="status">正在读取综合草稿…</p>{/if}
      {#if draftError}<p role="alert">{draftError}</p><button onclick={listDrafts}>重新读取草稿</button>{/if}
      {#if !draftLoading && !draftError && !drafts.length}<p class="panel-description">当前项目还没有综合草稿。</p>{/if}
      <div class="draft-list">
        {#each drafts as item}
          <button onclick={() => loadDraft(item.id)} disabled={draftLoading}>
            <strong>{item.title}</strong>
            <span>{item.producer} · {item.created_at}</span>
            <span>{item.stale ? '已过期 · 只供查看' : '未批准'}</span>
            <code>{item.source_revision}</code>
          </button>
        {/each}
      </div>
    </aside>
  {/if}

  {#if showDraft && draft}
    <aside class="canvas-panel" aria-label="综合说明">
      <div class="panel-heading"><h2>综合草稿</h2><button class="close-button" aria-label="关闭详情" onclick={closeOverlays}>×</button></div>
      <BaselineDetails {draft} />
    </aside>
  {/if}

  {#if showExecution}
    <aside class="canvas-panel" aria-label="扫描与执行说明">
      <div class="panel-heading"><h2>扫描并验证</h2><button class="close-button" aria-label="关闭详情" onclick={closeOverlays}>×</button></div>
      <p class="panel-description">扫描会在固定提交的分离工作树中执行仓库验证命令，继承启动服务时的环境，允许网络，不自动安装依赖。分离工作树不是安全沙箱，命令使用当前用户权限。</p>
      <p class="panel-description">刷新、切换项目或关闭窗口只停止界面跟踪，不会取消服务中的验证任务。需要中止时，使用“取消本次验证”，并等待进程和工作树清理结果。</p>
      <p class="panel-description">验证结果属于候选报告，不代表批准结构或认证任务完成。</p>
    </aside>
  {/if}

  {#if showScan && report}
    <aside class="canvas-panel" aria-label="扫描说明">
      <div class="panel-heading">
        <h2>扫描候选</h2>
        <button class="close-button" aria-label="关闭详情" onclick={closeOverlays}>×</button>
      </div>
      <p class="panel-description">这是根据已提交源码生成的候选结构，未批准，也未写入当前画布。验证结果以实际报告为准。候选固定于下方来源提交；刷新当前画布不会重新扫描。</p>
      <dl class="source-list">
        <dt>覆盖率</dt><dd>{Math.round(report.coverage * 100)}% · {report.assigned_file_count} / {report.source_file_count} 个源码文件</dd>
        <dt>模块</dt><dd>{report.modules.length} 个候选模块</dd>
      </dl>
      <p class="panel-description">扫描并验证会执行仓库命令，继承启动服务时的环境并允许网络，不自动安装依赖；分离工作树不是安全沙箱。</p>
      {#if jobActive}
        <div class="button-row">
          {#if !scanning}<button onclick={resumeScan}>查看验证进度</button>{/if}
          <button onclick={cancelScan} disabled={cancelling || busy}>{cancelling ? '等待取消与清理…' : '取消本次验证'}</button>
        </div>
      {/if}
      {#if activeJob?.status === 'cancelled'}<p class="panel-description">⊘ 本次验证已取消；以各命令结果与清理状态为准。</p>{/if}
      <VerificationDetails {report} running={jobActive && activeJob?.id === report.id} />
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
          {#if !draft}<dl class="source-list"><dt>置信度</dt><dd>{Math.round(selectedModule.confidence * 100)}%</dd><dt>依据</dt><dd>{selectedModule.rationale}</dd></dl>{/if}
          <p class="panel-description">{draft ? '综合草稿 · 结构与解释由提交者自报 · 未批准' : '候选模块 · 未批准'}</p>
          {#if detailReport}<VerificationDetails report={detailReport} moduleId={selectedModule.id} evidenceError={draft?.evidence_error} running={!draft && jobActive && activeJob?.id === detailReport.id} />{/if}
        {/if}
        {#if draft}
          {#if draft.stale}<p class="verification-status verification-unverifiable">! 来源提交已过期，此草稿只供查看。</p>{/if}
          <p class="panel-description">提交者（自报）：{draft.producer} · {draft.created_at} · 未批准</p>
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
        <span>来源 · {selected && (candidate || draft) ? displayed?.reference : view.reference}</span>
        <code>{selected && (candidate || draft) ? displayed?.revision : view.revision}</code>
      </div>
    </aside>
  {/if}
</main>
