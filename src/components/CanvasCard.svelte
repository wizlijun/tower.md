<script lang="ts">
  import { Handle, Position, type NodeProps } from '@xyflow/svelte'
  import { lowConfidence, type CardNode } from '../lib/graph'
  import { statusLabels } from '../lib/verification'
  import { objectTypeLabel, setByLabels } from '../lib/naming'
  import ProjectAttention from './ProjectAttention.svelte'
  import { rhythmLabels } from '../lib/follow-ups'

  let { data, draggable }: NodeProps<CardNode> = $props()
  const labels = { text: '文字', file: '文档', link: '链接', group: '容器' }
  let isProject = $derived(data.entry.kind === 'group' && data.entry.region === 'project')
  let isUnassigned = $derived(data.entry.kind === 'group' && data.entry.region === 'unassigned')
  // FR-20: shown only inside a candidate view (CSS scopes it); never moves the card.
  let low = $derived(lowConfidence(data.entry))
  const sides = [Position.Top, Position.Right, Position.Bottom, Position.Left]
  let preview = $derived(data.entry.content.replace(/^---\r?\n[\s\S]*?\r?\n---(?:\r?\n|$)/, '').trim())
</script>

<div class:canvas-card-group={data.entry.kind === 'group'} class:canvas-card-project={isProject} class:canvas-card-unassigned={isUnassigned} class:is-low-confidence={low !== null} class:is-quiet={data.rhythm === 'quiet'} class="canvas-card">
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
    <span class="canvas-card-kind">{isProject ? '项目' : isUnassigned ? '未划分区' : data.entry.object_type ? objectTypeLabel(data.entry.object_type) : labels[data.entry.kind]}{data.entry.set_by ? ` · ${setByLabels[data.entry.set_by]}` : ''}</span>
    <strong>{data.entry.title}</strong>
    {#if low !== null}
      <span class="verification-status confidence-mark"><span aria-hidden="true">◌</span> 置信度 {Math.round(low * 100)}% · 低</span>
    {/if}
    {#if data.rhythm}
      <span class={`rhythm-mark rhythm-${data.rhythm}`}><span aria-hidden="true">{rhythmLabels[data.rhythm].symbol}</span> {rhythmLabels[data.rhythm].text}</span>
    {/if}
    {#if data.status}
      <span class={`verification-status verification-${data.status}`}><span aria-hidden="true">{statusLabels[data.status].symbol}</span> {statusLabels[data.status].label}</span>
    {/if}
    {#if data.activity}
      <span class={`verification-status verification-${data.activity.tone}`}><span aria-hidden="true">{data.activity.symbol}</span> {data.activity.label}</span>
    {/if}
    {#if isProject && data.attention !== undefined}
      <ProjectAttention items={data.attention} backlog={data.backlog} later={data.later} />
    {/if}
    {#if data.entry.kind !== 'group'}
      <span class="canvas-card-preview">{preview || data.entry.file || data.entry.url || ''}</span>
    {/if}
  </button>
</div>
