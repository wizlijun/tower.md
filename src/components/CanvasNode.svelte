<script lang="ts">
  import { Handle, Position, type NodeProps } from '@xyflow/svelte';
  import type { FlowNode } from '../lib/canvas-flow';
  let { data, selected = false }: NodeProps<FlowNode> = $props();
  const sides = [['top',Position.Top],['right',Position.Right],['bottom',Position.Bottom],['left',Position.Left]] as const;
</script>
<div class="flow-card" class:flow-group={data.kind === 'group'} class:flow-opaque={data.opaque} class:review-baseline={data.reviewLayer === 'baseline'} class:review-proposed={data.reviewLayer === 'draft'} class:review-interface={data.interfaceAffected} class:selected aria-label={data.title}>
  {#if data.reviewLayer}<span class="review-layer-label">{data.reviewLayer === 'baseline' ? '━ 主干 · 实线' : '┄ 拟议 · 虚线'}</span>{/if}
  {#if data.interfaceAffected}<span class="review-interface-label">◆ Interface 影响端点 · {data.binding ?? data.canonicalId}</span>{/if}
  <span class="flow-kind">{data.opaque ? '不透明 · 只读' : data.kind}{data.binding ? ` · ${data.binding}` : ''}</span>
  <strong>{data.title}</strong>
  {#if data.kind === 'file'}<p>{data.object?.path ?? String((data.raw as Record<string,unknown>)?.file ?? '')}</p>
  {:else}<p>{data.text}</p>{/if}
  <span class="flow-boundary">{data.editable ? '草稿 · 未批准' : '只读'}</span>
  {#each sides as [side,position]}<Handle id="side:{side}" type="source" {position} isConnectable={data.editable} />{/each}
</div>
