<script lang="ts">
  import { chooseProject, initializeProject, type AppError, type InitOutput, type SelectedProject } from '../lib/project';
  let { choose = chooseProject, initialize = initializeProject } = $props<{
    choose?: typeof chooseProject; initialize?: typeof initializeProject;
  }>();
  let project = $state<SelectedProject | null>(null);
  let docsDir = $state('docs/tower');
  let confirmedDirs = $state<Record<string, string>>({});
  let choosing = $state(false);
  let initializing = $state(false);
  let initialized = $state(false);
  let result = $state<InitOutput | null>(null);
  let error = $state<AppError | null>(null);
  let selection = 0;
  const confirmedDir = $derived(project ? confirmedDirs[project.displayPath] : undefined);

  function appError(value: unknown): AppError {
    if (typeof value === 'object' && value !== null && 'message' in value) {
      const failure = value as Partial<AppError>;
      return { code: failure.code ?? 'app_error', message: String(failure.message), detail: failure.detail ?? null };
    }
    return { code: 'app_error', message: String(value), detail: null };
  }
  async function openProject() {
    if (choosing || initializing) return;
    const request = ++selection;
    choosing = true;
    try {
      const selected = await choose();
      if (request !== selection || initializing || !selected) return;
      project = selected;
      docsDir = 'docs/tower';
      initialized = false;
      result = null;
      error = null;
    } catch (failure) {
      if (request === selection && !initializing) error = appError(failure);
    } finally {
      if (request === selection) choosing = false;
    }
  }
  async function initProject() {
    if (!project || choosing || initializing || project.hasConfig || initialized) return;
    const directory = confirmedDir ?? docsDir;
    if (!directory.trim()) return;
    const selected = project;
    const request = ++selection;
    confirmedDirs = { ...confirmedDirs, [selected.displayPath]: directory };
    initializing = true;
    result = null;
    error = null;
    try {
      const output = await initialize(selected.token, directory);
      if (request !== selection || project?.token !== selected.token) return;
      result = output;
      initialized = true;
    } catch (failure) {
      if (request === selection && project?.token === selected.token) error = appError(failure);
    } finally {
      if (request === selection) initializing = false;
    }
  }
  function changeDirectory() {
    if (!project || initializing) return;
    docsDir = confirmedDir ?? docsDir;
    const updated = { ...confirmedDirs };
    delete updated[project.displayPath];
    confirmedDirs = updated;
  }
</script>

<button type="button" disabled={choosing || initializing} onclick={openProject}>{project ? '切换项目' : '打开项目'}</button>
{#if project}
  <p class="project-path">{project.displayPath}</p>
  {#if project.hasConfig}
    <p class="status">检测到配置；权威状态尚未接入。</p>
  {:else if initialized}
    <p class="status">文件未提交，请检查后自行提交到 main。生成文件不代表已批准基线；权威状态尚未接入。</p>
  {:else}
    <div class="init-guide">
      {#if confirmedDir !== undefined}
        <p class="status">本会话已确认文档目录：</p>
        <p class="project-path">{confirmedDir}</p>
        <button class="secondary" type="button" disabled={initializing} onclick={changeDirectory}>修改文档目录</button>
      {:else}
        <label for="docs-dir">文档目录</label>
        <input id="docs-dir" type="text" bind:value={docsDir} disabled={initializing} aria-describedby="docs-help" />
      {/if}
      <p id="docs-help" class="status">使用仓库内的相对目录，建议 docs/tower。其下 intents、arch、specs、baselines、state 将成为受保护路径，只能经人批准修改。</p>
      <button type="button" disabled={choosing || initializing || !(confirmedDir ?? docsDir).trim()} onclick={initProject}>{initializing ? '正在初始化…' : '初始化项目'}</button>
    </div>
  {/if}
{/if}
{#if result}
  <section class="diagnostics" aria-label="初始化输出">
    <p class="status">CLI stdout（含警告与提示）</p>
    <pre>{result.stdout}</pre>
    {#if result.stderr}<p class="status">CLI stderr</p><pre>{result.stderr}</pre>{/if}
  </section>
{/if}
{#if error}
  <section class="diagnostics" role="alert">
    <p>{error.message}</p>
    <p class="status">{error.code}</p>
    {#if error.detail}<pre>{error.detail}</pre>{/if}
  </section>
{/if}
