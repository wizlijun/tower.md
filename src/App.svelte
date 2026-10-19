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
  import type { DraftClient, SavedDraft } from './lib/draft';
  import type { CurrentDraftClient } from './lib/current-draft';
  import { candidateGraph, type CandidateClient, type CandidateDraft } from './lib/candidate';
  import { chooseProject, loadProjectOverview, type ProjectOverview, type SelectedProject } from './lib/project';
  import { overviewNodes } from './lib/overview';
  let { choose = chooseProject, load = loadProjectOverview, scanClient, savedDraftClient, currentDraftClient, recoveryClient, taskReviewClient, askAnswerClient, taskProgressClient, interventionClient = defaultInterventionClient, taskRecheckClient, proposalReviewClient } = $props<{ choose?: typeof chooseProject; load?: typeof loadProjectOverview; scanClient?: CandidateClient; savedDraftClient?: DraftClient; currentDraftClient?: CurrentDraftClient; recoveryClient?: RecoveryClient; taskReviewClient?: TaskReviewClient; askAnswerClient?: AskAnswerClient; taskProgressClient?: TaskProgressClient; interventionClient?: InterventionClient; taskRecheckClient?: TaskRecheckClient; proposalReviewClient?: ReviewClient }>();
  let showProject = $state(false);
  let showProgress = $state(false);
  let showRecovery = $state(false);
  let showTaskReview = $state(false);
  let showAskAnswer = $state(false);
  let proposalReviewIntent = $state<ProposalReviewIntent | null>(null); let proposalReviewError = $state(''); let proposalReviewSerial = 0;
  let activeHold = $state<{id:string;kind:string}|null>(null);
  let liveRefresh = $state(0);
  let project = $state<SelectedProject | null>(null);
  let view = $state<ProjectOverview | null>(null);
  let loading = $state(false);
  let error = $state('');
  let overviewError = $state('');
  let demoMode = $state(false);
  let candidate = $state<CandidateDraft | null>(null);
  let savedDraft = $state<SavedDraft | null>(null);
  let request = 0;
  const graph = $derived(candidate ? candidateGraph(candidate) : null);
  const nodes = $derived(graph?.nodes ?? (view ? overviewNodes(view) : []));
  const edges = $derived(graph?.edges ?? view?.edges.map(edge => [edge.from, edge.to]) ?? []);
  async function refresh(selected = project) {
    if (!selected) return;
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
  function selectProject(selected: SelectedProject) { overviewError = ''; proposalReviewIntent=null;proposalReviewError='';activeHold = null; liveRefresh = 0; showProgress = false; showAskAnswer = false; showTaskReview = false; showRecovery = false; savedDraft = null; project = selected; demoMode = false; candidate = null; void refresh(selected); }
  function showDemo() { overviewError = ''; proposalReviewIntent=null;proposalReviewError='';activeHold = null; showProgress = false; showAskAnswer = false; showTaskReview = false; showRecovery = false; request++; savedDraft = null; project = null; view = null; candidate = null; demoMode = true; error = ''; loading = false; showProject = false; }
  function openProposal(proposal:ProgressProposal){if(!project)return;proposalReviewError='';activeHold=null;proposalReviewIntent={key:++proposalReviewSerial,binding:{...proposal.binding}};}
  function openHold(id:string,kind:string) { activeHold = {id,kind}; showAskAnswer = false; showTaskReview = false; }
</script>

<main class="prototype-app" class:live-app={project !== null && !savedDraft && !candidate}>
  <header class="app-header">
    <div class="brand"><span class="brand-mark" aria-hidden="true">T</span><h1>Tower<span>.md</span></h1></div>
    <div class="header-divider"></div><span class="workspace-name">{view?.title ?? '本地工程驾驶舱'}</span><span class="prototype-badge">{savedDraft ? '已保存草稿 · 不代表当前基线' : candidate ? '真实候选 · 未批准' : project ? (view?.source === 'baseline' ? '真实项目 · 实时态势' : 'Git 目录预览') : demoMode ? '原型 · 演示数据' : '本地优先'}</span>
    {#if project}<button type="button" class="header-action secondary" disabled={loading} onclick={() => refresh()}>刷新</button><button type="button" class="header-action secondary" onclick={showDemo}>演示项目</button>{/if}
    <button class="project-toggle secondary" type="button" aria-expanded={showProject} onclick={() => showProject = !showProject}>{showProject ? '返回画布' : project ? '切换项目' : '真实项目入口'}</button>
  </header>
  <section class="project-overlay" hidden={!showProject} aria-labelledby="project-heading">
    <h2 id="project-heading">打开你的项目</h2><p class="intro">选择本地 Git 项目即可查看画布，无需先初始化。未配置画布时显示固定提交的目录预览。</p>
    <ProjectPicker {choose} onselected={selectProject} onview={(selected) => { if (project?.token !== selected.token) selectProject(selected); showProject = false; }} />
  </section>
  <div class="workspace-heading"><div><p class="eyebrow">让 Agent 推进，你只在关键处放行</p><h2>{view?.title ?? (project ? '读取项目结构' : '打开你的真实项目')}<span class="heading-dot">.</span></h2></div><div class="project-summary">{#if project}<strong>{savedDraft?.canvas_view.nodes.length ?? nodes.length}</strong> 个节点{:else if demoMode}<strong>6</strong> 个节点<span> / </span><strong>2</strong> 个模块{/if}</div></div>
  {#if proposalReviewError}<p class="live-diagnostic" role="alert">提案原件尚未打开：{proposalReviewError}</p>{/if}
  {#if overviewError}<p class="live-diagnostic" role="alert">基线画布同步失败：{overviewError}。旧图不会叠加新版本状态，可在态势区重试同步。</p>{/if}
  {#if !savedDraft}
  {#if !project && !demoMode}
    <section class="workspace-message"><strong>从一个本地 Git 项目开始</strong><p>无需初始化即可只读预览目录。已初始化项目可在你点击后生成真实候选画布。</p><button type="button" onclick={() => showProject = true}>打开真实项目</button><button type="button" class="secondary" onclick={showDemo}>查看演示</button></section>
  {:else if !project}
    <section class="attention-strip" aria-label="需要关注"><span class="attention-label"><span aria-hidden="true">△</span> 需要你关注</span><span class="attention-message">首版交付范围等待确认</span><span class="attention-demo">演示事项 · 1 项</span></section>
    <PrototypeCanvas />
  {:else if loading}
    <section class="workspace-message" role="status"><strong>正在读取项目…</strong><p>读取固定 Git 提交，不运行代码或验证命令。</p></section>
  {:else if error}
    <section class="workspace-message" role="alert"><strong>暂时无法读取项目</strong><p>{error}</p><button type="button" class="secondary" onclick={() => refresh()}>重试读取</button></section>
  {:else if view}
    <section class="source-strip" aria-label="项目来源"><span>{candidate ? '真实候选 · 未批准' : view.source === 'baseline' ? '权威 main 画布' : 'Git 结构预览 · 尚未批准为模块'}</span><code>{(candidate?.input.revision ?? view.revision)?.slice(0, 12) || '尚无提交'}</code><span class="source-path">{project.displayPath}</span></section>
    {#if candidate}<section class="candidate-report" aria-label="共享验证报告"><details><summary>共享验证报告（{candidate.verification_report.executions.length} 次执行）</summary><p>报告 ID：{candidate.report_eid}</p><pre>{JSON.stringify(candidate.verification_report, null, 2)}</pre></details></section>{/if}
    {#if view.diagnostics.length}<details class="view-diagnostics"><summary>读取提示（{view.diagnostics.length}）</summary>{#each view.diagnostics as diagnostic}<p>{diagnostic}</p>{/each}</details>{/if}
    {#if graph?.diagnostics.length}<details class="view-diagnostics"><summary>候选画布提示（{graph.diagnostics.length}）</summary>{#each graph.diagnostics as diagnostic}<p>{diagnostic}</p>{/each}</details>{/if}
    {#if project.hasConfig && !candidate}{#key project.token}<LiveWorkspace token={project.token} overview={view} client={taskProgressClient} {interventionClient} recheckClient={taskRecheckClient} onhold={openHold} onproposal={openProposal} onrevision={(revision) => refreshOverview(revision)} refreshKey={liveRefresh}/>{/key}
    {:else}{#key candidate ?? view}<PrototypeCanvas {nodes} {edges} demo={false} candidate={candidate !== null} />{/key}{/if}
  {/if}
  {/if}
  {#if project?.hasConfig}<section class="project-tools" aria-label="项目工具与诊断"><h3>项目工具与诊断</h3>
  {#if project?.hasConfig}<section class="task-progress-entry"><button type="button" class="secondary" aria-expanded={showProgress} onclick={()=>showProgress=!showProgress}>{showProgress?'收起记录诊断':'任务记录诊断'}</button>{#if showProgress}{#key project.token}<TaskProgressPanel token={project.token} client={taskProgressClient}/>{/key}{/if}</section>{/if}
  {#if project?.hasConfig}<section class="recovery-entry"><button type="button" class="secondary" aria-expanded={showRecovery} onclick={()=>showRecovery=!showRecovery}>{showRecovery ? '收起恢复请求' : '恢复本地 hub'}</button>{#if showRecovery}{#key project.token}<RecoveryCard token={project.token} client={recoveryClient}/>{/key}{/if}</section>{/if}
  {#if project?.hasConfig}<section class="ask-answer-entry"><button type="button" class="secondary" aria-expanded={showAskAnswer} onclick={()=>showAskAnswer=!showAskAnswer}>{showAskAnswer ? '收起本次问答' : '本次问答'}</button>{#if showAskAnswer}{#key project.token}<AskAnswerCard token={project.token} client={askAnswerClient}/>{/key}{/if}</section>{/if}
  {#if project?.hasConfig}<section class="task-review-entry"><button type="button" class="secondary" aria-expanded={showTaskReview} onclick={()=>showTaskReview=!showTaskReview}>{showTaskReview ? '收起任务评审' : '任务评审'}</button>{#if showTaskReview}{#key project.token}<TaskReviewCard token={project.token} client={taskReviewClient}/>{/key}{/if}</section>{/if}
  {#if project?.hasConfig && activeHold}<aside class="live-hold-drawer" aria-label="当前中断处理"><button type="button" class="secondary" onclick={() => activeHold = null}>关闭中断卡片</button>{#key `${project.token}:${activeHold.id}`} {#if activeHold.kind === 'ask'}<AskAnswerCard token={project.token} client={askAnswerClient} holdId={activeHold.id} onresolved={() => liveRefresh++}/>{:else if activeHold.kind === 'review'}<TaskReviewCard token={project.token} client={taskReviewClient} holdId={activeHold.id} onresolved={() => liveRefresh++}/>{/if}{/key}</aside>{/if}
  {#if project?.hasConfig}{#key project.token}<div hidden={savedDraft!==null}><CandidateScan token={project.token} client={scanClient} onloaded={(draft) => candidate = draft} /></div>{/key}{/if}
  {#if project?.hasConfig}{#key project.token}<DraftWorkspace token={project.token} client={savedDraftClient} currentClient={currentDraftClient} reviewIntent={proposalReviewIntent} reviewClient={proposalReviewClient} onreviewerror={(message)=>proposalReviewError=message} onreviewclosed={()=>liveRefresh++} candidateId={candidate?.scan_id} onloaded={(draft)=>savedDraft=draft}/>{/key}{/if}
  </section>{/if}
  <footer class="app-footer"><span><span class="footer-dot"></span> {showAskAnswer ? '本次问答 · 明确提交才记录' : showTaskReview ? '任务评审 · 明确提交才记录' : showRecovery ? 'Hub 恢复 · 明确确认后执行' : savedDraft ? '草稿编辑 · 批准与出版另行确认' : candidate ? '候选只读 · 尚未批准' : '基线只读 · 快层叠加实时记录'}</span><span>{showAskAnswer ? '本次回答 · 不生成长期决策或批准' : showTaskReview ? '绑定原计划或 tree · 不自动继续' : project ? '快层变化保持节点坐标' : demoMode ? '演示数据' : '等待选择真实项目'}</span></footer>
</main>
