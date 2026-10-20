<script lang="ts">
  import TaskProgressPanel from './components/TaskProgressPanel.svelte';
  import type { ProgressProposal, ProposalReviewIntent } from './lib/proposal-attention';
  import type { ReviewClient } from './lib/review';
  import type { TaskProgressClient } from './lib/task-progress';
  import { interventionClient as defaultInterventionClient, type InterventionClient } from './lib/interventions';
  import RecoveryCard from './components/RecoveryCard.svelte';
  import type { RecoveryClient } from './lib/recovery';
  import TaskReviewCard from './components/TaskReviewCard.svelte';
  import AskAnswerCard from './components/AskAnswerCard.svelte';
  import type { AskAnswerClient } from './lib/ask-answer';
  import type { TaskRecheckClient } from './lib/task-recheck';
  import type { TaskReviewClient } from './lib/task-review';
  import ProjectPicker from './components/ProjectPicker.svelte';
  import PrototypeCanvas from './components/PrototypeCanvas.svelte';
  import LiveWorkspace from './components/LiveWorkspace.svelte';
  import CandidateScan from './components/CandidateScan.svelte';
  import DraftWorkspace from './components/DraftWorkspace.svelte';
  import BaselineHistory from './components/BaselineHistory.svelte';
  import type { DraftClient, SavedDraft } from './lib/draft';
  import type { CurrentDraftClient } from './lib/current-draft';
  import { candidateGraph, type CandidateClient, type CandidateDraft } from './lib/candidate';
  import { chooseProject, loadProjectOverview, type ProjectOverview, type SelectedProject } from './lib/project';
  import { overviewNodes } from './lib/overview';
  let { choose = chooseProject, load = loadProjectOverview, scanClient, savedDraftClient, currentDraftClient, recoveryClient, taskReviewClient, askAnswerClient, taskProgressClient, interventionClient = defaultInterventionClient, taskRecheckClient, proposalReviewClient } = $props<{ choose?: typeof chooseProject; load?: typeof loadProjectOverview; scanClient?: CandidateClient; savedDraftClient?: DraftClient; currentDraftClient?: CurrentDraftClient; recoveryClient?: RecoveryClient; taskReviewClient?: TaskReviewClient; askAnswerClient?: AskAnswerClient; taskProgressClient?: TaskProgressClient; interventionClient?: InterventionClient; taskRecheckClient?: TaskRecheckClient; proposalReviewClient?: ReviewClient }>();
  let showProject = $state(false);
  let toolsOpen = $state(false);
  let tool = $state<'menu' | 'draft' | 'recovery' | 'ask' | 'review' | 'progress' | 'baselines'>('menu');
  let menu = $state<{x:number;y:number}|null>(null);
  let choosing = $state(false);
  let liveWorkspace = $state<LiveWorkspace | undefined>();
  function openTools() { liveWorkspace?.dismiss(); menu=null; candidatePreview=false; toolsOpen=true; tool='menu'; }
  function context(event: MouseEvent) {
    if(event.defaultPrevented || (event.target as Element).closest('.canvas-inspector')) return;
    event.preventDefault(); menu={x:Math.max(8,Math.min(event.clientX,window.innerWidth-220)),y:Math.max(8,Math.min(event.clientY,window.innerHeight-270))};
  }
  function closePanels() { liveWorkspace?.dismiss(); menu=null; showProject=false; toolsOpen=false; candidatePreview=false; savedDraft=null; proposalReviewIntent=null; activeHold=null; }
  function keyboard(event: KeyboardEvent) {
    if(event.key==='Escape') closePanels();
    if(event.shiftKey&&event.key==='F10') {event.preventDefault();liveWorkspace?.dismiss();menu={x:16,y:16};}
    if((event.metaKey||event.ctrlKey)&&event.key.toLowerCase()==='o') { event.preventDefault(); void openProject(); }
  }
  async function openProject() {
    if(choosing)return; choosing=true; error=''; menu=null;
    try { const selected=await choose(); if(selected)selectProject(selected); }
    catch(failure) { error=typeof failure==='object'&&failure&&'message' in failure?String(failure.message):String(failure); }
    finally { choosing=false; }
  }
  let proposalReviewIntent = $state<ProposalReviewIntent | null>(null); let proposalReviewError = $state(''); let proposalReviewSerial = 0;
  let activeHold = $state<{id:string;kind:string}|null>(null);
  let liveRefresh = $state(0);
  let project = $state<SelectedProject | null>(null);
  let view = $state<ProjectOverview | null>(null);
  let loading = $state(false);
  let error = $state('');
  let overviewError = $state('');
  let overviewTarget = $state('');
  let demoMode = $state(false);
  let candidate = $state<CandidateDraft | null>(null);
  let candidatePreview = $state(false);
  let savedDraft = $state<SavedDraft | null>(null);
  let lastReadDraft = $state<{token:string;draft:SavedDraft}|null>(null);
  let request = 0;
  const graph = $derived(candidatePreview && candidate ? candidateGraph(candidate) : null);
  const nodes = $derived(graph?.nodes ?? (view ? overviewNodes(view) : []));
  const edges = $derived(graph?.edges ?? view?.edges.map(edge => [edge.from, edge.to]) ?? []);
  async function refresh(selected = project) {
    if (!selected) return;
    candidatePreview=false;
    const serial = ++request;
    loading = true;
    error = '';
    view = null;
    try {
      const result = await load(selected.token);
      if (serial === request) view = result;
    } catch (failure) {
      if (serial === request) error = typeof failure === 'object' && failure && 'message' in failure ? String(failure.message) : String(failure);
    } finally { if (serial === request) loading = false; }
  }
  async function refreshOverview(revision:string) {
    const selected = project;
    if (!selected || (view?.authorityRevision ?? view?.revision) === revision) return;
    overviewTarget=revision;
    const serial = ++request; overviewError = '';
    try {
      const result = await load(selected.token);
      if (serial === request && project?.token === selected.token) {
        view = result;
        // A concurrent main update may be ahead of the triggering record.
        // Reread actual progress; never relax the exact revision boundary.
        if ((result.authorityRevision ?? result.revision) !== revision) liveRefresh++;
      }
    } catch (failure) {
      if (serial === request && project?.token === selected.token) overviewError = typeof failure === 'object' && failure && 'message' in failure ? String(failure.message) : String(failure);
    }
  }
  function selectProject(selected: SelectedProject) { overviewError = ''; overviewTarget=''; proposalReviewIntent=null;proposalReviewError='';activeHold = null; liveRefresh = 0; toolsOpen=false;tool='menu';showProject=false;menu=null; savedDraft = null; lastReadDraft=null; project = selected; demoMode = false; candidate = null; candidatePreview=false; void refresh(selected); }
  function showDemo() { overviewError = ''; overviewTarget=''; proposalReviewIntent=null;proposalReviewError='';activeHold = null; toolsOpen=false;tool='menu';showProject=false;menu=null; request++; savedDraft = null; lastReadDraft=null; project = null; view = null; candidate = null; candidatePreview=false; demoMode = true; error = ''; loading = false; showProject = false; }
  function openProposal(proposal:ProgressProposal){if(!project)return;proposalReviewError='';liveWorkspace?.dismiss();activeHold=null;toolsOpen=true;tool='draft';proposalReviewIntent={key:++proposalReviewSerial,binding:{...proposal.binding}};}
  function openHold(id:string,kind:string) { liveWorkspace?.dismiss();activeHold = {id,kind}; toolsOpen=false; }
