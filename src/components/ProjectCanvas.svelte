<script lang="ts">
  import { Background, BackgroundVariant, SvelteFlow } from '@xyflow/svelte'
  import '@xyflow/svelte/dist/style.css'
  import type { CanvasNode, ProjectView } from '../lib/api'
  import { toFlowEdges, toFlowNodes } from '../lib/graph'
  import CanvasCard from './CanvasCard.svelte'
  import type { CardStatus } from '../lib/verification'

  let { view, fitVersion = 0, statuses = {}, onOpen }: {
    view: ProjectView
    fitVersion?: number
    statuses?: Record<string, CardStatus>
    onOpen: (node: CanvasNode) => void
  } = $props()

  const nodeTypes = { card: CanvasCard }
  let nodes = $derived(toFlowNodes(view.nodes, onOpen, statuses))
  let edges = $derived(toFlowEdges(view.edges))
</script>

{#key fitVersion}
  <SvelteFlow
    {nodes}
    {edges}
    {nodeTypes}
    fitView
    fitViewOptions={{ padding: 0.2, maxZoom: 1 }}
    nodesDraggable={false}
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
