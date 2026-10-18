<script lang="ts">
  import { BaseEdge, EdgeLabel, getBezierPath, type EdgeProps } from '@xyflow/svelte';
  import type { FlowEdge } from '../lib/canvas-flow';
  let { id, sourceX, sourceY, sourcePosition, targetX, targetY, targetPosition, label, data, markerStart, markerEnd, selected, interactionWidth }: EdgeProps<FlowEdge> = $props();
  const path = $derived(getBezierPath({sourceX,sourceY,sourcePosition,targetX,targetY,targetPosition}));
</script>
<BaseEdge {id} path={path[0]} {markerStart} {markerEnd} {interactionWidth} />
{#if label || selected || data?.binding}<EdgeLabel x={path[1]} y={path[2]} selectEdgeOnClick><span class="flow-edge-label">{typeof label === 'string' ? label : ''}{data?.binding ? ` · 接口 ${data.binding}` : ''}</span></EdgeLabel>{/if}
