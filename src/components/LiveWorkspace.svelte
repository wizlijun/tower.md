<script lang="ts">
  import { untrack } from 'svelte';
  import { SvelteFlow, Background, BackgroundVariant, ConnectionMode } from '@xyflow/svelte';
  import '@xyflow/svelte/dist/style.css';
  import TaskRecheckCard from './TaskRecheckCard.svelte';
  import { taskRecheckClient, type TaskRecheckClient } from '../lib/task-recheck';
  import CanvasNode from './CanvasNode.svelte';
  import CanvasEdge from './CanvasEdge.svelte';
  import InterventionBar from './InterventionBar.svelte';
  import type { InterventionClient } from '../lib/interventions';
  import { taskProgressClient, progressStateLabel, evidenceResultLabel, type TaskProgressClient, type TaskProgressView, type ProgressProposal } from '../lib/task-progress';
  import { attentionItems, unresolvedItems, moduleTasks, moduleSummary, overviewFlow, taskFlow, taskEvents, taskDiagnostics, observeProgress, type LiveStatus } from '../lib/live-overview';
  import type { FlowNode } from '../lib/canvas-flow';
  import type { ProjectOverview } from '../lib/project';
  import { changesSinceClose } from '../lib/observation-window';
  import { dependencyChain } from '../lib/dependency-chain';
  let { token, overview, client = taskProgressClient, interventionClient, recheckClient = taskRecheckClient, onhold, onproposal, onrevision, onprojecttools, refreshKey = 0 } = $props<{ token: string; overview: ProjectOverview; client?: TaskProgressClient; interventionClient: InterventionClient; recheckClient?: TaskRecheckClient; onproposal?: (proposal: ProgressProposal) => void; onhold?: (id: string, kind: string) => void; onrevision?: (revision: string) => void | Promise<void>; onprojecttools?: () => void; refreshKey?: number }>();
  type OverviewNode = ProjectOverview['nodes'][number];
  let progress = $state<TaskProgressView | null>(null);
  let status = $state<LiveStatus>('loading');
  let connectionError = $state('');
  let module = $state<string | null>(null);
  let task = $state<string | null>(null);
  let selectedNode = $state<string | null>(null);
  let showMore = $state(false);
  let inspector = $state<'project' | 'node' | 'task' | 'intervention' | null>(null);
  let menu = $state<{x:number;y:number}|null>(null);
  let openRecheck = $state(false);
  $effect(() => { const selected = task; untrack(() => { openRecheck = false; void selected; }); });
  const taskPositions = new Map<string, { x: number; y: number }>();
  let observer: ReturnType<typeof observeProgress> | undefined;
  const nodeTypes = { canvas: CanvasNode }; const edgeTypes = { canvas: CanvasEdge };
  const selectedToken = $derived(token);
  const selectedClient = $derived(client);
  const selectedRefreshKey = $derived(refreshKey);
  const attention = $derived(attentionItems(progress));
  const unresolved = $derived(unresolvedItems(progress));
  const progressRevision = $derived(progress?.revision);
  const revisionMatches = $derived(!progress || progress.revision === (overview.authorityRevision ?? overview.revision));
  const coherentProgress = $derived(revisionMatches ? progress : null);
  const tasks = $derived(module ? moduleTasks(coherentProgress, module) : coherentProgress?.tasks ?? []);
  const graph = $derived(module ? taskFlow(tasks, taskPositions, coherentProgress) : overviewFlow(overview, progress));
  const statusLabel = $derived(status === 'live' ? '● 已连接实时事件' : status === 'loading' ? '◌ 正在读取真实态势' : status === 'updating' ? '◌ 记录正在更新 · 暂显示已核实记录' : status === 'history' ? '◇ 快照历史 · 需要恢复 Hub' : status === 'unavailable' ? '◇ 暂无任务记录' : '△ 实时连接失联 · 已读取记录');
  const projectNodeId = $derived.by(() => {
    let id = '__towermd_project_view__';
    while (overview.nodes.some((node: OverviewNode) => node.id === id) || progress?.tasks.some(task => task.id === id)) id += '_';
    return id;
  });
  // This is a system-view card, not a repository object. Its position depends
  // only on baseline geometry; fast records never move existing nodes.
  const projectPosition = $derived({x: Math.min(0, ...overview.nodes.map((node: OverviewNode) => node.x)) - 350, y: Math.min(0, ...overview.nodes.map((node: OverviewNode) => node.y))});
  const projection = $derived.by(() => {
    const projectNode: FlowNode = {id:projectNodeId,type:'canvas',position:projectPosition,width:300,height:260,draggable:false,connectable:false,deletable:false,
      data:{canonicalId:projectNodeId,kind:'项目视图',title:overview.title,text:`${statusLabel}\n${revisionMatches ? '' : '基线版本同步中\n'}任务 ${progress?.tasks.length ?? 0} · ${attention.length} 项待关注记录${attention.slice(0,3).map(item=>`\n${item.symbol} ${item.label} · ${item.title}`).join('')}`,editable:false,opaque:false,raw:null}};
    return {nodes:[projectNode,...graph.nodes],edges:graph.edges};
  });
  const currentTask = $derived(progress?.tasks.find(item => item.id === task));
  const currentNode = $derived(overview.nodes.find((item: OverviewNode) => item.id === selectedNode));
  const timeline = $derived(task && progress ? taskEvents(progress, task) : []);
  const diagnostics = $derived(task ? taskDiagnostics(progress, task) : []);
  const changes = $derived(progress ? changesSinceClose(progress) : null);
  const chain = $derived(module ? dependencyChain(tasks, coherentProgress?.tasks ?? []) : null);
  const controlModules = $derived(revisionMatches ? overview.nodes.filter((node: OverviewNode) => node.obj && node.objectType === 'Module').map((node: OverviewNode) => ({ id: node.obj!, title: node.title })) : []);
  const controlTasks = $derived(coherentProgress?.tasks.flatMap(item => typeof item.module === 'string' ? [{ id: item.id, title: item.title ?? item.id, module: item.module }] : []) ?? []);
  const controlWatermark = $derived(progress ? `${progress.project_id}:${progress.daemon_instance ?? ''}:${progress.through_seq ?? ''}:${progress.revision}` : 'loading');
  $effect(() => {
    const sourceToken = selectedToken; const sourceClient = selectedClient;
    progress = null; module = null; task = null; selectedNode = null; inspector = null; menu = null; taskPositions.clear();
    const active = untrack(() => observeProgress(sourceToken, sourceClient, value => progress = value, (value, message) => { status = value; connectionError = message; }));
    observer = active;
    return () => { active.close(); if (observer === active) observer = undefined; };
  });
  $effect(() => { if (selectedRefreshKey > 0) untrack(() => observer?.refresh()); });
  $effect(() => {
    const revision = progressRevision;
    if (!revision || revisionMatches) return;
    // Depend on the primitive target/match, so same-revision progress reads
    // cannot reset the retry budget. Cleanup cancels on convergence or switch.
    let active = true; let retries = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;
    async function syncOverview() {
      try { await onrevision?.(revision!); }
      catch { /* App retains the source error; retry the same observed revision. */ }
      if (active && retries < 3) timer = setTimeout(() => { retries++; void syncOverview(); }, 1000 * 2 ** retries);
    }
    untrack(() => void syncOverview());
    return () => { active = false; clearTimeout(timer); };
  });
  function select(id: string) {
    menu = null;
    if (id === projectNodeId) { inspect(); return; }
    if (module) { task = id; inspector = 'task'; return; }
    selectedNode = id; task = null; inspector = 'node';
  }
  function enterModule(id: string) { module = id; task = null; selectedNode = null; inspector = null; }
  function closeInspector() { inspector = null; task = null; selectedNode = null; openRecheck = false; }
  export function inspect() { menu = null; task = null; selectedNode = null; inspector = 'project'; }
  export function intervene() { menu = null; task = null; selectedNode = null; inspector = 'intervention'; }
  export function home() { module = null; closeInspector(); menu = null; }
  export function dismiss() { closeInspector(); menu = null; }
  function context(event: MouseEvent) {
    event.stopPropagation();
    if ((event.target as HTMLElement).closest('.canvas-inspector')) return;
    event.preventDefault();
    menu = {x: Math.max(0, Math.min(event.clientX, window.innerWidth - 210)), y: Math.max(0, Math.min(event.clientY, window.innerHeight - 210))};
  }
  function openAttention(id: string, kind: string, ids: string[], proposal?:ProgressProposal) {
    if(kind==='proposal'){if(proposal)onproposal?.(proposal);return;}
    if (kind === 'ask' || kind === 'review') onhold?.(id, kind);
    else if (ids[0]) openTask(ids[0]);
  }
  function openTask(id:string) { const found = progress?.tasks.find(item => item.id === id); module = typeof found?.module === 'string' ? found.module : null; task = id; inspector = 'task'; }
