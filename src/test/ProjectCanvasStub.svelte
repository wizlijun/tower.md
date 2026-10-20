<script lang="ts">
  import type { CanvasNode, ProjectView } from '../lib/api'
  import { statusLabels, type CardStatus } from '../lib/verification'
  import type { ActivityBadge } from '../lib/runtime'
  let { view, onOpen, fitVersion, statuses = {}, activity = {}, editable = false, onMove }: { view: ProjectView; editable?: boolean; onMove?: (id: string, x: number, y: number) => void; statuses?: Record<string, CardStatus>; activity?: Record<string, ActivityBadge>; fitVersion?: number; onOpen: (node: CanvasNode) => void } = $props()
</script>

<section aria-label="项目画布" data-fit-version={fitVersion}>
  {#each view.nodes as node}
    <button data-x={node.x} data-y={node.y} onclick={() => onOpen(node)}>查看 {node.title}</button>
    {#if statuses[node.object_id || node.id]}<span aria-label={`${node.title} 验证状态`}>{statusLabels[statuses[node.object_id || node.id]].label}</span>{/if}
    {#if activity[node.object_id || node.id]}<span aria-label={`${node.title} 任务状态`}>{activity[node.object_id || node.id].symbol} {activity[node.object_id || node.id].label}</span>{/if}
    {#if editable}<button onclick={() => onMove?.(node.id, node.x + 100.4, node.y + 49.6)}>移动 {node.title}</button>{/if}
  {/each}
</section>
