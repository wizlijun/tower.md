<script lang="ts">
  import { Background, BackgroundVariant, Panel, SvelteFlow, ViewportPortal } from '@xyflow/svelte'
  import '@xyflow/svelte/dist/style.css'
  import type { CanvasEdge, CanvasNode, FrameEntry, FramePlace, ProjectView } from '../lib/api'
  import { toFlowEdges, toFlowNodes, type CardNode } from '../lib/graph'
  import { shownCards, toFrameNodes, type FrameActions, type FrameBadge, type FrameNode as FrameFlowNode } from '../lib/frames'
  import { crossLines, placeCards } from '../lib/placement'
  import CanvasCard from './CanvasCard.svelte'
  import FrameNode from './FrameNode.svelte'
  import type { CardStatus } from '../lib/verification'
  import type { ActivityBadge, RuntimeAttention } from '../lib/runtime'
  import type { RhythmMark } from '../lib/follow-ups'

  let { view, fitVersion = 0, statuses = {}, activity = {}, attention, backlog, budget, rhythm = {}, editable = false, onMove, onOpen,
    frames = [], frameBadges = {}, framesEditable = false, onPlaceFrame, frameActions, since, onConvert }: {
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
    /** H2: a person's card writes, rule changes and decisions; absent while writes are blocked. */
    frameActions?: FrameActions
    /** FR-151: when the person last looked (ms); cards changed since get a thin border. */
    since?: number
    /** FR-144 方式 2: the person picked nodes to turn into a frame. */
    onConvert?: (nodes: CanvasNode[], edges: CanvasEdge[]) => void
  } = $props()

  // FR-141: lines to other frames show only while one end is hovered or clicked.
  let hovered = $state<string | null>(null)
  let selected = $state<string | null>(null)
  const onFocus = (card: string | null, sticky: boolean) => {
    if (sticky) selected = selected === card ? null : card
    else hovered = card
  }
  let placements = $derived(new Map(frames.map((frame) => [frame.id, placeCards(frame, shownCards(frame), frame.pins ?? {})])))
  let outside = $derived(crossLines(frames, placements, [hovered, selected].filter((card): card is string => !!card)))

  // FR-144 方式 2: pick nodes, then choose a template beside the canvas. Picking moves nothing.
  let choosing = $state(false)
  let chosen = $state<string[]>([])
  const toggle = (node: CanvasNode) => {
    if (node.kind === 'group') return
    chosen = chosen.includes(node.id) ? chosen.filter((id) => id !== node.id) : [...chosen, node.id]
  }
  function convert() {
    const picked = view.nodes.filter((node) => chosen.includes(node.id))
    if (!picked.length) return
    onConvert?.(picked, view.edges)
    choosing = false
    chosen = []
  }

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
    const placed = toFrameNodes(frames, frameBadges, framesEditable, onPlaceFrame, frameCache, framesEditable ? frameActions : undefined, { since, onFocus })
    const cards = toFlowNodes(view.nodes, choosing ? toggle : onOpen, statuses, editable, activity, nodeCache.nodes, attention, backlog, budget, rhythm)
    const next = [...placed, ...(choosing ? cards.map((node) => ({ ...node, class: chosen.includes(node.id) ? 'is-chosen' : undefined })) : cards)]
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
    {#if outside.length}
      <ViewportPortal target="front">
        <svg class="canvas-cross-lines" width="1" height="1" aria-hidden="true">
          {#each outside as line (line.id)}<line class="canvas-cross-line" x1={line.from.x} y1={line.from.y} x2={line.to.x} y2={line.to.y} />{/each}
        </svg>
      </ViewportPortal>
    {/if}
    {#if framesEditable && onConvert}
      <Panel position="top-left">
        {#if choosing}
          <div class="canvas-choose" role="status">
            <span>点选要转为容器的节点 · 已选 {chosen.length} 个</span>
            <button class="primary-button" disabled={!chosen.length} onclick={convert}>转为容器</button>
            <button class="subtle-button" onclick={() => { choosing = false; chosen = [] }}>取消</button>
          </div>
        {:else}
          <button class="subtle-button canvas-choose-toggle" onclick={() => { choosing = true; selected = null }}>选中节点转为容器</button>
        {/if}
      </Panel>
    {/if}
  </SvelteFlow>
{/key}