</script>

<!-- The canvas is the sole persistent surface. Inspectors and menus exist only after selection. -->
<!-- svelte-ignore a11y_no_static_element_interactions, a11y_no_noninteractive_element_interactions -->
<section class="live-workspace" aria-label="真实项目画布" oncontextmenu={context} onkeydown={(event)=>{if(event.key==='Escape'){menu=null;closeInspector();}}}>
  <div class="live-flow" role="region" aria-label={module ? '模块任务依赖图' : '权威基线模块地图'}>
    {#key module ?? 'z0'}<SvelteFlow nodes={projection.nodes} edges={projection.edges} {nodeTypes} {edgeTypes} proOptions={{hideAttribution:true}} fitView fitViewOptions={{ padding: 0.2, maxZoom: 1 }}
      nodeOrigin={[0,0]} zIndexMode="manual" elevateNodesOnSelect={false} connectionMode={ConnectionMode.Loose} nodesDraggable={false} nodesConnectable={false} elementsSelectable={true} deleteKey={null} selectionKey={null} multiSelectionKey={null} selectionOnDrag={false}
      minZoom={0.01} maxZoom={2} panOnScroll={true} zoomOnScroll={false} zoomOnDoubleClick={false} disableKeyboardA11y={true}
      onnodeclick={({node}) => select(node.id)} onbeforeconnect={() => false} onbeforedelete={async () => false} onbeforereconnect={() => false}>
      <Background variant={BackgroundVariant.Dots}/>
    </SvelteFlow>{/key}
  </div>
  {#if menu}<div class="canvas-context-menu" role="menu" aria-label="画布操作" style:left="{menu.x}px" style:top="{menu.y}px">
    <button type="button" role="menuitem" onclick={home}>返回全局画布</button>
    <button type="button" role="menuitem" onclick={inspect}>态势与关注</button>
    <button type="button" role="menuitem" onclick={intervene}>插话与运行控制</button>
    {#if onprojecttools}<button type="button" role="menuitem" onclick={()=>{menu=null;onprojecttools?.();}}>项目工具</button>{/if}
  </div>{/if}
  {#if inspector}<aside class="canvas-inspector" aria-label={inspector==='project'?'项目态势与关注':inspector==='intervention'?'插话与运行控制':inspector==='task'?'任务详情':'节点详情'}>
    <button class="canvas-close secondary" type="button" onclick={closeInspector}>关闭详情</button>
    {#if inspector==='intervention'}
      <InterventionBar {token} client={interventionClient} modules={controlModules} tasks={controlTasks} refreshKey={controlWatermark} onrecorded={() => observer?.refresh()}/>
    {:else if inspector==='project'}
      <h2>{overview.title}</h2><p>系统项目视图 · 只读，不是仓库结构对象。</p>
      <p role="status">{statusLabel}</p><p>{progress ? `任务 ${progress.tasks.length} · 已合并 ${progress.tasks.filter(item => item.state === 'merged').length} · 截至 seq ${progress.through_seq ?? '未知'}` : '等待核实任务、运行与证据'}</p>
      <button class="secondary" type="button" onclick={() => observer?.refresh()}>重新读取态势</button>
      {#if connectionError}<p class="live-diagnostic">{connectionError}</p>{/if}
      {#if !revisionMatches}<p class="live-diagnostic" role="status">main 已更新，正在同步基线画布。旧图暂不叠加新版本任务状态。<button class="secondary" type="button" onclick={() => progress && onrevision?.(progress.revision)}>重试同步画布</button></p>{/if}
      {#if progress?.diagnostics.length}<details class="live-diagnostic"><summary>记录读取提示（{progress.diagnostics.length}）</summary>{#each progress.diagnostics as diagnostic}<p>{diagnostic}</p>{/each}</details>{/if}
      {#if changes}<details class="live-diagnostic"><summary>{changes.label}{#if changes.closedAt!==null} · {new Date(changes.closedAt).toLocaleString()}{/if}{#if changes.coverage==='unavailable'} · 变化窗口不可用{:else if changes.coverage==='first_open'} · 尚无关闭起点{:else} · {changes.events.length} 条当前可核对事件{changes.coverage==='partial'?' · 仅部分记录':''}{/if}</summary>{#each changes.diagnostics as diagnostic}<p>{diagnostic}</p>{/each}<p>变化窗口仅用于回看；尚未解决的提问和评审仍保留在节点与关注中。</p><ol>{#each changes.events as event (event.eid)}<li>#{event.seq} · {event.type} · {event.obj}<details><summary>查看原事件</summary><pre>{JSON.stringify(event,null,2)}</pre></details></li>{/each}</ol></details>{/if}
      <section aria-label="需要你关注">
        <h3>需要你关注</h3><p>{attention.length} 项待关注记录</p>
        {#if attention.length===0}<p>{progress?'当前没有升级的未决记录。':'正在核对真实中断…'}</p>{/if}
        {#each attention.slice(0,3) as item (item.id)}<button class="live-attention-item" type="button" onclick={()=>openAttention(item.id,item.kind,item.tasks,item.proposal)}><span>{item.symbol} {item.label}</span><strong>{item.title}</strong><span>影响 {item.tasks.length} 个任务 · 打开原件核对</span>{#if item.proposal}<span>建议：{item.proposal.recommendation}</span>{/if}</button>{/each}
        {#if attention.length>3}<button class="secondary" type="button" aria-expanded={showMore} onclick={()=>showMore=!showMore}>{showMore?'收起更多':`更多（${attention.length-3}）`}</button>{/if}
        {#if showMore}<ul>{#each attention.slice(3) as item (item.id)}<li><button class="secondary" type="button" onclick={()=>openAttention(item.id,item.kind,item.tasks,item.proposal)}>{item.symbol} {item.label} · {item.title}</button></li>{/each}</ul>{/if}
      </section>
      {#if coherentProgress?.tasks.some(item=>!overview.nodes.some((node: OverviewNode)=>node.obj===item.module))}<details><summary>未绑定画布模块的任务</summary>{#each coherentProgress.tasks.filter(item=>!overview.nodes.some((node: OverviewNode)=>node.obj===item.module)) as item (item.id)}<button class="secondary" type="button" onclick={()=>openTask(item.id)}>{item.title??item.id} · {progressStateLabel(item.state)}</button>{/each}</details>{/if}
      <button type="button" class="secondary" onclick={intervene}>插话与运行控制</button>{#if onprojecttools}<button class="secondary" type="button" onclick={onprojecttools}>项目工具</button>{/if}
    {:else if inspector==='node' && currentNode}
      <h2>{currentNode.title}</h2><pre aria-label="节点原文">{currentNode.text}</pre>
      {#if currentNode.obj && (currentNode.objectType==='Module'||coherentProgress?.tasks.some(item=>item.module===currentNode.obj))}<p>{revisionMatches?moduleSummary(coherentProgress,currentNode.obj):'基线版本同步中'}</p><button class="secondary" type="button" disabled={!revisionMatches} onclick={()=>enterModule(currentNode!.obj!)}>进入模块任务</button>{/if}
    {:else if inspector==='task' && task && progress}
    <section class="live-task-detail" aria-label="任务运行时间线"><div class="live-section-heading"><h3>{currentTask?.title ?? task}</h3><span>{currentTask ? progressStateLabel(currentTask.state) : '当前无此任务记录'}</span></div><code>{task}</code>
      {#if currentTask?.needs_recheck === true}<p role="status">↻ 需复核 · 当前基线影响尚待人工核对</p>{#if revisionMatches && progress.state === 'recorded'}<button type="button" class="secondary" onclick={() => openRecheck = true}>打开复核原件</button>{:else}<p>历史或版本同步中的记录只读；等待重新核实当前状态。</p>{/if}{/if}
      {#if openRecheck}{#key task}<TaskRecheckCard token={token} taskId={task} client={recheckClient} onresolved={() => observer?.refresh()}/>{/key}{/if}
      <h4>运行与费用</h4>{#each progress.runs.filter(item => item.task === task) as run (run.id)}<div class="live-record"><strong>{run.id}</strong> · {progressStateLabel(run.state)}<p>Harness：{typeof run.self_reported === 'object' && run.self_reported && 'harness' in run.self_reported ? String(run.self_reported.harness) : '未报告'} · 费用：{typeof run.cost_usd === 'number' ? `$${run.cost_usd}` : '未知'}</p></div>{:else}<p>尚无运行记录。</p>{/each}
      <h4>证据</h4>{#each progress.evidence.filter(item => item.task === task || progress?.runs.some(run => run.task === task && run.id === item.run)) as evidence (evidence.id)}<details class="live-record"><summary>{evidence.id} · {evidenceResultLabel(evidence.result)}</summary><pre>{JSON.stringify(evidence, null, 2)}</pre></details>{:else}<p>尚无证据。Agent 自述完成不计作合并。</p>{/each}
      <h4>问题与评审</h4>{#each unresolved.filter(item => item.tasks.includes(task!)) as item (item.id)}<button type="button" class="secondary" onclick={() => openAttention(item.id, item.kind, item.tasks, item.proposal)}>{item.symbol} {item.title} · 打开核对</button>{:else}<p>没有未决问题或评审记录。</p>{/each}
      {#if diagnostics.length}<h4>检查诊断原件</h4>{#each diagnostics as diagnostic (String(diagnostic.id))}<details class="live-record" open={diagnostic.source_state == null && diagnostic.resolved_by == null}>
        <summary>{diagnostic.kind === 'forbidden' ? '⛔ 禁止' : '🚧 门禁失败'} · {String(diagnostic.id)} · {diagnostic.source_state === 'historical_only' || diagnostic.resolved_by != null ? '历史诊断' : diagnostic.source_state != null ? '来源未核实' : '当前检查记录'}</summary>
        {#if typeof diagnostic.source_reason === 'string'}<p>{diagnostic.source_reason}</p>{/if}
        <p>运行：{String(diagnostic.run ?? '未提供')} · 原检查提交：{String(diagnostic.commit ?? '未提供')} · 证据：{String(diagnostic.evidence ?? '未提供')}</p>
        {#if typeof diagnostic.path === 'string'}<p>路径：{diagnostic.path}</p>{/if}
        <p>此处只核对检查原件，不批准越界或继续运行。</p><pre>{JSON.stringify(diagnostic, null, 2)}</pre>
      </details>{/each}{/if}
      <h4>实际事件时间线</h4><p class="live-diagnostic">显示当前服务返回的实际事件窗口；已压缩的更早历史不在此窗口中。{progress.events_floor_seq != null ? `窗口起点 seq ${progress.events_floor_seq}` : ''}</p><ol class="live-timeline">{#each timeline as event (event.eid)}<li><details><summary>#{event.seq} · {event.type} · {event.t ?? ''}{event.stale ? ' · 历史/过期事件' : ''}</summary><pre>{JSON.stringify(event, null, 2)}</pre></details></li>{:else}<li>当前事件窗口中没有该任务的原始事件。</li>{/each}</ol>
    </section>
    {/if}
    {#if module && inspector!=='intervention'}<button class="secondary" type="button" onclick={home}>返回全局画布</button>{/if}
    {#if chain && inspector==='task'}<p class="live-diagnostic">最长未完成依赖链（按任务数，不含工期或预测）：{chain.ids.length?chain.ids.join(' → '):'当前不能认证或无未完成任务'}</p>{#each chain.diagnostics as diagnostic}<p class="live-diagnostic">{diagnostic}</p>{/each}{/if}
  </aside>{/if}
</section>
