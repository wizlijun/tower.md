<script lang="ts">
  import type { CanvasNode, ProjectView } from '../lib/api'
  import { statusLabels, type CardStatus } from '../lib/verification'
  let { view, onOpen, fitVersion, statuses = {} }: { view: ProjectView; statuses?: Record<string, CardStatus>; fitVersion?: number; onOpen: (node: CanvasNode) => void } = $props()
</script>

<section aria-label="项目画布" data-fit-version={fitVersion}>
  {#each view.nodes as node}
    <button data-x={node.x} data-y={node.y} onclick={() => onOpen(node)}>查看 {node.title}</button>
    {#if statuses[node.object_id || node.id]}<span aria-label={`${node.title} 验证状态`}>{statusLabels[statuses[node.object_id || node.id]].label}</span>{/if}
  {/each}
</section>
