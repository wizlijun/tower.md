<script lang="ts">
  import type { RuntimeAttention } from '../lib/runtime'
  import type { DelegationControls } from '../lib/delegation'
  import { attentionCard, attentionSymbols as symbols, cardText, FIRST_SCREEN } from '../lib/attention'
  import DelegationStatus from './DelegationStatus.svelte'
  let { items, backlog = null, disabled = false, quieted = 0, status, onOpen, onDismiss }: {
    items: RuntimeAttention[]
    backlog?: { count: number; threshold: number } | null
    disabled?: boolean
    /** Signals a person quieted in the App ("知道了" or muted). */
    quieted?: number
    /** The delegation channel's state, shown while a signal action is the latest one. */
    status?: DelegationControls
    onOpen: (item: RuntimeAttention) => void
    /** A person quiets a signal; only the App offers this. */
    onDismiss?: (item: RuntimeAttention, mode: 'ack' | 'mute') => void
  } = $props()
</script>

{#snippet entry(item: RuntimeAttention)}
  <li>
    {#if item.question || item.commit || item.proposal || item.delegation || item.tasks.length}
      <button {disabled} onclick={() => onOpen(item)}><span aria-hidden="true">{symbols[item.kind]}</span> {item.kind} · {item.title}</button>
    {:else}<strong><span aria-hidden="true">{symbols[item.kind]}</span> {item.kind} · {item.title}</strong>{/if}
    <p class="attention-card">{cardText(attentionCard(item))}</p>
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
{#if items.length}
  <section aria-label="关注清单">
    <h3>关注清单</h3>
    <ul class="diagnostics">{#each items.slice(0, FIRST_SCREEN) as item (item.id)}{@render entry(item)}{/each}</ul>
    {#if items.length > FIRST_SCREEN}
      <details><summary>另有 {items.length - FIRST_SCREEN} 条已延后</summary>
        <ul class="diagnostics">{#each items.slice(FIRST_SCREEN) as item (item.id)}{@render entry(item)}{/each}</ul>
      </details>
    {/if}
  </section>
{/if}
{#if quieted}<p class="panel-description">{quieted} 条提醒已标为知道了或静音；发送方升级或再次上报时会重新出现。</p>{/if}
