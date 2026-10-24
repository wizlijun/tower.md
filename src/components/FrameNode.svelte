<script lang="ts">
  import { NodeResizer, type NodeProps } from '@xyflow/svelte'
  import type { FramePending } from '../lib/api'
  import { FRAME_MAX_SIZE, FRAME_MIN_SIZE, cardCells, cellRect, frameLanes, pendingLabels, shownCards, withUntil, type FrameNode, type ShownCard } from '../lib/frames'
  import { setByLabels } from '../lib/naming'

  let { data, draggable, width, height }: NodeProps<FrameNode> = $props()
  let frame = $derived(data.frame)
  let w = $derived(width ?? frame.place.width)
  let h = $derived(height ?? frame.place.height)
  let lanes = $derived(frameLanes(frame.layout, w, h))
  let cells = $derived(cardCells(frame, shownCards(frame)))
  let pending = $derived(frame.pending ?? [])
  let actions = $derived(frame.frozen ? undefined : data.actions)

  // A person's writes, in the App only (H2). Nothing here moves a card the daemon placed.
  let adding = $state(false)
  let title = $state('')
  let editing = $state<string | null>(null)
  let editingRules = $state(false)
  let rulesText = $state('')
  let laneMax = $state('')
  let until = $state('')

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
    editingRules = !editingRules
  }
  function saveRules(event: SubmitEvent) {
    event.preventDefault()
    const max = laneMax.trim() ? Number(laneMax) : undefined
    const rules = withUntil({ ...frame.rules, text: rulesText.trim() }, until || null)
    if (max === undefined) delete rules.per_lane_max
    else rules.per_lane_max = max
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
  {#each cells as cell (JSON.stringify([cell.x, cell.y]))}
    {@const rect = cellRect(frame.layout, w, h, cell.x, cell.y)}
    <ol class="frame-cell" style={`left: ${rect.left}px; top: ${rect.top}px; width: ${rect.width}px; max-height: ${rect.height}px;`}>
      {#each cell.cards as card (card.id)}
        <li class="frame-card" class:is-proposed={card.kind === 'proposed'} class:is-bound={card.kind === 'bound'} class:is-done={card.done} class:is-waiting={card.waiting.length > 0}>
          <button type="button" class="frame-card-title nodrag" disabled={card.kind !== 'free' || !actions?.card}
            onclick={() => (editing = editing === card.id ? null : card.id)} title={card.body || card.title}>
            {#if card.kind === 'proposed'}<span class="frame-card-mark" aria-hidden="true">◇</span>{/if}
            {#if card.kind === 'bound'}<span class="frame-card-mark" aria-hidden="true">⛓</span>{/if}
            {card.title}
          </button>
          {#if card.kind === 'proposed'}<span class="frame-card-note">提案 · 待你处理</span>
          {:else if card.waiting.length}<span class="frame-card-note"><span aria-hidden="true">◇</span> {marks(card)}</span>{/if}
          {#if editing === card.id && actions?.card}
            <div class="frame-card-edit nodrag">
              <select aria-label="状态" value={card.state ?? ''} onchange={(event) => { actions?.card?.(frame.id, card.id, { op: 'move', state: event.currentTarget.value }); editing = null }}>
                {#each frame.states as state (state)}<option value={state}>{state}</option>{/each}
              </select>
              <button type="button" class="subtle-button" onclick={() => { actions?.card?.(frame.id, card.id, { op: 'archive' }); editing = null }}>归档</button>
            </div>
          {/if}
        </li>
      {/each}
      {#if cell.hidden}<li class="frame-card-more" title="超过每栏上限的卡片已折叠；请合并或归档">+{cell.hidden}</li>{/if}
    </ol>
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
    {#if actions?.card && frame.mode !== 'bound'}<button type="button" class="frame-node-action nodrag" onclick={() => (adding = !adding)}>+ 卡片</button>{/if}
    {#if data.actions?.rules}<button type="button" class="frame-node-action nodrag" onclick={openRules}>规则</button>{/if}
  </header>
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
      <button type="submit" class="primary-button">保存规则</button>
    </form>
  {/if}
  {#if pending.length && data.actions?.resolve}
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
