<script lang="ts">
  import type { Overview } from '../lib/api'
  import { attentionSymbols, FIRST_SCREEN, when } from '../lib/attention'
  // FR-133: every project on this machine as one node, then one list of at most three across them.
  let { overview, current = null, disabled = false, onOpen }: {
    overview: Overview
    current?: string | null
    disabled?: boolean
    onOpen: (id: string) => void
  } = $props()
  const symbol = (label: string) => (attentionSymbols as Record<string, string>)[label] ?? '•'
  const attention = $derived(overview.attention)
</script>

<ul class="overview-projects" aria-label="本机项目">
  {#each overview.projects as project (project.id)}
    <li>
      <button class="overview-project health-{project.health?.color ?? 'unknown'}" class:is-current={project.id === current}
        disabled={disabled || !project.present || project.id === current} onclick={() => onOpen(project.id)}
        title={project.health?.reasons.join('；') || project.root}>
        <span class="overview-health">
          {#if project.health}<span aria-hidden="true">{project.health.symbol}</span> {project.health.label}
          {:else}<span aria-hidden="true">?</span> {project.present ? '状态未知' : '目录不在'}{/if}
        </span>
        <strong>{project.id}</strong>
        <span class="overview-meta">待处理 {project.needs ?? '—'} · 最近 {project.last_activity ? when(Date.parse(project.last_activity)) : '—'}</span>
        <span class="overview-root">{project.root}</span>
        {#if project.health?.reasons.length}<span class="overview-reasons">{project.health.reasons.join('；')}</span>{/if}
        {#if project.error}<span class="overview-reasons">{project.error}</span>{/if}
      </button>
    </li>
  {:else}
    <li class="panel-description">还没有打开过项目。</li>
  {/each}
</ul>

<h3>跨项目关注</h3>
{#if attention}
  <ol class="overview-attention">
    {#each attention.items.slice(0, FIRST_SCREEN) as item (`${item.project}:${item.id}`)}
      <li title={`${item.label} · ${item.title}`}>
        <span class="overview-tag">{item.project}</span>
        <span aria-hidden="true">{symbol(item.label)}</span> {item.card}
      </li>
    {:else}
      <li class="panel-description">没有需要你处理的事。</li>
    {/each}
  </ol>
  {#if attention.later}<p class="canvas-project-hint">另有 {attention.later} 件稍后</p>{/if}
{:else}
  <p class="panel-description">守护进程未运行，暂无跨项目关注。</p>
{/if}
