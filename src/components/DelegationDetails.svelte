<script lang="ts">
  import type { DelegationObject, GateResult } from '../lib/api'
  import type { DelegationControls } from '../lib/delegation'
  import { checkpointLine, delegateLabel, formatDuration, localTime, phaseLabels } from '../lib/delegation'
  import { completionLabels, gateStatusLabels, objectTypeLabel } from '../lib/naming'
  import { humanRequestId } from '../lib/request-id'
  import AcceptanceCard from './AcceptanceCard.svelte'
  import ClarificationPanel from './ClarificationPanel.svelte'
  import DelegationStatus from './DelegationStatus.svelte'
  let { object, controls }: { object: DelegationObject; controls: DelegationControls } = $props()
  const reportStates: Record<string, string> = {
    not_started: '未开始', in_progress: '进行中', waiting: '等待中', blocked: '受阻', claimed_done: '已声称完成',
  }
  // Shape and text as well as colour: ✓ passed, ✕ blocking, ◷ waiting, – clear, ↷ waived.
  const gateMarks: Record<string, [string, string]> = {
    passed: ['✓', 'evidence-check'], failed: ['✕', 'evidence-failed'], error: ['✕', 'evidence-failed'],
    pending: ['◷', 'evidence-unproven'], clear: ['–', 'evidence-claimed'], waived: ['↷', 'evidence-unproven'],
  }
  // FR-72: only a declared hard gate that holds the object back can be waived; never the kit's own checks.
  const DAY_MS = 24 * 3600 * 1000
  const waivable = (gate: GateResult) => gate.hard && !gate.builtin && (gate.status === 'failed' || gate.status === 'pending')
  let waiving = $state<string | null>(null)
  let waiveReason = $state('')
  let waiveDays = $state(7)
  let waiveOk = $derived(Boolean(waiveReason.trim()) && Number.isInteger(waiveDays) && waiveDays >= 1 && waiveDays <= 30)
  let completion = $derived(object.completion)
  let locked = $derived(controls.disabled || controls.busy || Boolean(controls.pending))
  let reports = $derived(controls.view.report_requests.filter((request) => request.object === object.id))
  let offer = $derived(controls.ruleOffer?.object === object.id ? controls.ruleOffer : null)
  let ruleText = $state('')
  let offerSource = $state<string | null>(null)
  $effect(() => {
    if (offer && offer.source !== offerSource) { offerSource = offer.source; ruleText = offer.text }
  })
</script>

