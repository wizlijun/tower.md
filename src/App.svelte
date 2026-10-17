<script lang="ts">
  import ProjectPicker from './components/ProjectPicker.svelte';
  import PrototypeCanvas from './components/PrototypeCanvas.svelte';
  import { chooseProject, loadProjectOverview, type ProjectOverview, type SelectedProject } from './lib/project';
  import { overviewNodes } from './lib/overview';
  let { choose = chooseProject, load = loadProjectOverview } = $props<{ choose?: typeof chooseProject; load?: typeof loadProjectOverview }>();
  let showProject = $state(false);
  let project = $state<SelectedProject | null>(null);
  let view = $state<ProjectOverview | null>(null);
  let loading = $state(false);
  let error = $state('');
  let request = 0;
  const nodes = $derived(view ? overviewNodes(view) : []);
  const edges = $derived(view?.edges.map(edge => [edge.from, edge.to]) ?? []);
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
  function selectProject(selected: SelectedProject) { project = selected; void refresh(selected); }
  function showDemo() { request++; project = null; view = null; error = ''; loading = false; showProject = false; }
</script>

<main class="prototype-app">
  <header class="app-header">
    <div class="brand"><span class="brand-mark" aria-hidden="true">T</span><h1>Tower<span>.md</span></h1></div>
    <div class="header-divider"></div><span class="workspace-name">{view?.title ?? '首版交付工作台'}</span><span class="prototype-badge">{project ? (view?.source === 'baseline' ? '项目画布 · 只读' : 'Git 目录预览') : '原型 · 演示数据'}</span>
    {#if project}<button type="button" class="header-action secondary" disabled={loading} onclick={() => refresh()}>刷新</button><button type="button" class="header-action secondary" onclick={showDemo}>演示项目</button>{/if}
    <button class="project-toggle secondary" type="button" aria-expanded={showProject} onclick={() => showProject = !showProject}>{showProject ? '返回画布' : project ? '切换项目' : '真实项目入口'}</button>
  </header>
  <section class="project-overlay" hidden={!showProject} aria-labelledby="project-heading">
    <h2 id="project-heading">打开你的项目</h2><p class="intro">选择本地 Git 项目即可查看画布，无需先初始化。未配置画布时显示固定提交的目录预览。</p>
    <ProjectPicker {choose} onselected={selectProject} onview={(selected) => { if (project?.token !== selected.token) selectProject(selected); showProject = false; }} />
  </section>
  <div class="workspace-heading"><div><p class="eyebrow">让 Agent 推进，你只在关键处放行</p><h2>{view?.title ?? (project ? '读取项目结构' : '看清全局，聚焦下一步')}<span class="heading-dot">.</span></h2></div><div class="project-summary">{#if project}<strong>{nodes.length}</strong> 个节点{:else}<strong>6</strong> 个节点<span> / </span><strong>2</strong> 个模块{/if}</div></div>
  {#if !project}
    <section class="attention-strip" aria-label="需要关注"><span class="attention-label"><span aria-hidden="true">△</span> 需要你关注</span><span class="attention-message">首版交付范围等待确认</span><span class="attention-demo">演示事项 · 1 项</span></section>
    <PrototypeCanvas />
  {:else if loading}
    <section class="workspace-message" role="status"><strong>正在读取项目…</strong><p>读取固定 Git 提交，不运行代码或验证命令。</p></section>
  {:else if error}
    <section class="workspace-message" role="alert"><strong>暂时无法读取项目</strong><p>{error}</p><button type="button" class="secondary" onclick={() => refresh()}>重试读取</button></section>
  {:else if view}
    <section class="source-strip" aria-label="项目来源"><span>{view.source === 'baseline' ? '权威 main 画布' : 'Git 结构预览 · 尚未批准为模块'}</span><code>{view.revision ? view.revision.slice(0, 12) : '尚无提交'}</code><span class="source-path">{project.displayPath}</span></section>
    {#if view.diagnostics.length}<details class="view-diagnostics"><summary>读取提示（{view.diagnostics.length}）</summary>{#each view.diagnostics as diagnostic}<p>{diagnostic}</p>{/each}</details>{/if}
    {#key view}<PrototypeCanvas {nodes} {edges} demo={false} />{/key}
  {/if}
  <footer class="app-footer"><span><span class="footer-dot"></span> 本地预览 · 不写入仓库</span><span>{project ? '刷新以读取最新提交 · 不包含未提交改动' : '演示关系图 · 打开真实项目即可切换'}</span></footer>
</main>
