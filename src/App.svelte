<script lang="ts">
  import ProjectPicker from './components/ProjectPicker.svelte';
  import PrototypeCanvas from './components/PrototypeCanvas.svelte';
  let showProject = $state(false);
</script>

<main class="prototype-app">
  <header class="app-header">
    <div class="brand"><span class="brand-mark" aria-hidden="true">T</span><h1>Tower<span>.md</span></h1></div>
    <div class="header-divider"></div><span class="workspace-name">首版交付工作台</span><span class="prototype-badge">原型 · 演示数据</span>
    <button class="project-toggle secondary" type="button" aria-expanded={showProject} onclick={() => showProject = !showProject}>{showProject ? '返回画布' : '真实项目入口'}</button>
  </header>
  {#if showProject}
    <section class="project-overlay" aria-labelledby="project-heading">
      <h2 id="project-heading">打开你的项目</h2><p class="intro">桌面 App 可选择并初始化本地 Git 项目。真实项目的画布数据接线尚未完成。</p>
      <ProjectPicker />
    </section>
  {/if}
  <div class="workspace-heading"><div><p class="eyebrow">让 Agent 推进，你只在关键处放行</p><h2>看清全局，聚焦下一步<span class="heading-dot">.</span></h2></div><div class="project-summary"><strong>6</strong> 个节点<span> / </span><strong>2</strong> 个模块</div></div>
  <section class="attention-strip" aria-label="需要关注">
    <span class="attention-label"><span aria-hidden="true">△</span> 需要你关注</span>
    <span class="attention-message">首版交付范围等待确认</span><span class="attention-demo">演示事项 · 1 项</span>
  </section>
  <PrototypeCanvas />
  <footer class="app-footer"><span><span class="footer-dot"></span> 本地原型 · 不写入仓库</span><span>演示关系图 · 尚未连接真实项目状态</span></footer>
</main>
