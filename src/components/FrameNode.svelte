<script lang="ts">
  import { NodeResizer, useSvelteFlow, type NodeProps } from '@xyflow/svelte'
  import type { CalloutKind, CardChange, FrameEntry, FrameHistory, FramePending, FrameSourceView, FrameStep, SourceChange, SourceHistory, FrameExport, FrameFormat } from '../lib/api'
  import { FRAME_MAX_SIZE, FRAME_MIN_SIZE, calloutLabels, calloutsFor, cardChangeLabels, changeRangeLabels, changesSince, frameLanes, frameVersions, pastFrame, pendingLabels, rangeStart, shownCards, sourceChangeText, sourceStatus, withUntil, flowChart, offersFlow, type ChangeRange, type FrameNode, type ShownCard, type FlowChart } from '../lib/frames'
  import { placeCards, type PlacedCard } from '../lib/placement'
  import { setByLabels } from '../lib/naming'
  import { GHOST_REASONS, ghostNodes, OVERLAY_LABELS } from '../lib/overlay'
  import { rhythmLabels } from '../lib/follow-ups'
  import SourceStateRule from './SourceStateRule.svelte'
  import FrameTimeView from './FrameTimeView.svelte'
  import { offeredViews } from '../lib/frames'

  let { data, draggable, width, height }: NodeProps<FrameNode> = $props()
  const flow = useSvelteFlow()
  // FR-157: a past step replaces what the frame shows, never where it is; replay is read-only.
  let past = $state<{ seq: number; step: FrameStep; frame: FrameEntry | null } | null>(null)
  // FR-169: a text-source frame's file at one past commit; read-only like a past step.
  let pastFile = $state<{ commit: string; source: FrameSourceView } | null>(null)
  let frame = $derived(past?.frame ?? (pastFile ? { ...data.frame, source: pastFile.source } : data.frame))
  let w = $derived(width ?? frame.place.width)
  let h = $derived(height ?? frame.place.height)
  let lanes = $derived(frameLanes(frame.layout, w, h))
  // FR-149: every card's place is derived from the data; pinned cards stay where the person put them.
  let placement = $derived(placeCards(frame, shownCards(frame), frame.pins ?? {}, { width: w, height: h }))
  let centers = $derived(new Map(placement.cards.map((placed) => [placed.card.id, { x: placed.x + placed.width / 2, y: placed.y + placed.height / 2 }])))
  let pending = $derived(frame.pending ?? [])
  let actions = $derived(frame.frozen || past || pastFile ? undefined : data.actions)
  // FR-163–176: a text-source frame's cards are its file's elements; edits become line patches on main.
  let source = $derived(frame.source ?? null)
  let sourceNote = $derived(source ? sourceStatus(source) : null)
  let locked = $derived(!!source && (!!source.locked || source.status !== 'ok'))
  let showUnknown = $state(false)
  let fixing = $state<string | null>(null)
  let renaming = $state('')
  function sourceEdit(change: SourceChange) {
    actions?.card?.(frame.id, frame.id, { op: 'source', change })
  }
  /** The App's card-ID check; elements named only by a title with spaces stay where the layout puts them. */
  function pinnable(card: ShownCard): boolean {
    return card.kind !== 'proposed' && (card.kind !== 'source' || /^[A-Za-z0-9_-]+(\.[A-Za-z0-9_-]+)*$/.test(card.id))
  }
  function editable(card: ShownCard): boolean {
    return card.kind === 'free' || (card.kind === 'source' && !locked && !card.readOnly)
  }
  let canRules = $derived(!!data.actions?.rules && !past)

  // A person's writes, in the App only (H2). Nothing here moves a card the daemon placed.
  let adding = $state(false)
  let title = $state('')
  let editing = $state<string | null>(null)
  let editingRules = $state(false)
  let rulesText = $state('')
  let laneMax = $state('')
  let until = $state('')
  let reviewEvery = $state('')
  let calloutsOff = $state(false)

  // FR-160/161: callouts follow their card and take no part in layout; the person hides them by kind.
  let hidden = $state<CalloutKind[]>([])
  let showCalloutKinds = $state(false)
  let notes = $derived(calloutsFor(frame, hidden))
  const calloutKinds = Object.keys(calloutLabels) as CalloutKind[]
  const NOTE_WIDTH = 180
  function toggleKind(kind: CalloutKind) {
    hidden = hidden.includes(kind) ? hidden.filter((other) => other !== kind) : [...hidden, kind]
  }
  /** Beside the card where there is room, else on its other side, else under it. Drawn only; nothing is laid out around it. */
  function notePlace(spot: { x: number; y: number }, width: number, height: number): string {
    if (spot.x + width + 10 + NOTE_WIDTH <= w) return `left: ${spot.x + width + 10}px; top: ${spot.y}px;`
    if (spot.x - 10 - NOTE_WIDTH >= 0) return `left: ${spot.x - 10 - NOTE_WIDTH}px; top: ${spot.y}px;`
    return `left: ${spot.x}px; top: ${spot.y + height + 6}px;`
  }

  // FR-157/158: history and change ranges, read on request only; highlighting adds badges and fades, it moves nothing.
  let history = $state<FrameHistory | null>(null)
  let historyError = $state<string | null>(null)
  let showingHistory = $state(false)
  let range = $state<ChangeRange | ''>('')
  let rangeDate = $state('')
  // FR-158: the milestone's time, from the project's versions or this frame's own.
  let rangeMilestone = $state('')
  let stepIndex = $state(0)
  let reading = 0
  let contentKey = $derived(JSON.stringify([data.frame.cards ?? [], data.frame.rules_changed ?? null, data.frame.pins ?? {}]))
  $effect(() => {
    void contentKey
    if ((range || showingHistory) && data.history) void loadHistory()
  })
  async function loadHistory() {
    const reader = data.history
    if (!reader) return
    const ticket = ++reading
    try {
      const read = await reader.steps(data.frame.id)
      if (ticket !== reading) return
      history = read
      historyError = null
      if (!past) stepIndex = read.steps.length
    } catch (cause) {
      if (ticket === reading) historyError = `读取历史失败：${String(cause)}`
    }
  }
  async function showStep(index: number) {
    stepIndex = index
    const steps = history?.steps ?? []
    if (!data.history || index >= steps.length) { past = null; return }
    const step = steps[index]
    const ticket = ++reading
    try {
      const at = await data.history.at(data.frame.id, step.seq)
      if (ticket !== reading) return
      past = { seq: step.seq, step: at.step, frame: pastFrame(data.frame, at) }
      version = null
      historyError = null
    } catch (cause) {
      if (ticket === reading) historyError = `读取那一步失败：${String(cause)}`
    }
  }
  // FR-169: the file's own commits, each summarized from the parses.
  let fileHistory = $state<SourceHistory | null>(null)
  async function loadFileHistory() {
    const read = data.history?.sourceSteps
    if (!read || !data.frame.source) return
    const ticket = ++reading
    try {
      const steps = await read(data.frame.id)
      if (ticket === reading) fileHistory = steps
    } catch (cause) {
      if (ticket === reading) historyError = `读取文件历史失败：${String(cause)}`
    }
  }
  $effect(() => {
    if (showingHistory && data.frame.source && !fileHistory) void loadFileHistory()
  })
  async function showCommit(commit: string) {
    const read = data.history?.sourceAt
    if (!read) return
    const ticket = ++reading
    try {
      const at = await read(data.frame.id, commit)
      if (ticket !== reading) return
      past = null
      pastFile = { commit, source: at.source }
      historyError = null
    } catch (cause) {
      if (ticket === reading) historyError = `读取那一版失败：${String(cause)}`
    }
  }
  // FR-155: the frame as text, read on request; it is derived and writes nothing.
  let exported = $state<FrameExport | null>(null)
  let exportError = $state<string | null>(null)
  let showingExport = $state(false)
  let exportTicket = 0
  async function readExport(format?: FrameFormat) {
    const reader = data.history?.text
    if (!reader) return
    showingExport = true
    const ticket = ++exportTicket
    try {
      const read = await reader(data.frame.id, format)
      if (ticket !== exportTicket) return
      exported = read
      exportError = null
    } catch (cause) {
      if (ticket === exportTicket) exportError = `导出失败：${String(cause)}`
    }
  }
  function closeExport() {
    showingExport = false
    exported = null
    exportError = null
    exportTicket++
  }
  // FR-159: the cumulative flow, opened on request in the frame's corner;
  // derived from the events on every read, it writes and moves nothing.
  let chart = $state<FlowChart | null>(null)
  let chartError = $state<string | null>(null)
  let showingFlow = $state(false)
  let pointed = $state<number | null>(null)
  let flowTicket = 0
  async function readFlow() {
    const reader = data.history?.flow
    if (!reader) return
    showingFlow = true
    const ticket = ++flowTicket
    try {
      const read = await reader(data.frame.id)
      if (ticket !== flowTicket) return
      chart = flowChart(read.flow, 280, 96)
      chartError = null
    } catch (cause) {
      if (ticket === flowTicket) chartError = `读取累积流失败：${String(cause)}`
    }
  }
  function closeFlow() {
    showingFlow = false
    chart = null
    chartError = null
    pointed = null
    flowTicket++
  }
  // FR-159: the other views a template may declare, one open at a time.
  let timeView = $state<'plan_deviation' | 'argument_evolution' | null>(null)
  const timeViewLabels = { plan_deviation: '计划偏差', argument_evolution: '演变' } as const
  function toggleTimeView(view: 'plan_deviation' | 'argument_evolution') {
    closeFlow()
    timeView = timeView === view ? null : view
  }
  $effect(() => {
    if (showingFlow) timeView = null
  })
  function pointAt(event: PointerEvent) {
    if (!chart || !chart.days.length) return
    const box = (event.currentTarget as SVGElement).getBoundingClientRect()
    const share = box.width ? Math.min(1, Math.max(0, (event.clientX - box.left) / box.width)) : 1
    pointed = Math.round(share * (chart.days.length - 1))
  }
  let shownDay = $derived(chart && chart.days.length ? chart.days[pointed ?? chart.days.length - 1] : null)
  function closeHistory() {
    showingHistory = false
    past = null
    pastFile = null
    version = null
    fileHistory = null
    reading++
  }
  let start = $derived(range ? rangeStart(range, Date.now(), data.since, range === 'milestone' ? rangeMilestone : rangeDate) : null)
  // FR-157: a person-set frame's versions; the chosen one shows as a ghost layer behind the live cards.
  let versions = $derived(history && frame.set_by === 'human' && !data.frame.source ? frameVersions(history.steps) : [])
  let milestones = $derived([...(data.milestones ?? []), ...frameVersions(history?.steps ?? [])])
  let version = $state<{ seq: number; frame: FrameEntry | null } | null>(null)
  const boxes = (cards: PlacedCard[]) => cards.map((placed) => ({ id: placed.card.id, title: placed.card.title, x: placed.x, y: placed.y, width: placed.width, height: placed.height }))
  let ghosts = $derived(version?.frame && !past && !pastFile
    ? ghostNodes(boxes(placeCards(version.frame, shownCards(version.frame), version.frame.pins ?? {}, { width: w, height: h }).cards), boxes(placement.cards)) : [])
  async function showVersion(value: string) {
    const seq = Number(value)
    if (!value || !data.history || !Number.isSafeInteger(seq)) { version = null; return }
    const ticket = ++reading
    try {
      const at = await data.history.at(data.frame.id, seq)
      if (ticket !== reading) return
      past = null
      stepIndex = history?.steps.length ?? 0
      version = { seq, frame: pastFrame(data.frame, at) }
      historyError = null
    } catch (cause) {
      if (ticket === reading) historyError = `读取那一版失败：${String(cause)}`
    }
  }
  let highlight = $derived<Record<string, CardChange[]> | null>(past ? past.step.cards
    : range && history && start !== null ? changesSince(history.steps, start) : null)
  function kindsOf(card: ShownCard): CardChange[] {
    return highlight?.[card.id] ?? []
  }
  /** Bound cards follow objects outside the frame's history, so they are never faded for it. */
  function faded(card: ShownCard): boolean {
    return !!highlight && card.kind !== 'bound' && kindsOf(card).length === 0
  }

  function add(event: SubmitEvent) {
    event.preventDefault()
    const text = title.trim()
    if (!text) return
    if (source) sourceEdit({ do: 'add', title: text })
    else data.actions?.card?.(frame.id, frame.id, { op: 'add', title: text })
    title = ''
    adding = false
  }
  function openRules() {
    rulesText = frame.rules.text
    laneMax = frame.rules.per_lane_max?.toString() ?? ''
    until = frame.rules.update_until && 'date' in frame.rules.update_until ? frame.rules.update_until.date : ''
    reviewEvery = frame.rules.review_every?.toString() ?? ''
    calloutsOff = frame.rules.callouts_off ?? false
    editingRules = !editingRules
  }
  function saveRules(event: SubmitEvent) {
    event.preventDefault()
    const max = laneMax.trim() ? Number(laneMax) : undefined
    const rules = withUntil({ ...frame.rules, text: rulesText.trim() }, until || null)
    if (max === undefined) delete rules.per_lane_max
    else rules.per_lane_max = max
    const every = String(reviewEvery).trim() ? Number(reviewEvery) : undefined
    if (every === undefined) delete rules.review_every
    else rules.review_every = every
    rules.callouts_off = calloutsOff
    // The frame's rules change by the person's hand alone, frozen or not.
    data.actions?.rules?.(frame.id, rules)
    editingRules = false
  }
  function summary(item: FramePending): string {
    const op = item.op
    const who = item.from ?? '受托者'
    switch (op.op) {
      case 'add': return `${who} 想加「${op.title}」`
      case 'update': return `${who} 建议改 ${item.target}${op.title ? `的标题为「${op.title}」` : ''}`
      case 'move': return `${who} 想把 ${item.target} 移到「${op.state}」`
      case 'link': return `${who} 想连 ${item.target} → ${op.to}`
      case 'archive': return `${who} 想归档 ${item.target}`
      case 'rules': return `${who} 提议改规则`
      case 'source': return `${item.from ?? '你'}：${sourceChangeText(op.change)}${item.note ? `（没写进文件：${item.note}）` : ''}`
    }
  }
  function marks(card: ShownCard): string {
    return [...new Set(card.waiting.map((item) => pendingLabels[item.why]))].join('、')
  }
  /** FR-151: changed since the person last looked. */
  function changed(card: ShownCard): boolean {
    return data.since !== undefined && !!card.changed && Date.parse(card.changed) > data.since
  }

  // FR-149: dragging a card pins it where the person lets go; a click is not a drag.
  let dragging = $state<{ id: string; start: { x: number; y: number }; from: { x: number; y: number }; to: { x: number; y: number } } | null>(null)
  let dragged = false
  function grab(event: PointerEvent, placed: PlacedCard) {
    if (!actions?.pin || !pinnable(placed.card) || event.button !== 0) return
    dragged = false
    dragging = { id: placed.card.id, start: flow.screenToFlowPosition({ x: event.clientX, y: event.clientY }), from: { x: placed.x, y: placed.y }, to: { x: placed.x, y: placed.y } }
    ;(event.currentTarget as HTMLElement).setPointerCapture?.(event.pointerId)
  }
  function drag(event: PointerEvent) {
    if (!dragging) return
    const at = flow.screenToFlowPosition({ x: event.clientX, y: event.clientY })
    const dx = at.x - dragging.start.x
    const dy = at.y - dragging.start.y
    if (Math.abs(dx) + Math.abs(dy) > 3) dragged = true
    dragging.to = { x: Math.round(Math.min(Math.max(dragging.from.x + dx, 0), w - 40)), y: Math.round(Math.min(Math.max(dragging.from.y + dy, 0), h - 20)) }
  }
  function drop() {
    const done = dragging
    dragging = null
    if (done && dragged) actions?.pin?.(frame.id, { [done.id]: done.to })
  }
  function at(placed: PlacedCard): { x: number; y: number } {
    return dragging?.id === placed.card.id ? dragging.to : placed
  }
