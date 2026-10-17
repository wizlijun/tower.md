<script lang="ts">
  import ProjectPicker from './components/ProjectPicker.svelte';
</script>

<main class="welcome">
  <section class="project-entry" aria-labelledby="app-title">
    <p class="eyebrow">本地优先 · 工程驾驶舱</p>
    <h1 id="app-title">Tower.md</h1>
    <p class="intro">从你的 Git 项目开始。</p>
    <ProjectPicker />
  </section>
</main>
