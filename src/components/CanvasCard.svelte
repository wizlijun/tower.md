<script lang="ts">
  import { Handle, Position, type NodeProps } from '@xyflow/svelte'
  import type { CardNode } from '../lib/graph'
  import { statusLabels } from '../lib/verification'
  import { objectTypeLabel, setByLabels } from '../lib/naming'
  import ProjectAttention from './ProjectAttention.svelte'

  let { data, draggable }: NodeProps<CardNode> = $props()
  const labels = { text: '文字', file: '文档', link: '链接', group: '容器' }
  let isProject = $derived(data.entry.kind === 'group' && data.entry.region === 'project')
  const sides = [Position.Top, Position.Right, Position.Bottom, Position.Left]
  let preview = $derived(data.entry.content.replace(/^---\r?\n[\s\S]*?\r?\n---(?:\r?\n|$)/, '').trim())
</script>

<div class:canvas-card-group={data.entry.kind === 'group'} class:canvas-card-project={isProject} class="canvas-card">
  {#each sides as position}
    <Handle id={`in-${position}`} type="target" {position} isConnectable={false} aria-hidden="true" />
    <Handle id={`out-${position}`} type="source" {position} isConnectable={false} aria-hidden="true" />
  {/each}
  {#if draggable}<div class="canvas-card-drag" title="拖动卡片位置" aria-hidden="true">⠿</div>{/if}
  <button
    class="canvas-card-open nodrag"
    aria-label={`查看 ${data.entry.title}`}
    onclick={() => data.onOpen?.(data.entry)}
  >
    <span class="canvas-card-kind">{isProject ? '项目' : data.entry.object_type ? objectTypeLabel(data.entry.object_type) : labels[data.entry.kind]}{data.entry.set_by ? ` · ${setByLabels[data.entry.set_by]}` : ''}</span>
    <strong>{data.entry.title}</strong>
    {#if data.status}
      <span class={`verification-status verification-${data.status}`}><span aria-hidden="true">{statusLabels[data.status].symbol}</span> {statusLabels[data.status].label}</span>
    {/if}
    {#if data.activity}
      <span class={`verification-status verification-${data.activity.tone}`}><span aria-hidden="true">{data.activity.symbol}</span> {data.activity.label}</span>
    {/if}
    {#if isProject && data.attention !== undefined}
      <ProjectAttention items={data.attention} backlog={data.backlog} />
    {/if}
    {#if data.entry.kind !== 'group'}
      <span class="canvas-card-preview">{preview || data.entry.file || data.entry.url || ''}</span>
    {/if}
  </button>
</div>
