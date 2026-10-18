<script lang="ts">
  import { SvelteFlow, Controls, Background, BackgroundVariant, ConnectionMode, type Connection } from '@xyflow/svelte';
  import '@xyflow/svelte/dist/style.css';
  import CanvasNode from './CanvasNode.svelte';
  import CanvasEdge from './CanvasEdge.svelte';
  import { projectFlow, moveEdit, type FlowNode, type FlowEdge } from '../lib/canvas-flow';
  import type { SavedDraft, DraftEdit } from '../lib/draft';
  let { saved, editable = false, onedit, onselect, onrelation }: { saved: SavedDraft; editable?: boolean; onedit?: (edit: DraftEdit) => void; onselect?: (id: string, edge: boolean) => void; onrelation?: (from: string, to: string) => void } = $props();
  let nodes = $state<FlowNode[]>([]); let edges = $state<FlowEdge[]>([]);
  const projection = $derived(projectFlow(saved,editable));
  $effect(() => { nodes = projection.nodes; edges = projection.edges; });
  const nodeTypes = { canvas: CanvasNode }; const edgeTypes = { canvas: CanvasEdge };
  function dragstop({targetNode,nodes:moved}: {targetNode:FlowNode|null; nodes:FlowNode[]}) {
    if (!editable || !targetNode || moved.length !== 1) {nodes = projectFlow(saved,editable).nodes;return;}
    // Resolve the original coordinates from the immutable saved source, never
    // from Flow nodes, which are disposable interaction state.
    const original = projectFlow(saved,editable).nodes.find(node => node.id === targetNode.id);
    const edit = original ? moveEdit(original,targetNode.position) : null;
    if (edit) onedit?.(edit); else nodes = projectFlow(saved,editable).nodes;
  }
  function connect(connection:Connection) { if (editable && connection.source && connection.target) onrelation?.(connection.source,connection.target); }
</script>
<section class="draft-flow" aria-label="草稿关系画布">
  <SvelteFlow bind:nodes bind:edges {nodeTypes} {edgeTypes} fitView fitViewOptions={{padding:0.2,maxZoom:1.15}}
    nodeOrigin={[0,0]} zIndexMode="manual" elevateNodesOnSelect={false} connectionMode={ConnectionMode.Loose}
    nodesDraggable={editable} nodesConnectable={editable} elementsSelectable={true} deleteKey={null} selectionKey={null} multiSelectionKey={null} selectionOnDrag={false}
    minZoom={0.01} maxZoom={2} panOnScroll={true} zoomOnScroll={false} zoomOnDoubleClick={false} disableKeyboardA11y={true} onnodedragstop={dragstop} onbeforeconnect={(value)=>{connect(value);return false;}} onbeforedelete={async()=>false} onbeforereconnect={()=>false}
    onnodeclick={({node})=>onselect?.(node.data.canonicalId,false)} onedgeclick={({edge})=>onselect?.(edge.data?.canonicalId ?? edge.id,true)}>
    <Background variant={BackgroundVariant.Dots}/><Controls showLock={false}/>
  </SvelteFlow>
  {#if projection.diagnostics.length}<details class="flow-diagnostics"><summary>原文读取提示（{projection.diagnostics.length}）</summary><pre>{JSON.stringify(projection.diagnostics,null,2)}</pre></details>{/if}
  {#if !projection.nodes.length}<p class="flow-empty">此草稿没有可显示节点；原始条目仍保留。</p>{/if}
</section>
