<script lang="ts">
  import type { AskKind, GenericRequestView } from '../lib/api'
  import { askDeadlines, askKindLabels, localTime, rfc3339Local, type DelegationControls } from '../lib/delegation'
  import DelegationStatus from './DelegationStatus.svelte'

  // A person asks about one node (FR-116, FR-127); the daemon picks who
  // receives it. Answers, silence and "nobody can answer" show here.
  // FR-127: an answered question can be recorded as a decision when the
  // App passes `decided` (request IDs already recorded).
  let { target, controls, showStatus = false, now = () => new Date(), decided }:
    { target: string; controls: DelegationControls; showStatus?: boolean; now?: () => Date; decided?: ReadonlySet<string> } = $props()
  let decisions = $state<Record<string, string>>({})

  function decide(event: SubmitEvent, request: string) {
    event.preventDefault()
    const body = (decisions[request] ?? '').trim()
    if (!body || blocked) return
    controls.onAction({ kind: 'decision', data: { text: body, source: { kind: 'reply', request } } })
    decisions[request] = ''
  }

  let ask = $state<AskKind>('question')
  let text = $state('')
  let hours = $state(24)
  let requests = $derived((controls.view.requests ?? []).filter((request) => request.data.target === target))
  let blocked = $derived(controls.disabled || controls.busy || !!controls.pending)
  let isFrame = $derived(/^C-[^.]+$/.test(target))
  $effect(() => { if (!isFrame && ask === 'update') ask = 'question' })

  function submit(event: SubmitEvent) {
    event.preventDefault()
    const body = text.trim()
    if (!body || blocked) return
    const due = new Date(now().getTime() + hours * 3_600_000)
    controls.onAction({ kind: 'ask', data: { target, ask, text: body, due: rfc3339Local(due) } })
    text = ''
  }

  function describe(request: GenericRequestView): string {
    if (request.reply) return `✓ 已回答 · ${request.reply.data.from}：${request.reply.data.summary}`
    if (request.withdrawn) return '– 已撤回'
    const silent = request.silence ? '◷ 失联 · ' : ''
    if (request.recipients.length === 0) return `${silent}∅ 无人可答：等待有人声明可回答本节点`
    return `${silent}等待 ${request.recipients.join('、')} 回答，截止 ${localTime(request.data.due)}`
  }
</script>

<section aria-label="提问">
  <h3>提问</h3>
  {#if showStatus}<DelegationStatus {controls} />{/if}
  {#if requests.length}
    <ul class="report-list" aria-label="本节点的请求">
      {#each requests as request (request.id)}
        <li>
          <strong>{askKindLabels[request.data.ask]}</strong> {request.data.text}
          <p class="panel-description">{describe(request)}</p>
          {#if request.reply?.data.refs?.length}<p class="panel-description">依据：{request.reply.data.refs.join('、')}</p>{/if}
          {#if decided && request.data.ask === 'question' && request.reply}
            {#if decided.has(request.id)}<p class="panel-description">已记为决策</p>
            {:else}
              <form class="button-row" aria-label="记为决策" onsubmit={(event) => decide(event, request.id)}>
                <label>决策<input bind:value={decisions[request.id]} maxlength="200" disabled={blocked} /></label>
                <button type="submit" disabled={blocked || !(decisions[request.id] ?? '').trim()}>记为决策</button>
              </form>
            {/if}
          {/if}
          {#if request.open}
            <button disabled={blocked} onclick={() => controls.onAction({ kind: 'withdraw', object: request.id, data: {} })}>撤回请求</button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
  <form class="draft-edit-form" onsubmit={submit}>
    <div class="draft-edit-fields">
      <label>类别
        <select bind:value={ask} disabled={blocked}>
          <option value="question">{askKindLabels.question}</option>
          <option value="report">{askKindLabels.report}</option>
          <!-- FR-156: only a frame can be updated by its rules. -->
          {#if isFrame}<option value="update">{askKindLabels.update}</option>{/if}
        </select>
      </label>
      <label>内容<textarea bind:value={text} rows="3" disabled={blocked}></textarea></label>
      <label>期限
        <select bind:value={hours} disabled={blocked}>
          {#each askDeadlines as deadline (deadline.hours)}<option value={deadline.hours}>{deadline.label}</option>{/each}
        </select>
      </label>
    </div>
    <div class="button-row">
      <button class="primary-button" type="submit" disabled={blocked || !text.trim()}>发出</button>
    </div>
  </form>
</section>
