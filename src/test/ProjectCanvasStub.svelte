<script lang="ts">
  import type { CanvasNode, FrameEntry, FramePlace, ProjectView } from '../lib/api'
  import type { FrameActions, FrameBadge } from '../lib/frames'
  import { statusLabels, type CardStatus } from '../lib/verification'
  import type { ActivityBadge, RuntimeAttention } from '../lib/runtime'
  import ProjectAttention from '../components/ProjectAttention.svelte'
  let { view, onOpen, fitVersion, statuses = {}, activity = {}, attention, editable = false, onMove, frames = [], frameBadges = {}, framesEditable = false, onPlaceFrame, frameActions }: { view: ProjectView; editable?: boolean; onMove?: (id: string, x: number, y: number) => void; statuses?: Record<string, CardStatus>; activity?: Record<string, ActivityBadge>; attention?: RuntimeAttention[]; fitVersion?: number; onOpen: (node: CanvasNode) => void; frames?: FrameEntry[]; frameBadges?: Record<string, FrameBadge>; framesEditable?: boolean; onPlaceFrame?: (id: string, place: FramePlace) => void; frameActions?: FrameActions } = $props()
  export function canvasPoint(x: number, y: number) { return { x, y } }
</script>

<section aria-label="项目画布" data-fit-version={fitVersion}>
  {#each frames as frame (frame.id)}
    <div role="group" aria-label={`容器 ${frame.title}`} data-place={`${frame.place.x},${frame.place.y},${frame.place.width},${frame.place.height}`}>
      {#if frameBadges[frame.id]}<span>{frameBadges[frame.id].label}</span>{/if}
      {#if framesEditable}<button onclick={() => onPlaceFrame?.(frame.id, { ...frame.place, x: frame.place.x + 200 })}>移动容器 {frame.title}</button>{/if}
      {#each frame.cards ?? [] as card (card.id)}<span>{card.title}</span>{/each}
      {#if framesEditable && frameActions}
        <button onclick={() => frameActions?.card?.(frame.id, frame.id, { op: 'add', title: '人加的' })}>加卡片到 {frame.title}</button>
        {#each frame.pending ?? [] as item (item.id)}<button onclick={() => frameActions?.resolve?.(frame.id, item.id, true)}>接受 {item.id}</button>{/each}
      {/if}
    </div>
  {/each}
  {#each view.nodes as node}
    <button data-x={node.x} data-y={node.y} onclick={() => onOpen(node)}>查看 {node.title}</button>
    {#if statuses[node.object_id || node.id]}<span aria-label={`${node.title} 验证状态`}>{statusLabels[statuses[node.object_id || node.id]].label}</span>{/if}
    {#if activity[node.object_id || node.id]}<span aria-label={`${node.title} 任务状态`}>{activity[node.object_id || node.id].symbol} {activity[node.object_id || node.id].label}</span>{/if}
    {#if node.kind === 'group' && node.region === 'project' && attention !== undefined}<ProjectAttention items={attention} />{/if}
    {#if editable}<button onclick={() => onMove?.(node.id, node.x + 100.4, node.y + 49.6)}>移动 {node.title}</button>{/if}
  {/each}
</section>
