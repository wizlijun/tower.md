<script lang="ts">
  import type { RuntimeAttention } from '../lib/runtime'
  let { items, backlog }: { items: Pick<RuntimeAttention, 'id' | 'kind' | 'title'>[]; backlog?: number } = $props()
  const symbols = { '禁止': '⊘', '门禁失败': '✕', '提问': '?', '待批提案': '◇', '告警': '!', '抽查': '◎', '汇报过期': '◷' }
</script>

<span class="canvas-project-attention" aria-label="项目关注摘要">
  {#if backlog !== undefined}<span class="backlog-line"><span aria-hidden="true">≡</span> {backlog} 件等你验收</span>{/if}
  {#each items.slice(0, 3) as item (item.id)}
    <span class="canvas-project-concern" class:is-blocking={item.kind === '禁止' || item.kind === '门禁失败'} title={`${item.kind} · ${item.title}`}>
      <span aria-hidden="true">{symbols[item.kind]}</span> {item.kind} · {item.title}
    </span>
  {/each}
  {#if items.length === 0 && backlog === undefined}<span class="canvas-project-hint">选择查看项目态势</span>{/if}
</span>
