<script lang="ts">
  import { Background, BackgroundVariant, SvelteFlow } from '@xyflow/svelte'
  import '@xyflow/svelte/dist/style.css'
  import type { CanvasNode, FrameEntry, FramePlace, ProjectView } from '../lib/api'
  import { toFlowEdges, toFlowNodes, type CardNode } from '../lib/graph'
  import { toFrameNodes, type FrameBadge, type FrameNode as FrameFlowNode } from '../lib/frames'
  import CanvasCard from './CanvasCard.svelte'
  import FrameNode from './FrameNode.svelte'
  import type { CardStatus } from '../lib/verification'
  import type { ActivityBadge, RuntimeAttention } from '../lib/runtime'
  import type { RhythmMark } from '../lib/follow-ups'

  let { view, fitVersion = 0, statuses = {}, activity = {}, attention, backlog, budget, rhythm = {}, editable = false, onMove, onOpen,
    frames = [], frameBadges = {}, framesEditable = false, onPlaceFrame }: {
    view: ProjectView
    fitVersion?: number
    statuses?: Record<string, CardStatus>
    activity?: Record<string, ActivityBadge>
    attention?: RuntimeAttention[]
    backlog?: number
    /** First-screen attention items on the project card (FR-128). */
    budget?: number
    /** Quiet or resurfaced objects (FR-135, FR-136). */
    rhythm?: Record<string, RhythmMark>
    editable?: boolean
    onMove?: (id: string, x: number, y: number) => void
    onOpen: (node: CanvasNode) => void
    /** FR-141–146: frames a person placed; only the person moves or resizes them. */
    frames?: FrameEntry[]
    frameBadges?: Record<string, FrameBadge>
    framesEditable?: boolean
    onPlaceFrame?: (id: string, place: FramePlace) => void
  } = $props()

  const nodeTypes = { card: CanvasCard, frame: FrameNode }
  const nodeCache = { fitVersion: undefined as number | undefined, nodes: new Map<string, CardNode>() }
  const frameCache = new Map<string, FrameFlowNode>()
  let last: (FrameFlowNode | CardNode)[] = []
  let nodes = $derived.by(() => {
    if (nodeCache.fitVersion !== fitVersion) {
      nodeCache.nodes.clear()
      frameCache.clear()
      nodeCache.fitVersion = fitVersion
    }
    const placed = toFrameNodes(frames, frameBadges, framesEditable, onPlaceFrame, frameCache)
    const next = [...placed, ...toFlowNodes(view.nodes, onOpen, statuses, editable, activity, nodeCache.nodes, attention, backlog, budget, rhythm)]
    // The same nodes keep the same array, so an unrelated live update never resets a node being dragged.
    if (next.length === last.length && next.every((node, index) => node === last[index])) return last
    last = next
    return next
  })
  let viewport = $state({ x: 0, y: 0, zoom: 1 })
  export function canvasPoint(x: number, y: number) {
    return { x: (x - viewport.x) / viewport.zoom, y: (y - viewport.y) / viewport.zoom }
  }
  let edges = $derived(toFlowEdges(view.edges))

  function dragStopped(node: { id: string; type?: string; position: { x: number; y: number }; width?: number; height?: number; data: unknown } | null | undefined) {
    if (!node) return
    if (node.type === 'frame') {
      const frame = (node.data as FrameFlowNode['data']).frame
      if (!framesEditable) return
      onPlaceFrame?.(frame.id, { x: Math.round(node.position.x), y: Math.round(node.position.y),
        width: Math.round(node.width ?? frame.place.width), height: Math.round(node.height ?? frame.place.height) })
      return
    }
    if (editable) onMove?.(node.id, node.position.x, node.position.y)
  }
</script>

{#key fitVersion}
  <SvelteFlow
    {nodes}
    {edges}
    {nodeTypes}
    fitView
    fitViewOptions={{ padding: 0.2, maxZoom: 1 }}
    nodesDraggable={editable || framesEditable}
    nodeDragThreshold={0}
    bind:viewport
    onnodedragstop={({ targetNode }) => dragStopped(targetNode)}
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
