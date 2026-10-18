<script lang="ts">
  import { BaseEdge, EdgeLabel, getBezierPath, type EdgeProps } from '@xyflow/svelte';
  import type { FlowEdge } from '../lib/canvas-flow';
  let { id, sourceX, sourceY, sourcePosition, targetX, targetY, targetPosition, label, data, markerStart, markerEnd, selected, interactionWidth, style, class:className }: EdgeProps<FlowEdge> = $props();
  const path = $derived(getBezierPath({sourceX,sourceY,sourcePosition,targetX,targetY,targetPosition}));
</script>
<BaseEdge {id} path={path[0]} {markerStart} {markerEnd} {interactionWidth} {style} class={className} />
{#if label || selected || data?.binding || data?.reviewLayer}<EdgeLabel x={path[1]} y={path[2]} selectEdgeOnClick><span class="flow-edge-label">{data?.reviewLayer ? (data.reviewLayer === 'baseline' ? '━ 主干 · ' : '┄ 拟议 · ') : ''}{typeof label === 'string' ? label : ''}{data?.binding ? ` · 接口 ${data.binding}` : ''}</span></EdgeLabel>{/if}
