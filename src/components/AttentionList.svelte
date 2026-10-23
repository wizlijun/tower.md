<script lang="ts">
  import type { RuntimeAttention } from '../lib/runtime'
  let { items, backlog = null, disabled = false, onOpen }: {
    items: RuntimeAttention[]
    backlog?: { count: number; threshold: number } | null
    disabled?: boolean
    onOpen: (item: RuntimeAttention) => void
  } = $props()
</script>

{#snippet entry(item: RuntimeAttention)}
  <li>
    {#if item.question || item.commit || item.proposal || item.delegation || item.tasks.length}
      <button {disabled} onclick={() => onOpen(item)}>{item.kind} · {item.title}</button>
    {:else}<strong>{item.kind} · {item.title}</strong>{/if}
    <p class="panel-description">{item.modules.length ? `影响模块：${item.modules.join('、')}` : '影响范围：项目'}
      {item.tasks.length ? ` · ${item.tasks.length} 个任务` : ''}</p>
    {#if item.detail}<p>{item.detail}</p>{/if}
    <p class="panel-description">{item.action}</p>
  </li>
{/snippet}

{#if backlog}
  <p class="backlog-line"><span aria-hidden="true">≡</span> {backlog.count} 件待验收</p>
{/if}
{#if items.length}
  <section aria-label="关注清单">
    <h3>关注清单</h3>
    <ul class="diagnostics">{#each items.slice(0, 3) as item (item.id)}{@render entry(item)}{/each}</ul>
    {#if items.length > 3}
      <details><summary>更多关注（{items.length - 3}）</summary>
        <ul class="diagnostics">{#each items.slice(3) as item (item.id)}{@render entry(item)}{/each}</ul>
      </details>
    {/if}
  </section>
{/if}
