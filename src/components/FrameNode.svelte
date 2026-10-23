<script lang="ts">
  import { NodeResizer, type NodeProps } from '@xyflow/svelte'
  import { FRAME_MAX_SIZE, FRAME_MIN_SIZE, frameLanes, type FrameNode } from '../lib/frames'
  import { setByLabels } from '../lib/naming'

  let { data, draggable, width, height }: NodeProps<FrameNode> = $props()
  let frame = $derived(data.frame)
  let lanes = $derived(frameLanes(frame.layout, width ?? frame.place.width, height ?? frame.place.height))
</script>

<!-- FR-141–146: a frame sits behind every card; lanes are drawn, never laid out from the fast layer. -->
<NodeResizer isVisible={draggable} minWidth={FRAME_MIN_SIZE} minHeight={FRAME_MIN_SIZE} maxWidth={FRAME_MAX_SIZE} maxHeight={FRAME_MAX_SIZE}
  lineClass="frame-node-resize" handleClass="frame-node-handle"
  onResizeEnd={(_, place) => data.onPlace?.(frame.id, { x: Math.round(place.x), y: Math.round(place.y), width: Math.round(place.width), height: Math.round(place.height) })} />
<section class="frame-node" aria-label={`容器 ${frame.title}`}>
  {#each lanes.columns as lane, index (index)}
    <div class="frame-node-lane frame-node-column" style={`left: ${lane.start}px; width: ${lane.size}px;`}><span>{lane.label}</span></div>
  {/each}
  {#each lanes.rows as lane, index (index)}
    <div class="frame-node-lane frame-node-row" style={`top: ${lane.start}px; height: ${lane.size}px;`}><span>{lane.label}</span></div>
  {/each}
  <header class="frame-node-head" class:is-draggable={draggable} title={draggable ? '拖动容器位置' : undefined}>
    {#if draggable}<span class="frame-node-grip" aria-hidden="true">⠿</span>{/if}
    <strong>{frame.title}</strong>
    <span class="frame-node-meta">容器 · {frame.template.id}@{frame.template.version} · {setByLabels[frame.set_by]}</span>
    {#if data.badge}
      <span class={`frame-node-badge frame-node-badge-${data.badge.tone}`} role="status">
        <span aria-hidden="true">{data.badge.tone === 'warn' ? '▲' : '●'}</span> {data.badge.label}
      </span>
    {/if}
  </header>
  {#if frame.rules.text}<p class="frame-node-rules">规则：{frame.rules.text}</p>{/if}
</section>
