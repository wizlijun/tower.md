<script lang="ts">
 import {SvelteFlow,Controls,Background,BackgroundVariant,ConnectionMode} from '@xyflow/svelte';
 import '@xyflow/svelte/dist/style.css';
 import CanvasNode from './CanvasNode.svelte';
 import CanvasEdge from './CanvasEdge.svelte';
 import {projectComparison,type ComparisonMode} from '../lib/review-canvas';
 import type {CanvasComparison} from '../lib/review';
 let {comparison,interfaceModules}:{comparison?:CanvasComparison;interfaceModules:string[]}=$props();
 let mode=$state<ComparisonMode>('overlay');
 const projection=$derived(comparison?.state==='available'?projectComparison(comparison,mode,interfaceModules):null);
 const nodeTypes={canvas:CanvasNode};const edgeTypes={canvas:CanvasEdge};
</script>
<section class="review-canvas" aria-label="主干与草稿画布对照">
 <h3>主干与草稿画布对照 · 只读</h3>
 {#if comparison?.state==='available' && projection}
  <p>权威 main：<code>{comparison.main_revision}</code> · 当前基线：{comparison.baseline??'尚无已认证基线'}</p>
  <p>━ 主干为实线；┄ 拟议草稿为虚线。保留两侧原始坐标，切换视图不会保存或批准变更。</p>
  <div class="review-canvas-modes" role="group" aria-label="画布对照模式">
   <button type="button" class="secondary" aria-pressed={mode==='overlay'} onclick={()=>mode='overlay'}>叠加</button>
   <button type="button" class="secondary" aria-pressed={mode==='baseline'} onclick={()=>mode='baseline'}>主干</button>
   <button type="button" class="secondary" aria-pressed={mode==='draft'} onclick={()=>mode='draft'}>草稿</button>
  </div>
  {#if !comparison.baseline_canvas}<p>权威 main 尚无画布；主干视图为空，不以目录图替代。</p>{/if}
  {#if projection.endpoints.length}<ul class="review-interface-endpoints" aria-label="Interface 影响模块">{#each projection.endpoints as endpoint}<li>◆ {endpoint.id}：主干{endpoint.baseline?'已高亮':'无可显示节点'}；草稿{endpoint.draft?'已高亮':'无可显示节点'}</li>{/each}</ul>{/if}
  <div class="review-flow">
   {#key mode}
   <SvelteFlow nodes={projection.nodes} edges={projection.edges} {nodeTypes} {edgeTypes} fitView fitViewOptions={{padding:0.2,maxZoom:1.15}}
    nodeOrigin={[0,0]} zIndexMode="manual" elevateNodesOnSelect={false} connectionMode={ConnectionMode.Loose}
    nodesDraggable={false} nodesConnectable={false} elementsSelectable={false} deleteKey={null} selectionKey={null} multiSelectionKey={null} selectionOnDrag={false}
    minZoom={0.01} maxZoom={2} panOnScroll={true} zoomOnScroll={false} zoomOnDoubleClick={false} disableKeyboardA11y={true}
    onbeforeconnect={()=>false} onbeforedelete={async()=>false} onbeforereconnect={()=>false}>
    <Background variant={BackgroundVariant.Dots}/><Controls showLock={false}/>
   </SvelteFlow>
   {/key}
   {#if !projection.nodes.length}<p class="flow-empty">此视图没有可显示节点；原始画布条目和逐文件差异仍保留。</p>{/if}
  </div>
  {#if projection.diagnostics.length}<details open class="review-canvas-diagnostics"><summary>画布读取与影响提示（{projection.diagnostics.length}）</summary><pre>{JSON.stringify(projection.diagnostics,null,2)}</pre></details>{/if}
 {:else}<p role="status">画布对照不可用：{comparison?.state==='unavailable'?comparison.reason:'服务端尚未返回权威 main 与草稿画布，不能推断当前基线或构造替代图。'}</p>{/if}
</section>