</script>

<svelte:window onkeydown={keyboard} onpointerdown={(event) => {if(!(event.target as Element).closest('[role="menu"]'))menu=null;}} />
<!-- The window is the canvas; all other surfaces require an explicit action. -->
<main class="prototype-app canvas-app" aria-label="工程画布" oncontextmenu={context}>
  {#if !project && !demoMode}
    <section class="canvas-blank" aria-label="空画布">
      <button class="canvas-open-node" type="button" disabled={choosing} onclick={() => void openProject()}>{choosing?'正在打开…':'打开项目'}</button>
      {#if error}<p role="alert">{error}</p>{/if}
    </section>
  {:else if !project}
    <PrototypeCanvas />
  {:else if loading}
    <section class="canvas-blank" aria-label="项目画布"><p role="status">正在读取项目…</p></section>
  {:else if error}
    <section class="canvas-blank" aria-label="项目画布"><p role="alert">{error}</p><button type="button" onclick={() => refresh()}>重试读取</button></section>
  {:else if view}
    {#if project.hasConfig && !candidatePreview}
      {#key project.token}<LiveWorkspace bind:this={liveWorkspace} token={project.token} overview={view} client={taskProgressClient} {interventionClient} recheckClient={taskRecheckClient} onhold={openHold} onproposal={openProposal} onprojecttools={openTools} onrevision={(revision) => refreshOverview(revision)} refreshKey={liveRefresh}/>{/key}
    {:else}
      {#key candidatePreview ? candidate : view}<PrototypeCanvas {nodes} {edges} demo={false} candidate={candidatePreview}/>{/key}
    {/if}
  {/if}

  {#if menu}
    <div class="canvas-context-menu" role="menu" aria-label="画布菜单" style:left="{menu.x}px" style:top="{menu.y}px">
      <button role="menuitem" type="button" onclick={() => void openProject()}>{project?'切换项目':'打开项目'}</button>
      {#if project}
        <button role="menuitem" type="button" onclick={() => {menu=null;void refresh();}}>刷新画布</button>
        <button role="menuitem" type="button" onclick={() => {menu=null;liveWorkspace?.inspect();}}>态势与关注</button>
        <button role="menuitem" type="button" onclick={openTools}>项目工具</button>
      {/if}
      <button role="menuitem" type="button" onclick={() => {menu=null;showDemo();}}>查看演示</button>
    </div>
  {/if}

  {#if showProject}
    <aside class="canvas-inspector" aria-label="打开你的项目">
      <button class="canvas-close secondary" type="button" onclick={() => showProject=false}>关闭项目设置</button>
      <ProjectPicker {choose} onselected={selectProject} onview={(selected) => { if(project?.token!==selected.token)selectProject(selected);showProject=false;}}/>
    </aside>
  {/if}
  {#if toolsOpen && project}
    <aside class="canvas-inspector canvas-project-tools" aria-label="项目工具">
      <button class="canvas-close secondary" type="button" onclick={() => {toolsOpen=false;candidatePreview=false;savedDraft=null;proposalReviewIntent=null;}}>关闭项目工具</button>
      {#if tool==='menu'}
        <h2>{view?.title ?? '项目'}</h2><p>{project.displayPath}</p>
        <p>{view?.source==='baseline'?'权威 main 画布':'Git 结构预览 · 尚未批准为模块'} · <code>{view?.revision.slice(0,12)||'尚无提交'}</code></p>
        {#if overviewError}<p role="alert">基线画布同步失败：{overviewError}</p><button type="button" onclick={() => overviewTarget && refreshOverview(overviewTarget)}>重试同步画布</button>{/if}
        {#if proposalReviewError}<p role="alert">提案原件尚未打开：{proposalReviewError}</p>{/if}
        {#each view?.diagnostics ?? [] as diagnostic}<p>{diagnostic}</p>{/each}
        <button type="button" class="secondary" onclick={() => {toolsOpen=false;showProject=true;}}>项目设置与初始化</button>
        {#if project.hasConfig}
          <button type="button" class="secondary" onclick={() => tool='baselines'}>基线版本链</button>
          <button type="button" class="secondary" onclick={() => tool='draft'}>草稿与候选画布</button>
          <button type="button" class="secondary" onclick={() => tool='progress'}>任务记录诊断</button>
          <button type="button" class="secondary" onclick={() => tool='recovery'}>恢复本地 hub</button>
          <button type="button" class="secondary" onclick={() => tool='ask'}>本次问答</button>
          <button type="button" class="secondary" onclick={() => tool='review'}>任务评审</button>
        {/if}
      {:else}
        <button type="button" class="secondary" onclick={() => {tool='menu';candidatePreview=false;savedDraft=null;proposalReviewIntent=null;}}>返回项目工具</button>
        {#key project.token}
          {@const toolToken=project.token}
          {#if tool==='draft'}
            {#if !savedDraft}<CandidateScan token={project.token} client={scanClient} onloaded={(draft) => {candidate=draft;candidatePreview=true;}}/>{/if}
            {#if candidate && !candidatePreview && !savedDraft}<button type="button" class="secondary" onclick={()=>candidatePreview=true}>查看候选画布</button>{/if}
            {#if candidate}<section class="candidate-report" aria-label="共享验证报告"><details><summary>共享验证报告（{candidate.verification_report.executions.length} 次执行）</summary><p>{candidate.report_eid}</p><pre>{JSON.stringify(candidate.verification_report,null,2)}</pre></details></section>{/if}
            <DraftWorkspace token={project.token} client={savedDraftClient} currentClient={currentDraftClient} reviewIntent={proposalReviewIntent} reviewClient={proposalReviewClient} onreviewerror={(message)=>proposalReviewError=message} onreviewclosed={()=>liveRefresh++} candidateId={candidate?.scan_id} onloaded={(draft)=>{if(project?.token!==toolToken)return;savedDraft=draft;if(draft)lastReadDraft={token:toolToken,draft};}}/>
          {:else if tool==='baselines'}<BaselineHistory token={project.token} client={currentDraftClient} draft={lastReadDraft?.token===project.token?lastReadDraft.draft:null}/>
          {:else if tool==='progress'}<TaskProgressPanel token={project.token} client={taskProgressClient}/>
          {:else if tool==='recovery'}<RecoveryCard token={project.token} client={recoveryClient}/>
          {:else if tool==='ask'}<AskAnswerCard token={project.token} client={askAnswerClient}/>
          {:else if tool==='review'}<TaskReviewCard token={project.token} client={taskReviewClient}/>{/if}
        {/key}
      {/if}
    </aside>
  {/if}
  {#if project && activeHold}
    <aside class="canvas-inspector" aria-label="当前中断处理">
      <button type="button" class="canvas-close secondary" onclick={() => activeHold=null}>关闭中断卡片</button>
      {#key `${project.token}:${activeHold.id}`}
        {#if activeHold.kind==='ask'}<AskAnswerCard token={project.token} client={askAnswerClient} holdId={activeHold.id} onresolved={()=>liveRefresh++}/>
        {:else if activeHold.kind==='review'}<TaskReviewCard token={project.token} client={taskReviewClient} holdId={activeHold.id} onresolved={()=>liveRefresh++}/>{/if}
      {/key}
    </aside>
  {/if}
</main>
