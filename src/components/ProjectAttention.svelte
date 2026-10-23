<script lang="ts">
  import type { RuntimeAttention } from '../lib/runtime'
  import { attentionSymbols as symbols, FIRST_SCREEN } from '../lib/attention'
  let { items, backlog, later = 0 }: { items: Pick<RuntimeAttention, 'id' | 'kind' | 'title'>[]; backlog?: number; later?: number } = $props()
</script>

<span class="canvas-project-attention" aria-label="项目关注摘要">
  {#if backlog !== undefined}<span class="backlog-line"><span aria-hidden="true">≡</span> {backlog} 件待验收</span>{/if}
  {#each items.slice(0, FIRST_SCREEN) as item (item.id)}
    <span class="canvas-project-concern" class:is-blocking={item.kind === '禁止' || item.kind === '检查未通过'} title={`${item.kind} · ${item.title}`}>
      <span aria-hidden="true">{symbols[item.kind]}</span> {item.kind} · {item.title}
    </span>
  {/each}
  {#if later}<span class="canvas-project-hint">另有 {later} 条已延后</span>{/if}
  {#if items.length === 0 && backlog === undefined}<span class="canvas-project-hint">选择查看项目态势</span>{/if}
</span>