</script>

<!-- FR-141–153: a frame sits behind every card; lanes are drawn, never laid out from the fast layer. -->
<NodeResizer isVisible={draggable} minWidth={FRAME_MIN_SIZE} minHeight={FRAME_MIN_SIZE} maxWidth={FRAME_MAX_SIZE} maxHeight={FRAME_MAX_SIZE}
  lineClass="frame-node-resize" handleClass="frame-node-handle"
  onResizeEnd={(_, place) => data.onPlace?.(frame.id, { x: Math.round(place.x), y: Math.round(place.y), width: Math.round(place.width), height: Math.round(place.height) })} />
<section class="frame-node" class:is-quiet={data.rhythm === 'quiet'} aria-label={`容器 ${frame.title}`}>
  {#each lanes.columns as lane, index (index)}
    <div class="frame-node-lane frame-node-column" style={`left: ${lane.start}px; width: ${lane.size}px;`}><span>{lane.label}</span></div>
  {/each}
  {#each lanes.rows as lane, index (index)}
    <div class="frame-node-lane frame-node-row" style={`top: ${lane.start}px; height: ${lane.size}px;`}><span>{lane.label}</span></div>
  {/each}
  <svg class="frame-node-lines" width={w} height={h} aria-hidden="true">
    {#each placement.lines as line (`${line.kind}:${line.from}>${line.to}`)}
      {@const from = centers.get(line.from)}
      {@const to = centers.get(line.to)}
      {#if from && to}<line class={`frame-line frame-line-${line.kind}`} x1={from.x} y1={from.y} x2={to.x} y2={to.y} />{/if}
    {/each}
  </svg>
  {#each placement.marks as mark (mark.label)}
    <span class="frame-node-mark" style={`left: ${mark.x}px; top: ${mark.y}px;`}>{mark.label}</span>
  {/each}
  {#if ghosts.length}
    <div class="canvas-ghosts" role="group" aria-label="旧版本幽灵层">
      {#each ghosts as ghost (ghost.node.id)}
        <div class="canvas-ghost" style={`left: ${ghost.node.x}px; top: ${ghost.node.y}px; width: ${ghost.node.width}px; height: ${ghost.node.height}px;`}>
          <span class="canvas-ghost-label"><span aria-hidden="true">{OVERLAY_LABELS.ghost.symbol}</span> {OVERLAY_LABELS.ghost.label} · {GHOST_REASONS[ghost.why]}</span>
          <strong>{ghost.node.title}</strong>
        </div>
      {/each}
    </div>
  {/if}
  {#each placement.cards as placed (placed.card.id)}
    {@const card = placed.card}
    {@const spot = at(placed)}
    <div class="frame-card nodrag nopan" class:is-proposed={card.kind === 'proposed'} class:is-bound={card.kind === 'bound'} class:is-done={card.done} class:is-read-only={!!card.readOnly}
      class:is-waiting={card.waiting.length > 0} class:is-changed={changed(card)} class:is-pinned={placed.pinned} class:is-dragging={dragging?.id === card.id} class:is-faded={faded(card)}
      style={`left: ${spot.x}px; top: ${spot.y}px; width: ${placed.width}px; min-height: ${placed.height}px;`}
      role="group" aria-label={`卡片 ${card.title}`}
      onpointerdown={(event) => grab(event, placed)} onpointermove={drag} onpointerup={drop} onpointercancel={() => (dragging = null)}
      onmouseenter={() => data.onFocus?.(card.id, false)} onmouseleave={() => data.onFocus?.(null, false)}>
      <button type="button" class="frame-card-title" title={card.body || card.title}
        onclick={() => { if (dragged) { dragged = false; return } data.onFocus?.(card.id, true); if (editable(card) && actions?.card) { renaming = ""; editing = editing === card.id ? null : card.id } }}>
        {#if card.kind === 'proposed'}<span class="frame-card-mark" aria-hidden="true">◇</span>{/if}
        {#if card.kind === 'bound'}<span class="frame-card-mark" aria-hidden="true">⛓</span>{/if}
        {card.title}
      </button>
      {#if card.kind === 'proposed' && card.waiting[0]?.why === 'write'}<span class="frame-card-note">{card.waiting[0].note ? '没写进文件 · 待你处理' : '待写入文件'}</span>
      {:else if card.kind === 'proposed'}<span class="frame-card-note">提案 · 待你处理</span>
      {:else if card.waiting.length}<span class="frame-card-note"><span aria-hidden="true">◇</span> {marks(card)}</span>{/if}
      {#if kindsOf(card).length}
        <span class="frame-card-changes" role="status">
          {#each kindsOf(card) as change (change)}<span class={`frame-card-change frame-card-change-${change}`}><span aria-hidden="true">{cardChangeLabels[change].icon}</span> {cardChangeLabels[change].label}</span>{/each}
        </span>
      {/if}
      {#if placed.pinned}
        <button type="button" class="frame-card-pin" disabled={!actions?.pin} title="你拖动过这张卡片，布局不再移动它；点此放回自动布局"
          onclick={() => actions?.pin?.(frame.id, { [card.id]: null })}><span aria-hidden="true">◆</span> 已固定</button>
      {/if}
      {#if card.readOnly}<span class="frame-card-note" title={card.readOnly}><span aria-hidden="true">⊘</span> 只读：{card.readOnly}</span>{/if}
      {#if editing === card.id && actions?.card && card.kind === 'source'}
        <form class="frame-card-edit" onsubmit={(event) => { event.preventDefault(); const next = renaming.trim(); if (next && next !== card.title) sourceEdit({ do: 'title', element: card.id, title: next }); editing = null }}>
          <select aria-label="状态" value={card.state ?? ''} onchange={(event) => { sourceEdit({ do: 'state', element: card.id, state: event.currentTarget.value }); editing = null }}>
            {#if !card.state}<option value="">（无状态）</option>{/if}
            {#each frame.states as state (state)}<option value={state}>{state}</option>{/each}
          </select>
          <input aria-label="新名称" maxlength={frame.limits.title_max} placeholder={card.title} bind:value={renaming} />
          <button type="submit" class="subtle-button" disabled={!renaming.trim()}>改名</button>
        </form>
      {:else if editing === card.id && actions?.card}
        <div class="frame-card-edit">
          <select aria-label="状态" value={card.state ?? ''} onchange={(event) => { actions?.card?.(frame.id, card.id, { op: 'move', state: event.currentTarget.value }); editing = null }}>
            {#each frame.states as state (state)}<option value={state}>{state}</option>{/each}
          </select>
          <button type="button" class="subtle-button" onclick={() => { actions?.card?.(frame.id, card.id, { op: 'archive' }); editing = null }}>归档</button>
        </div>
      {/if}
    </div>
    {#if notes.get(card.id)?.length}
      <div class="frame-callouts" style={notePlace(spot, placed.width, placed.height)} aria-label={`卡片 ${card.title} 的旁注`}>
        {#each notes.get(card.id) ?? [] as note (`${note.kind}:${note.source}`)}
          <span class={`frame-callout frame-callout-${note.kind}`} title={`${calloutLabels[note.kind].label}：${note.text}（来自 ${note.source}）`}>
            <span aria-hidden="true">{calloutLabels[note.kind].icon}</span> <b>{calloutLabels[note.kind].label}</b> {note.text}
          </span>
        {/each}
      </div>
    {/if}
  {/each}
  {#if notes.get(frame.id)?.length}
    <div class="frame-callouts frame-callouts-frame" aria-label={`容器 ${frame.title} 的旁注`}>
      {#each notes.get(frame.id) ?? [] as note (`${note.kind}:${note.source}`)}
        <span class={`frame-callout frame-callout-${note.kind}`} title={`${calloutLabels[note.kind].label}：${note.text}（来自 ${note.source}）`}>
          <span aria-hidden="true">{calloutLabels[note.kind].icon}</span> <b>{calloutLabels[note.kind].label}</b> {note.text}
        </span>
      {/each}
    </div>
  {/if}
  {#each placement.folds as fold, index (index)}
    <span class="frame-card-more" style={`left: ${fold.x}px; top: ${fold.y}px; min-width: ${Math.min(fold.width, 64)}px;`}
      title="放不下或超过每栏上限的卡片已折叠；请合并或归档">+{fold.count}</span>
  {/each}
  <header class="frame-node-head" class:is-draggable={draggable} title={draggable ? '拖动容器位置' : undefined}>
    {#if draggable}<span class="frame-node-grip" aria-hidden="true">⠿</span>{/if}
    <strong>{frame.title}</strong>
    <span class="frame-node-meta">容器 · {frame.template.id}@{frame.template.version} · {setByLabels[frame.set_by]}</span>
    {#if frame.frozen}<span class="frame-node-frozen" role="status"><span aria-hidden="true">■</span> 已停止更新</span>{/if}
    {#if data.rhythm}<span class={`rhythm-mark frame-node-rhythm rhythm-${data.rhythm}`} role="status"><span aria-hidden="true">{rhythmLabels[data.rhythm].symbol}</span> {rhythmLabels[data.rhythm].text}</span>{/if}
    {#if version}<span class="frame-node-past" role="status"><span aria-hidden="true">{OVERLAY_LABELS.ghost.symbol}</span> 对照旧版本{ghosts.length ? ` · ${ghosts.length} 张` : ' · 与现在相同'}</span>{/if}
    {#if data.badge}
      <span class={`frame-node-badge frame-node-badge-${data.badge.tone}`} role="status">
        <span aria-hidden="true">{data.badge.tone === 'warn' ? '▲' : '●'}</span> {data.badge.label}
      </span>
    {/if}
    {#if past}<span class="frame-node-past" role="status"><span aria-hidden="true">◷</span> 历史 · 只读</span>{/if}
    {#if source}
      <span class="frame-node-source" title={source.revision ? `${source.path} @ ${source.revision.slice(0, 7)}` : source.path}>
        文件 {source.path}{typeof source.understood === 'number' ? ` · 理解 ${source.understood}%` : ''}
      </span>
      {#if sourceNote}<span class="frame-node-frozen" role="status"><span aria-hidden="true">▲</span> {sourceNote}</span>{/if}
      {#if locked}<span class="frame-node-frozen" role="status"><span aria-hidden="true">■</span> 编辑已锁定</span>{/if}
      {#if pastFile}<span class="frame-node-past" role="status"><span aria-hidden="true">◷</span> 文件 {pastFile.commit.slice(0, 7)} · 只读</span>{/if}
    {/if}
    {#if actions?.card && frame.mode !== 'bound' && !locked}<button type="button" class="frame-node-action nodrag" onclick={() => (adding = !adding)}>+ 卡片</button>{/if}
    {#if canRules}<button type="button" class="frame-node-action nodrag" onclick={openRules}>规则</button>{/if}
    {#if (data.frame.callouts ?? []).length}<button type="button" class="frame-node-action nodrag" aria-expanded={showCalloutKinds} onclick={() => (showCalloutKinds = !showCalloutKinds)}>旁注</button>{/if}
    {#if data.history}
      <button type="button" class="frame-node-action nodrag" aria-pressed={showingHistory} onclick={() => (showingHistory ? closeHistory() : (showingHistory = true))}>历史</button>
      <select class="frame-node-range nodrag" aria-label="高亮变化的范围" bind:value={range}>
        <option value="">不分类高亮</option>
        {#each Object.entries(changeRangeLabels) as [value, label] (value)}<option {value}>{label}</option>{/each}
      </select>
      {#if range === 'date'}<input class="frame-node-range nodrag" aria-label="从这天起" type="date" bind:value={rangeDate} />{/if}
      {#if range === 'milestone'}
        <select class="frame-node-range nodrag" aria-label="从这个里程碑起" bind:value={rangeMilestone}>
          <option value="">选择里程碑</option>
          {#each milestones as milestone (milestone.id)}<option value={milestone.at}>{milestone.label}</option>{/each}
        </select>
      {/if}
    {/if}
    {#if data.history?.flow && offersFlow(data.frame) && !data.frame.source && !past && !pastFile}
      <button type="button" class="frame-node-action nodrag" aria-pressed={showingFlow} onclick={() => (showingFlow ? closeFlow() : void readFlow())}>累积流</button>
    {/if}
    {#if data.history && !past && !pastFile}
      {#each offeredViews(data.frame).filter((view) => (view === 'plan_deviation' ? data.history?.deviation : data.history?.evolution)) as view (view)}
        <button type="button" class="frame-node-action nodrag" aria-pressed={timeView === view} onclick={() => toggleTimeView(view)}>{timeViewLabels[view]}</button>
      {/each}
    {/if}
    {#if data.history?.text && !data.frame.source && !past && !pastFile}
      <button type="button" class="frame-node-action nodrag" aria-pressed={showingExport} onclick={() => (showingExport ? closeExport() : void readExport())}>导出</button>
    {/if}
    {#if data.actions?.ask && !past}<button type="button" class="frame-node-action nodrag" onclick={() => data.actions?.ask?.(frame.id)}>提问</button>{/if}
    {#if data.actions?.materialize && !past}
      <button type="button" class="frame-node-action nodrag" title={frame.materialized ? `上次写入：${frame.materialized.commit.slice(0, 7)}` : '写入 tower.canvas，批准前先看要写的内容'}
        onclick={() => data.actions?.materialize?.(frame.id)}>写入画布{frame.materialized ? ' ✓' : ''}</button>
    {/if}
  </header>
  {#if showCalloutKinds}
    <div class="frame-node-form nodrag" role="group" aria-label="按种类显示旁注">
      {#each calloutKinds as kind (kind)}
        <label class={`frame-callout frame-callout-${kind}`}><input type="checkbox" checked={!hidden.includes(kind)} onchange={() => toggleKind(kind)} />
          <span aria-hidden="true">{calloutLabels[kind].icon}</span> {calloutLabels[kind].label}</label>
      {/each}
    </div>
  {/if}
  {#if source && (source.signal || source.renamed_to || source.problems?.length || source.fixes?.length || source.unknown_lines)}
    <div class="frame-node-source-panel nodrag nopan" role="group" aria-label="文本源文件">
      {#if source.signal}<p class={`frame-node-source-signal is-${source.signal.level}`} role="status"><span aria-hidden="true">{source.signal.level === 'active' ? '▲' : '●'}</span> {source.signal.text}</p>{/if}
      {#if source.renamed_to}
        <p>文件看起来已改名为 {source.renamed_to}。
          {#if data.actions?.relink && !past && !pastFile}<button type="button" class="subtle-button" onclick={() => data.actions?.relink?.(frame.id, source.renamed_to ?? '')}>改为跟随它</button>{/if}
        </p>
      {/if}
      {#each source.problems ?? [] as problem (problem.line)}<p class="frame-node-history-error">第 {problem.line} 行：{problem.reason}</p>{/each}
      {#each source.fixes ?? [] as fix (fix.id)}
        <div class="frame-node-source-fix">
          <button type="button" class="subtle-button" aria-expanded={fixing === fix.id} onclick={() => (fixing = fixing === fix.id ? null : fix.id)}>修复建议：{fix.title}</button>
          {#if fixing === fix.id}
            <pre class="frame-node-source-diff">{fix.diff.join('\n')}</pre>
            {#if actions?.card && source.sha256}
              <button type="button" class="primary-button" onclick={() => { sourceEdit({ do: 'fix', fix: fix.id, sha256: source.sha256 ?? '' }); fixing = null }}>Fix</button>
            {/if}
          {/if}
        </div>
      {/each}
      {#if source.unknown_lines}
        <button type="button" class="subtle-button" aria-expanded={showUnknown} onclick={() => (showUnknown = !showUnknown)}>看不懂的 {source.unknown_lines} 行（原样保留）</button>
        {#if showUnknown}
          <ul class="frame-node-source-unknown">
            {#each source.unknown ?? [] as line (line.line)}<li>第 {line.line} 行 <code>{line.text}</code> · {line.reason}</li>{/each}
          </ul>
        {/if}
      {/if}
    </div>
  {/if}
  {#if source && canRules && !pastFile}
    <SourceStateRule tokens={source.lint?.find((item) => item.code === 'unmapped_status')?.tokens ?? []} states={frame.states}
      rules={frame.rules} onSave={(rules) => data.actions?.rules?.(frame.id, rules)} />
  {/if}
  {#if range === 'visit' && data.since === undefined}<p class="frame-node-history-note" role="status">还没有上次查看的记录。</p>{/if}
  {#if range === 'milestone' && history && !milestones.length}<p class="frame-node-history-note" role="status">还没有可选的里程碑：项目没有已出版版本，容器也没有写入过画布。</p>{/if}
  {#if adding}
    <form class="frame-node-form nodrag" onsubmit={add}>
      <input aria-label="卡片标题" maxlength={frame.limits.title_max} bind:value={title} placeholder="卡片标题" />
      <button type="submit" class="primary-button" disabled={!title.trim()}>加入</button>
    </form>
  {/if}
  {#if editingRules}
    <form class="frame-node-form nodrag" onsubmit={saveRules}>
      <input aria-label="规则说明" maxlength={300} bind:value={rulesText} placeholder="规则说明" />
      <input aria-label="每栏上限" type="number" min="1" bind:value={laneMax} placeholder="每栏上限" />
      <input aria-label="更新截止日期" type="date" bind:value={until} />
      <input aria-label="每隔几天回看" type="number" min="1" max="365" bind:value={reviewEvery} placeholder="每隔几天回看" />
      <label><input type="checkbox" bind:checked={calloutsOff} /> 关闭旁注</label>
      <button type="submit" class="primary-button">保存规则</button>
    </form>
  {/if}
  {#if showingFlow}
    <div class="frame-node-flow nodrag nopan" role="group" aria-label="累积流图">
      {#if chart && shownDay}
        <svg viewBox={`0 0 ${chart.width} ${chart.height}`} width={chart.width} height={chart.height} role="img"
          aria-label={`累积流图：${chart.days[0].date} 至 ${chart.days.at(-1)?.date}，共 ${chart.days.length} 天，最多 ${chart.max} 张卡片`}
          onpointermove={pointAt} onpointerleave={() => (pointed = null)}>
          {#each chart.bands as band (band.name)}<path class={`frame-flow-band frame-flow-slot-${band.slot}`} d={band.path}><title>{band.name}</title></path>{/each}
          {#if pointed !== null}<line class="frame-flow-cursor" x1={chart.days.length > 1 ? (pointed / (chart.days.length - 1)) * chart.width : chart.width / 2} x2={chart.days.length > 1 ? (pointed / (chart.days.length - 1)) * chart.width : chart.width / 2} y1="0" y2={chart.height} />{/if}
        </svg>
        <p class="frame-flow-day" role="status">{shownDay.date}</p>
        <ul class="frame-flow-legend">
          {#each chart.bands as band, index (band.name)}<li><span class={`frame-flow-swatch frame-flow-slot-${band.slot}`} aria-hidden="true"></span>{band.name} {shownDay.counts[index]}</li>{/each}
        </ul>
        <details class="frame-flow-table">
          <summary>按天查看</summary>
          <table>
            <thead><tr><th>日期</th>{#each chart.bands as band (band.name)}<th>{band.name}</th>{/each}</tr></thead>
            <tbody>{#each [...chart.days].reverse() as day (day.date)}<tr><td>{day.date}</td>{#each day.counts as count, index (index)}<td>{count}</td>{/each}</tr>{/each}</tbody>
          </table>
        </details>
      {:else if !chartError}<span>正在计算…</span>{/if}
      {#if chartError}<span class="frame-node-history-error" role="alert">{chartError}</span>{/if}
      <button type="button" class="subtle-button" onclick={closeFlow}>关闭</button>
    </div>
  {/if}
  {#if timeView && data.history && !past && !pastFile}
    <FrameTimeView frame={frame.id} view={timeView} reader={data.history} onClose={() => (timeView = null)} />
  {/if}
  {#if showingExport}
    <div class="frame-node-export nodrag nopan" role="group" aria-label="导出为文本">
      {#if exported}
        <select aria-label="导出格式" value={exported.format} onchange={(event) => { void readExport(event.currentTarget.value as FrameFormat) }}>
          {#each exported.formats as format (format)}<option value={format}>{format === 'mermaid' ? 'Mermaid' : 'Markdown'}</option>{/each}
        </select>
        <textarea aria-label="导出的文本" readonly rows="8" value={exported.text}></textarea>
      {:else if !exportError}<span>正在导出…</span>{/if}
      {#if exportError}<span class="frame-node-history-error" role="alert">{exportError}</span>{/if}
      <button type="button" class="subtle-button" onclick={closeExport}>关闭</button>
    </div>
  {/if}
  {#if showingHistory}
    <div class="frame-node-history nodrag nopan" role="group" aria-label="容器历史">
      {#if history}
        <input type="range" min="0" max={history.steps.length} value={stepIndex} aria-label="历史上的一步"
          onchange={(event) => { void showStep(Number(event.currentTarget.value)) }} />
        <span class="frame-node-history-at">
          {#if past}第 {stepIndex + 1}/{history.steps.length} 步 · {past.step.at.slice(0, 16).replace('T', ' ')} · {past.step.actor}{:else}现在{/if}
        </span>
        {#if past}<button type="button" class="subtle-button" onclick={() => { past = null; stepIndex = history?.steps.length ?? 0 }}>回到现在</button>{/if}
      {:else if !historyError}<span>正在读取历史…</span>{/if}
      {#if versions.length}
        <select aria-label="按版本对照" value={version ? String(version.seq) : ''} onchange={(event) => { void showVersion(event.currentTarget.value) }}>
          <option value="">不对照旧版本</option>
          {#each versions as entry (entry.id)}<option value={String(entry.seq)}>{entry.label}</option>{/each}
        </select>
      {/if}
      {#if historyError}<span class="frame-node-history-error" role="alert">{historyError}</span>{/if}
      <button type="button" class="subtle-button" onclick={closeHistory}>关闭</button>
      {#if fileHistory}
        <ol class="frame-node-source-steps" aria-label="文件历史">
          {#each fileHistory.steps as step (step.commit)}
            <li><button type="button" class="subtle-button" aria-pressed={pastFile?.commit === step.commit} onclick={() => { void showCommit(step.commit) }}>
              {step.commit.slice(0, 7)} · {step.at.slice(0, 16).replace('T', ' ')} · {step.summary.text}</button></li>
          {/each}
        </ol>
        {#if pastFile}<button type="button" class="subtle-button" onclick={() => (pastFile = null)}>回到 main</button>{/if}
      {/if}
    </div>
  {:else if historyError}<p class="frame-node-history-note" role="alert">{historyError}</p>
  {:else if pending.length && data.actions?.resolve && !past}
    <ul class="frame-node-pending nodrag" aria-label="待你处理">
      {#each pending as item (item.id)}
        <li>
          <span class="frame-node-pending-why">{pendingLabels[item.why]}</span>
          <span class="frame-node-pending-text">{summary(item)}</span>
          {#if item.why === 'write'}
            <!-- FR-167: a write the file no longer takes is redone or dropped by the person; one still queued can only be dropped. -->
            {#if item.note}<button type="button" class="subtle-button" disabled={frame.frozen || locked} onclick={() => data.actions?.resolve?.(frame.id, item.id, true)}>重做</button>{/if}
            <button type="button" class="subtle-button" onclick={() => data.actions?.resolve?.(frame.id, item.id, false)}>放弃</button>
          {:else}
            <button type="button" class="subtle-button" disabled={frame.frozen} onclick={() => data.actions?.resolve?.(frame.id, item.id, true)}>接受</button>
            <button type="button" class="subtle-button" onclick={() => data.actions?.resolve?.(frame.id, item.id, false)}>忽略</button>
          {/if}
        </li>
      {/each}
    </ul>
  {:else if frame.rules.text}<p class="frame-node-rules">规则：{frame.rules.text}</p>{/if}
</section>