<section class="delegation-details" aria-label="委托">
  <h3>委托 · {phaseLabels[object.phase]}</h3>
  <DelegationStatus {controls} />
  <dl class="source-list">
    <dt>类别</dt><dd>{object.work_kind}{object.work_kind !== object.default_work_kind ? `（原 ${object.default_work_kind}）` : ''}</dd>
    {#if object.brief.why}<dt>目的</dt><dd>{object.brief.why}</dd>{/if}
    <dt>完成标准</dt><dd class="multiline">{object.brief.done_when.map((item, index) => `${index + 1}. ${item}`).join('\n') || '未定稿'}</dd>
    {#if object.brief.limits}<dt>边界</dt><dd>{object.brief.limits}</dd>{/if}
    {#if object.brief.checkpoints?.length}<dt>检查点</dt><dd class="multiline">{object.brief.checkpoints.map(checkpointLine).join('\n')}</dd>{/if}
    {#if object.brief.self_check}<dt>自检</dt><dd>{object.brief.self_check}</dd>{/if}
    <dt>定稿</dt><dd>{object.brief_set_by ?? '尚未由人定稿'}</dd>
  </dl>

  {#if completion && (completion.gates.length || completion.error)}
    <h3>{objectTypeLabel('Gate')} · {completionLabels[completion.verdict]}</h3>
    <ul class="evidence-list" aria-label={objectTypeLabel('Gate')}>
      {#if completion.error}
        <li><span class="evidence-mark evidence-failed"><span aria-hidden="true">✕</span> {gateStatusLabels.error}</span>
          <span class="evidence-detail">{completion.error}</span></li>
      {/if}
      {#each completion.gates as gate (gate.gate)}
        <li>
          <span class="evidence-mark {gateMarks[gate.status][1]}"><span aria-hidden="true">{gateMarks[gate.status][0]}</span> {gateStatusLabels[gate.status]}</span>
          <span>{gate.gate}{gate.check && gate.check !== gate.gate ? `（${gate.check}）` : ''}{gate.hard ? '' : ' · 只提醒'}</span>
          {#if gate.reason}<span class="evidence-detail">{gate.reason}</span>{/if}
          {#if waivable(gate) && waiving !== gate.gate}
            <button type="button" disabled={locked} onclick={() => { waiving = gate.gate; waiveReason = ''; waiveDays = 7 }}>豁免…</button>
          {/if}
          {#if waiving === gate.gate}
            <form class="draft-edit-form" aria-label="豁免检查 {gate.gate}" onsubmit={(event) => {
              event.preventDefault()
              if (!waiveOk || locked) return
              const until = new Date(Date.now() + waiveDays * DAY_MS).toISOString()
              controls.onAction({ kind: 'waive', object: object.id, data: { gate: gate.gate, reason: waiveReason.trim(), until } })
              waiving = null
            }}>
              <fieldset class="draft-edit-fields" disabled={locked}>
                <label>豁免理由（一句话）<input maxlength="200" bind:value={waiveReason} /></label>
                <label>有效天数（最长 30 天，到期自动失效、不续期）<input type="number" min="1" max="30" step="1" bind:value={waiveDays} /></label>
                <div class="button-row">
                  <button class="primary-button" type="submit" disabled={!waiveOk || locked}>豁免</button>
                  <button type="button" onclick={() => { waiving = null }}>取消</button>
                </div>
              </fieldset>
            </form>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}

  {#if object.phase === 'clarifying' || object.phase === 'proposed'}
    {#key object.clarification?.event.eid ?? object.id}
      <ClarificationPanel {object} disabled={locked}
        onFinalize={(data) => controls.onAction({ kind: 'finalize', object: object.id, data })} />
    {/key}
  {/if}

  {#if object.card}
    {#key object.card.attempt}
      <AcceptanceCard {object} card={object.card} disabled={locked} onAction={controls.onAction} />
    {/key}
  {/if}

  {#if offer}
    <form class="draft-edit-form" aria-label="记为规则" onsubmit={(event) => {
      event.preventDefault()
      if (!ruleText.trim() || locked) return
      controls.onAction({ kind: 'rule', object: `rule-${humanRequestId()}`, data: { kind: offer.kind, text: ruleText.trim(), source: offer.source } })
    }}>
      <fieldset class="draft-edit-fields" disabled={locked}>
        <label>把这次纠正记为规则？（类别 {offer.kind}）<textarea rows="2" bind:value={ruleText}></textarea></label>
        <div class="button-row">
          <button class="primary-button" type="submit" disabled={!ruleText.trim() || locked}>记为规则</button>
          <button type="button" onclick={controls.onDismissRule}>不记</button>
        </div>
      </fieldset>
    </form>
  {/if}

  {#if reports.length}
    <h3>汇报请求</h3>
    <ul class="report-list">
      {#each reports as request (request.id)}
        <li>
          <strong>{request.text}</strong> · 截止 {localTime(request.due)}
          {#if request.reply}
            <span>· {reportStates[request.reply.data.state] ?? request.reply.data.state}：{request.reply.data.summary}（受托者自述）</span>
          {:else if request.overdue || (request.open && Date.parse(request.due) < Date.now())}
            <span class="evidence-mark evidence-failed"><span aria-hidden="true">◷</span> 过了期限未汇报</span>
          {:else if request.open}
            <span>· 等待汇报</span>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}

  {#if object.attempts.length}
    <details class="sample-history">
      <summary>托付记录（{object.attempts.length}）</summary>
      <ol class="diagnostics">
        {#each object.attempts as attempt (attempt.id)}
          <li>
            {attempt.id} · {delegateLabel(attempt.delegate)} · 委托于 {localTime(attempt.delegated.at)}
            {#if attempt.delivered}· 交付于 {localTime(attempt.delivered.at)}{/if}
            {#if attempt.done}· 完成（{attempt.done_by === 'human' ? '人确认' : '检查'}）{/if}
            · 越界 {attempt.out_of_bounds ?? '未接入'}
            {#if attempt.verdict}· {attempt.verdict.kind === 'confirmed' ? '已确认' : `已退回：${attempt.verdict.reason}`} · 验收 {formatDuration(attempt.verdict.review_ms)}{/if}
          </li>
        {/each}
      </ol>
    </details>
  {/if}
</section>
