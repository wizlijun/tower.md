import {projectCanvas,type CanvasSource,type FlowNode,type FlowEdge,type ReviewLayer} from './canvas-flow';
import type {CanvasComparison} from './review';
export type ComparisonMode = 'overlay'|'baseline'|'draft';
export function projectComparison(comparison:Extract<CanvasComparison,{state:'available'}>,mode:ComparisonMode,interfaceModules:string[]) {
 const nodes:FlowNode[]=[];const edges:FlowEdge[]=[];const diagnostics:unknown[]=[...comparison.diagnostics];
 const isModule=(node:FlowNode,id:string)=>node.data.binding===id&&node.data.object?.id===id&&node.data.object.type==='Module';
 const endpoints:{id:string;baseline:boolean;draft:boolean}[]=Array.from(new Set(interfaceModules),id=>({id,baseline:false,draft:false}));
 function layer(source:CanvasSource,side:ReviewLayer,visible:boolean){
  const flow=projectCanvas(source,false);
  diagnostics.push(...flow.diagnostics.map(detail=>({layer:side,detail})));
  for(const endpoint of endpoints)endpoint[side]=flow.nodes.some(node=>isModule(node,endpoint.id));
  if(!visible)return;
  for(const node of flow.nodes)nodes.push({...node,id:`${side}:${node.id}`,zIndex:node.zIndex,selectable:false,focusable:false,
   data:{...node.data,reviewLayer:side,interfaceAffected:endpoints.some(endpoint=>isModule(node,endpoint.id))}});
  for(const edge of flow.edges)edges.push({...edge,id:`${side}:${edge.id}`,source:`${side}:${edge.source}`,target:`${side}:${edge.target}`,selectable:false,focusable:false,
   class:side==='draft'?'review-proposed-edge':'review-baseline-edge',data:{...edge.data!,reviewLayer:side}});
 }
 if(comparison.baseline_canvas)layer(comparison.baseline_canvas,'baseline',mode!=='draft');
 layer(comparison.draft_canvas,'draft',mode!=='baseline');
 for(const endpoint of endpoints)if(!endpoint.baseline&&!endpoint.draft)diagnostics.push({message:`找不到 Interface 影响模块：${endpoint.id}（主干与草稿均无可显示节点）`});
 return {nodes,edges,diagnostics,endpoints};
}
