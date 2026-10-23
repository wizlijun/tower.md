<script lang="ts">
  import { Background, BackgroundVariant, SvelteFlow } from '@xyflow/svelte'
  import '@xyflow/svelte/dist/style.css'
  import type { CanvasNode, ProjectView } from '../lib/api'
  import { toFlowEdges, toFlowNodes, type CardNode } from '../lib/graph'
  import CanvasCard from './CanvasCard.svelte'
  import type { CardStatus } from '../lib/verification'
  import type { ActivityBadge, RuntimeAttention } from '../lib/runtime'

  let { view, fitVersion = 0, statuses = {}, activity = {}, attention, backlog, budget, editable = false, onMove, onOpen }: {
    view: ProjectView
    fitVersion?: number
    statuses?: Record<string, CardStatus>
    activity?: Record<string, ActivityBadge>
    attention?: RuntimeAttention[]
    backlog?: number
    /** First-screen attention items on the project card (FR-128). */
    budget?: number
    editable?: boolean
    onMove?: (id: string, x: number, y: number) => void
    onOpen: (node: CanvasNode) => void
  } = $props()

  const nodeTypes = { card: CanvasCard }
  const nodeCache = { fitVersion: undefined as number | undefined, nodes: new Map<string, CardNode>() }
  let nodes = $derived.by(() => {
    if (nodeCache.fitVersion !== fitVersion) {
      nodeCache.nodes.clear()
      nodeCache.fitVersion = fitVersion
    }
    return toFlowNodes(view.nodes, onOpen, statuses, editable, activity, nodeCache.nodes, attention, backlog, budget)
  })
  let viewport = $state({ x: 0, y: 0, zoom: 1 })
  export function canvasPoint(x: number, y: number) {
    return { x: (x - viewport.x) / viewport.zoom, y: (y - viewport.y) / viewport.zoom }
  }
  let edges = $derived(toFlowEdges(view.edges))
</script>

{#key fitVersion}
  <SvelteFlow
    {nodes}
    {edges}
    {nodeTypes}
    fitView
    fitViewOptions={{ padding: 0.2, maxZoom: 1 }}
    nodesDraggable={editable}
    nodeDragThreshold={0}
    bind:viewport
    onnodedragstop={({ targetNode }) => { if (editable && targetNode) onMove?.(targetNode.id, targetNode.position.x, targetNode.position.y) }}
    nodesConnectable={false}
    nodesFocusable={false}
    edgesFocusable={false}
    elementsSelectable={false}
    deleteKey={null}
    panOnDrag
    zoomOnScroll
    zoomOnPinch
    zoomOnDoubleClick={false}
    minZoom={0.1}
    maxZoom={3}
    proOptions={{ hideAttribution: true }}
  >
    <Background variant={BackgroundVariant.Dots} gap={24} size={1} patternColor="#d7d9d3" />
  </SvelteFlow>
{/key}
