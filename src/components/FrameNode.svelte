<script lang="ts">
  import { NodeResizer, useSvelteFlow, type NodeProps } from '@xyflow/svelte'
  import type { CalloutKind, CardChange, FrameEntry, FrameExport, FrameFormat, FrameHistory, FramePending, FrameStep } from '../lib/api'
  import { FRAME_MAX_SIZE, FRAME_MIN_SIZE, calloutLabels, calloutsFor, cardChangeLabels, changeRangeLabels, changesSince, frameLanes, pastFrame, pendingLabels, rangeStart,
    shownCards, withUntil, type ChangeRange, type FrameNode, type ShownCard } from '../lib/frames'
  import { placeCards, type PlacedCard } from '../lib/placement'
  import { setByLabels } from '../lib/naming'

  let { data, draggable, width, height }: NodeProps<FrameNode> = $props()
  const flow = useSvelteFlow()
  // FR-157: a past step replaces what the frame shows, never where it is; replay is read-only.
  let past = $state<{ seq: number; step: FrameStep; frame: FrameEntry | null } | null>(null)
  let frame = $derived(past?.frame ?? data.frame)
  let w = $derived(width ?? frame.place.width)
  let h = $derived(height ?? frame.place.height)
  let lanes = $derived(frameLanes(frame.layout, w, h))
  // FR-149: every card's place is derived from the data; pinned cards stay where the person put them.
  let placement = $derived(placeCards(frame, shownCards(frame), frame.pins ?? {}, { width: w, height: h }))
  let centers = $derived(new Map(placement.cards.map((placed) => [placed.card.id, { x: placed.x + placed.width / 2, y: placed.y + placed.height / 2 }])))
  let pending = $derived(frame.pending ?? [])
  let actions = $derived(frame.frozen || past ? undefined : data.actions)
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
      historyError = null
    } catch (cause) {
      if (ticket === reading) historyError = `读取那一步失败：${String(cause)}`
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
  function closeHistory() {
    showingHistory = false
    past = null
    reading++
  }
  let start = $derived(range ? rangeStart(range, Date.now(), data.since, rangeDate) : null)
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
    data.actions?.card?.(frame.id, frame.id, { op: 'add', title: text })
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
    if (!actions?.pin || placed.card.kind === 'proposed' || event.button !== 0) return
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
<section class="frame-node" aria-label={`容器 ${frame.title}`}>
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
  {#each placement.cards as placed (placed.card.id)}
    {@const card = placed.card}
    {@const spot = at(placed)}
    <div class="frame-card nodrag nopan" class:is-proposed={card.kind === 'proposed'} class:is-bound={card.kind === 'bound'} class:is-done={card.done}
      class:is-waiting={card.waiting.length > 0} class:is-changed={changed(card)} class:is-pinned={placed.pinned} class:is-dragging={dragging?.id === card.id} class:is-faded={faded(card)}
      style={`left: ${spot.x}px; top: ${spot.y}px; width: ${placed.width}px; min-height: ${placed.height}px;`}
      role="group" aria-label={`卡片 ${card.title}`}
      onpointerdown={(event) => grab(event, placed)} onpointermove={drag} onpointerup={drop} onpointercancel={() => (dragging = null)}
      onmouseenter={() => data.onFocus?.(card.id, false)} onmouseleave={() => data.onFocus?.(null, false)}>
      <button type="button" class="frame-card-title" title={card.body || card.title}
        onclick={() => { if (dragged) { dragged = false; return } data.onFocus?.(card.id, true); if (card.kind === 'free' && actions?.card) editing = editing === card.id ? null : card.id }}>
        {#if card.kind === 'proposed'}<span class="frame-card-mark" aria-hidden="true">◇</span>{/if}
        {#if card.kind === 'bound'}<span class="frame-card-mark" aria-hidden="true">⛓</span>{/if}
        {card.title}
      </button>
      {#if card.kind === 'proposed'}<span class="frame-card-note">提案 · 待你处理</span>
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
      {#if editing === card.id && actions?.card}
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
    {#if data.badge}
      <span class={`frame-node-badge frame-node-badge-${data.badge.tone}`} role="status">
        <span aria-hidden="true">{data.badge.tone === 'warn' ? '▲' : '●'}</span> {data.badge.label}
      </span>
    {/if}
    {#if past}<span class="frame-node-past" role="status"><span aria-hidden="true">◷</span> 历史 · 只读</span>{/if}
    {#if actions?.card && frame.mode !== 'bound'}<button type="button" class="frame-node-action nodrag" onclick={() => (adding = !adding)}>+ 卡片</button>{/if}
    {#if canRules}<button type="button" class="frame-node-action nodrag" onclick={openRules}>规则</button>{/if}
    {#if (data.frame.callouts ?? []).length}<button type="button" class="frame-node-action nodrag" aria-expanded={showCalloutKinds} onclick={() => (showCalloutKinds = !showCalloutKinds)}>旁注</button>{/if}
    {#if data.history}
      <button type="button" class="frame-node-action nodrag" aria-pressed={showingHistory} onclick={() => (showingHistory ? closeHistory() : (showingHistory = true))}>历史</button>
      <select class="frame-node-range nodrag" aria-label="高亮变化的范围" bind:value={range}>
        <option value="">不分类高亮</option>
        {#each Object.entries(changeRangeLabels) as [value, label] (value)}<option {value}>{label}</option>{/each}
      </select>
      {#if range === 'date'}<input class="frame-node-range nodrag" aria-label="从这天起" type="date" bind:value={rangeDate} />{/if}
    {/if}
    {#if data.history?.text && !past}
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
  {#if range === 'visit' && data.since === undefined}<p class="frame-node-history-note" role="status">还没有上次查看的记录。</p>{/if}
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
      {#if historyError}<span class="frame-node-history-error" role="alert">{historyError}</span>{/if}
      <button type="button" class="subtle-button" onclick={closeHistory}>关闭</button>
    </div>
  {:else if historyError}<p class="frame-node-history-note" role="alert">{historyError}</p>
  {:else if pending.length && data.actions?.resolve && !past}
    <ul class="frame-node-pending nodrag" aria-label="待你处理">
      {#each pending as item (item.id)}
        <li>
          <span class="frame-node-pending-why">{pendingLabels[item.why]}</span>
          <span class="frame-node-pending-text">{summary(item)}</span>
          <button type="button" class="subtle-button" disabled={frame.frozen} onclick={() => data.actions?.resolve?.(frame.id, item.id, true)}>接受</button>
          <button type="button" class="subtle-button" onclick={() => data.actions?.resolve?.(frame.id, item.id, false)}>忽略</button>
        </li>
      {/each}
    </ul>
  {:else if frame.rules.text}<p class="frame-node-rules">规则：{frame.rules.text}</p>{/if}
</section>
