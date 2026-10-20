<script lang="ts">
  import { untrack } from 'svelte';
  import { demoNodes, demoEdges, type DemoNode } from '../lib/demo';
  let { nodes = demoNodes, edges = demoEdges, demo = true, candidate = false }: {
    nodes?: DemoNode[]; edges?: string[][]; demo?: boolean; candidate?: boolean;
  } = $props();

  let selectedId = $state('');
  let zoom = $state(1);
  let offset = $state({ x: 0, y: 0 });
  let width = $state(0);
  let height = $state(0);
  let drag: { x: number; y: number; originX: number; originY: number } | null = null;
  let dragging = $state(false);
  const selected = $derived(nodes.find(node => node.id === selectedId));
  const bounds = $derived({
    left: Math.min(0, ...nodes.map(node => node.x)),
    top: Math.min(0, ...nodes.map(node => node.y)),
    right: Math.max(400, ...nodes.map(node => node.x + (node.width ?? 240))),
    bottom: Math.max(300, ...nodes.map(node => node.y + (node.height ?? 144))),
  });

  function fit() {
    if (!width || !height) return;
    const next = Math.min((width - 70) / (bounds.right - bounds.left), (height - 100) / (bounds.bottom - bounds.top), 1.15);
    zoom = Math.max(0.001, next);
    offset = { x: (width - (bounds.right - bounds.left) * zoom) / 2 - bounds.left * zoom, y: (height - (bounds.bottom - bounds.top) * zoom) / 2 - bounds.top * zoom };
  }
  $effect(() => { if (width && height) untrack(fit); });

  function scale(factor: number, x = width / 2, y = height / 2) {
    const next = Math.max(0.001, Math.min(2, zoom * factor));
    offset = { x: x - (x - offset.x) * next / zoom, y: y - (y - offset.y) * next / zoom };
    zoom = next;
  }
  function startPan(event: PointerEvent) {
    if (event.button !== 0 || (event.target as Element).closest('button')) return;
    drag = { x: event.clientX, y: event.clientY, originX: offset.x, originY: offset.y };
    dragging = true;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  }
  function pan(event: PointerEvent) {
    if (drag) offset = { x: drag.originX + event.clientX - drag.x, y: drag.originY + event.clientY - drag.y };
  }
  function endPan() { drag = null; dragging = false; }
  function wheel(event: WheelEvent) {
    event.preventDefault();
    const bounds = (event.currentTarget as HTMLElement).getBoundingClientRect();
    if (event.ctrlKey || event.metaKey) scale(Math.exp(-event.deltaY * 0.008), event.clientX - bounds.left, event.clientY - bounds.top);
    else offset = { x: offset.x - event.deltaX, y: offset.y - event.deltaY };
  }
  function keyboard(event: KeyboardEvent) {
    if ((event.target as HTMLElement).closest('button')) return;
    const delta: Record<string, [number, number]> = { ArrowLeft: [40, 0], ArrowRight: [-40, 0], ArrowUp: [0, 40], ArrowDown: [0, -40] };
    if (delta[event.key]) {
      event.preventDefault();
      offset = { x: offset.x + delta[event.key][0], y: offset.y + delta[event.key][1] };
    } else if (event.key === 'Home') { event.preventDefault(); fit(); }
    else if(event.key==='Escape') selectedId='';
  }
  function edgePath(from: string, to: string) {
    const a = nodes.find(node => node.id === from);
    const b = nodes.find(node => node.id === to);
    if (!a || !b) return '';
    const start = a.x + (a.width ?? 240), sy = a.y + (a.height ?? 144) / 2, ey = b.y + (b.height ?? 144) / 2;
    return `M ${start} ${sy} C ${start + 55} ${sy}, ${b.x - 55} ${ey}, ${b.x} ${ey}`;
  }
</script>

<div class="canvas-layout">
  <!-- Custom spatial canvas: focus enables the arrow-key and Home equivalents of pointer navigation. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <section class="canvas-surface" class:dragging bind:clientWidth={width} bind:clientHeight={height}
    role="application" aria-label="项目关系画布，方向键平移，Home 适应画布" tabindex="0"
    onpointerdown={startPan} onpointermove={pan} onpointerup={endPan} onpointercancel={endPan} onlostpointercapture={endPan}
    onwheel={wheel} onkeydown={keyboard}>
    <div class="canvas-world" style:transform="translate({offset.x}px, {offset.y}px) scale({zoom})">
      <svg class="connections" width="990" height="650" aria-hidden="true">
        <defs><marker id="arrow" markerWidth="7" markerHeight="7" refX="6" refY="3.5" orient="auto"><path d="M0,0 L7,3.5 L0,7" fill="#b7c4bf" /></marker></defs>
        {#each edges as [from, to]}<path d={edgePath(from, to)} marker-end="url(#arrow)" />{/each}
      </svg>
      {#each nodes as node}
        <button type="button" class="graph-node {node.tone}" class:group={node.kind === 'group'} class:selected={selected?.id === node.id}
          style:left="{node.x}px" style:top="{node.y}px" style:width="{node.width ?? 240}px" style:height="{node.height ?? 144}px" aria-pressed={selected?.id === node.id}
          aria-label="{node.title}，{node.status}" onclick={() => selectedId = node.id}>
          <span class="node-top"><span>{node.layer}</span><span>{node.id}</span></span>
          <strong>{node.title}</strong>
          <span class="node-status"><span aria-hidden="true">{node.symbol}</span> {node.status}</span>
        </button>
      {/each}
    </div>
    {#if !nodes.length}<div class="canvas-empty"><strong>这里还没有节点</strong><p>当前提交没有可展示的节点，可刷新或切换项目。</p></div>{/if}

  </section>

  {#if selected}<aside class="node-details canvas-inspector" aria-label="节点详情" aria-live="polite">
    <button type="button" class="canvas-close secondary" onclick={() => selectedId=''}>关闭节点详情</button>
    <div class="detail-label">节点详情 <span>{demo ? '演示' : '只读'}</span></div>
    <p class="detail-id">{selected.id} <span> / {selected.layer}</span></p>
    <h2>{selected.title}</h2>
    <span class="detail-status {selected.tone}">{selected.symbol} {selected.status}</span>
    <p class="detail-description">{selected.description}</p>
    <dl>{#each selected.facts as [label, value]}<div><dt>{label}</dt><dd>{value}</dd></div>{/each}</dl>
    {#if selected.content}<section class="node-content" aria-label="节点原文"><h3>原文 / 文件路径</h3><pre>{selected.content}</pre></section>{/if}
    {#if selected.verification}<section class="node-content" aria-label="验证结果"><h3>验证结果 / 原始执行</h3><pre>{selected.verification}</pre></section>{/if}
    <div class="detail-boundary"><span aria-hidden="true">◇</span><div><strong>结构只在人批准处改变</strong><p>{demo ? '此原型展示交互与信息结构。批准、执行和证据写回尚未接入。' : candidate ? '真实候选尚未批准。本阶段只读展示；编辑、保存与批准尚未接入，不改变基线或节点位置。' : '本视图不会修改节点位置、批准结构或写入仓库。未提交的改动不进入此视图。'}</p></div></div>
    <div class="detail-hint">点击其他节点，探索它与项目的关系。</div>
  </aside>{/if}
</div>
