<script lang="ts">
  import type { RuntimeAttention } from '../lib/runtime'
  import type { DelegationControls } from '../lib/delegation'
  import { attentionCard, attentionSymbols as symbols, cardText, FIRST_SCREEN, timing, timingLabels } from '../lib/attention'
  import DelegationStatus from './DelegationStatus.svelte'
  import QuickAction from './QuickAction.svelte'
  let { items, budget = FIRST_SCREEN, focus = false, backlog = null, disabled = false, quieted = 0, asleep = 0, status, onOpen, onDismiss, onApprove, onAnswer, onConfirm, onFinishFollowUp, onSeen }: {
    items: RuntimeAttention[]
    /** First-screen items from `attention.budget` (FR-128); the rest wait under "later". */
    budget?: number
    /** Inside a focus window (FR-130): only time-sensitive items interrupt. */
    focus?: boolean
    backlog?: { count: number; threshold: number } | null
    disabled?: boolean
    /** Signals a person quieted in the App ("知道了" or muted). */
    quieted?: number
    /** Items about quiet objects (FR-135): last under "later", never on the first screen. */
    asleep?: number
    /** The delegation channel's state, shown while a signal action is the latest one. */
    status?: DelegationControls
    onOpen: (item: RuntimeAttention) => void
    /** A person quiets a signal; only the App offers this. */
    onDismiss?: (item: RuntimeAttention, mode: 'ack' | 'mute') => void
    /** FR-138: one-step human actions on the card; only the App passes these. */
    onApprove?: (item: RuntimeAttention) => void
    onAnswer?: (item: RuntimeAttention, text: string) => void
    onConfirm?: (item: RuntimeAttention, reviewMs: number) => void
    onFinishFollowUp?: (item: RuntimeAttention) => void
    onSeen?: (item: RuntimeAttention) => void
  } = $props()
  // The App passes the first-screen count, which is zero when every item is about a quiet object.
  let shown = $derived(Math.min(Math.max(budget, 0), FIRST_SCREEN))
</script>

{#snippet entry(item: RuntimeAttention)}
  {@const when = timingLabels[timing(item)]}
  <li>
    <span class="attention-timing"><span aria-hidden="true">{when.symbol}</span> {when.text}</span>
    {#if item.question || item.commit || item.proposal || item.delegation || item.tasks.length}
      <button {disabled} onclick={() => onOpen(item)}><span aria-hidden="true">{symbols[item.kind]}</span> {item.kind} · {item.title}</button>
    {:else}<strong><span aria-hidden="true">{symbols[item.kind]}</span> {item.kind} · {item.title}</strong>{/if}
    <p class="attention-card">{cardText(attentionCard(item))}</p>
    {#if item.recap}
      <div class="recap" aria-label="上次到哪了">
        <strong>上次到哪了</strong>
        <p>结论：{item.recap.conclusion ?? '还没有记下的结论'}</p>
        {#if item.recap.overturned.length}<p>推翻过：{item.recap.overturned.join('；')}</p>{/if}
        <p>未决：{item.recap.open.length ? item.recap.open.join('；') : '没有未决问题'}</p>
      </div>
    {/if}
    <QuickAction {item} {disabled} {onApprove} {onAnswer} {onConfirm} {onFinishFollowUp} {onSeen} />
    {#if item.signal && onDismiss}
      <div class="button-row">
        <button {disabled} onclick={() => onDismiss(item, 'ack')}>知道了</button>
        <button {disabled} onclick={() => onDismiss(item, 'mute')}>静音 7 天</button>
      </div>
    {/if}
  </li>
{/snippet}

{#if backlog}
  <p class="backlog-line"><span aria-hidden="true">≡</span> {backlog.count} 件待验收</p>
{/if}
{#if status}<DelegationStatus controls={status} />{/if}
{#if focus}<p class="panel-description">专注时段：只有必须停下的事会立即通知，其余等下次查看。</p>{/if}
{#if items.length}
  <section aria-label="关注清单">
    <h3>关注清单</h3>
    <ul class="diagnostics">{#each items.slice(0, shown) as item (item.id)}{@render entry(item)}{/each}</ul>
    {#if items.length > shown}
      <details><summary>另有 {items.length - shown} 条已延后{asleep ? `（其中 ${asleep} 条属于休眠对象）` : ''}</summary>
        <ul class="diagnostics">{#each items.slice(shown) as item (item.id)}{@render entry(item)}{/each}</ul>
      </details>
    {/if}
  </section>
{/if}
{#if quieted}<p class="panel-description">{quieted} 条提醒已标为知道了或静音；发送方升级或再次上报时会重新出现。</p>{/if}
