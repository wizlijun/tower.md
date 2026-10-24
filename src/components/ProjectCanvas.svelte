<script lang="ts">
  import { onDestroy, untrack } from 'svelte'
  import { Background, BackgroundVariant, Panel, SvelteFlow, ViewportPortal } from '@xyflow/svelte'
  import '@xyflow/svelte/dist/style.css'
  import type { CanvasEdge, CanvasNode, FrameEntry, FramePlace, ProjectView } from '../lib/api'
  import { sameProgress, toFlowEdges, toFlowNodes, type CardNode, type Progress } from '../lib/graph'
  import { coalescer } from '../lib/coalesce'
  import { GHOST_REASONS, ghostNodes, layoutMoves, OVERLAY_LABELS, TRANSITION_MS, tweenAt, type CanvasMark, type Move } from '../lib/overlay'
  import { shownCards, toFrameNodes, type FrameActions, type FrameBadge, type FrameReader, type FrameNode as FrameFlowNode } from '../lib/frames'
  import { crossLines, placeCards } from '../lib/placement'
  import CanvasCard from './CanvasCard.svelte'
  import FrameNode from './FrameNode.svelte'
  import type { CardStatus } from '../lib/verification'
  import type { ActivityBadge, RuntimeAttention } from '../lib/runtime'
  import type { RhythmMark } from '../lib/follow-ups'

  let { view, fitVersion = 0, statuses = {}, activity = {}, attention, backlog, budget, rhythm = {}, editable = false, onMove, onOpen,
    frames = [], frameBadges = {}, framesEditable = false, onPlaceFrame, frameActions, frameHistory, since, onConvert, marks = {}, before }: {
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
    /** FR-157/158: read-only history for each frame's slider and change ranges. */
    frameHistory?: FrameReader
    /** FR-151: when the person last looked (ms); cards changed since get a thin border. */
    since?: number
    /** FR-144 方式 2: the person picked nodes to turn into a frame. */
    onConvert?: (nodes: CanvasNode[], edges: CanvasEdge[]) => void
    /** FR-45, FR-36: proposed changes (dashed) and relation ends drawn over the version shown. */
    marks?: Record<string, CanvasMark>
    /** FR-35, FR-46: the approved version a draft is compared with; its moved and removed cards show as ghosts. */
    before?: CanvasNode[]
  } = $props()

  // FR-50: progress on cards shows at most once a second, the latest state
  // winning. A different view (another version, a draft, a sample) shows at once.
  let live = $state.raw<Progress | null>(null)
  let liveView: ProjectView | null = null
  const liveMerge = coalescer<Progress>((value) => { live = value })
  $effect(() => {
    const next: Progress = { activity, attention, backlog, rhythm }
    const current = view
    untrack(() => {
      const now = live === null || current !== liveView
      liveView = current
      if (!now && live && sameProgress(live, next)) { liveMerge.stop(); return }
      liveMerge.push(next, now)
    })
  })
  let progress = $derived(live ?? { activity, attention, backlog, rhythm })

  // FR-46: when a new version changes the layout, cards glide to their new
  // places and the old version can show as a ghost layer. Only a new revision
  // does this; dragging, fast-layer updates and refits never do.
  let former = $state.raw<CanvasNode[] | null>(null)
  let tween = $state.raw<{ moves: Map<string, Move>; t: number } | null>(null)
  let ghostsShown = $state(false)
  let seen: { fit: number; view: ProjectView } | null = null
  let frame: number | undefined
  function stopTween() {
    if (frame !== undefined) cancelAnimationFrame(frame)
    frame = undefined
    tween = null
  }
  function startTween(moves: Map<string, Move>) {
    stopTween()
    if (typeof requestAnimationFrame !== 'function' || globalThis.matchMedia?.('(prefers-reduced-motion: reduce)').matches) return
    let start: number | undefined
    const step = (time: number) => {
      start ??= time
      const t = (time - start) / TRANSITION_MS
      tween = t < 1 ? { moves, t } : null
      frame = t < 1 ? requestAnimationFrame(step) : undefined
    }
    tween = { moves, t: 0 }
    frame = requestAnimationFrame(step)
  }
  $effect(() => {
    const current = view
    const fit = fitVersion
    untrack(() => {
      const last = seen
      seen = { fit, view: current }
      if (!last || last.fit !== fit) { former = null; stopTween(); return }
      if (last.view.revision === current.revision || !ghostNodes(last.view.nodes, current.nodes).length) return
      former = last.view.nodes
      ghostsShown = false
      startTween(layoutMoves(last.view.nodes, current.nodes))
    })
  })
  onDestroy(() => { liveMerge.stop(); stopTween() })
  // FR-35: comparing a draft shows the approved version by default; after a new version it waits to be asked for.
  let comparing = $derived(!!before)
  $effect(() => {
    const value = comparing
    untrack(() => { ghostsShown = value })
  })
  let ghosts = $derived(ghostNodes(before ?? former ?? [], view.nodes))

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
    const placed = toFrameNodes(frames, frameBadges, framesEditable, onPlaceFrame, frameCache, framesEditable ? frameActions : undefined, { since, onFocus, history: frameHistory })
    const flow = toFlowNodes(view.nodes, choosing ? toggle : onOpen, statuses, editable, progress.activity, nodeCache.nodes, progress.attention, progress.backlog, budget, progress.rhythm, marks)
    const moving = tween
    const cards = moving ? flow.map((node) => {
      const move = moving.moves.get(node.id)
      return move ? { ...node, position: tweenAt(move, moving.t) } : node
    }) : flow
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
    {#if ghostsShown && ghosts.length}
      <ViewportPortal target="back">
        <div class="canvas-ghosts" role="group" aria-label="旧版本幽灵层">
          {#each ghosts as ghost (ghost.node.id)}
            <div class="canvas-ghost" style:left={`${ghost.node.x}px`} style:top={`${ghost.node.y}px`} style:width={`${ghost.node.width}px`} style:height={`${ghost.node.height}px`}>
              <span class="canvas-ghost-label"><span aria-hidden="true">{OVERLAY_LABELS.ghost.symbol}</span> {OVERLAY_LABELS.ghost.label} · {GHOST_REASONS[ghost.why]}</span>
              <strong>{ghost.node.title}</strong>
            </div>
          {/each}
        </div>
      </ViewportPortal>
    {/if}
    {#if ghosts.length}
      <Panel position="top-center">
        <button class="subtle-button canvas-ghost-toggle" aria-pressed={ghostsShown} onclick={() => { ghostsShown = !ghostsShown }}>
          <span aria-hidden="true">{OVERLAY_LABELS.ghost.symbol}</span> 旧版本幽灵层（{ghosts.length}）
        </button>
      </Panel>
    {/if}
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
