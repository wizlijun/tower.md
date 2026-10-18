import {expect,test} from 'vitest';
import {savedFixture} from './draft.fixture';
import {projectComparison} from './review-canvas';
import type {CanvasSource} from './canvas-flow';
import type {CanvasComparison} from './review';
function comparison():Extract<CanvasComparison,{state:'available'}>{
 const baseline=savedFixture();const draft=savedFixture();
 (draft.canvas_view.nodes[0].raw as {x:number;y:number}).x=-250;(draft.canvas_view.nodes[0].raw as {x:number;y:number}).y=740;
 return {state:'available',main_revision:'a'.repeat(40),baseline:'towermd/B0',baseline_canvas:baseline,draft_canvas:draft,diagnostics:[]};
}
test('real canvas projection keeps both same-ID coordinates and namespaced endpoints without a SavedDraft identity',()=>{
 const input=comparison();const before=JSON.stringify(input);
 // Only native canvas/object projections are required; no fake saved receipt or validity.
 const draft:CanvasSource={canvas_view:input.draft_canvas.canvas_view,object_views:input.draft_canvas.object_views};input.draft_canvas=draft;
 const projection=projectComparison(input,'overlay',['A-module']);
 expect(projection.nodes.map(node=>[node.id,node.position])).toEqual([
  ['baseline:A-module',{x:10,y:20}],['baseline:note',{x:400,y:20}],
  ['draft:A-module',{x:-250,y:740}],['draft:note',{x:400,y:20}]
 ]);
 expect(projection.edges.map(edge=>[edge.id,edge.source,edge.target])).toEqual([
  ['baseline:relation','baseline:A-module','baseline:note'],['draft:relation','draft:A-module','draft:note']
 ]);
 expect(projection.edges[0].class).toBe('review-baseline-edge');expect(projection.edges[1].class).toBe('review-proposed-edge');
 for(const node of projection.nodes){expect(node.draggable).toBe(false);expect(node.connectable).toBe(false);expect(node.selectable).toBe(false);expect(node.data.editable).toBe(false);expect(node.deletable).toBe(false);}
 expect(projection.nodes.filter(node=>node.data.interfaceAffected).map(node=>node.id)).toEqual(['baseline:A-module','draft:A-module']);
 expect(projection.nodes[0].data.object?.text).toContain('future: 原样保留');
 const savedBaseline=input.baseline_canvas!;expect(projection.nodes[0].data.raw).toBe(savedBaseline.canvas_view.nodes[0].raw);
 projection.nodes[0].position.x=999;projection.nodes[0].selected=true;
 expect(JSON.stringify(input.baseline_canvas)).toBe(JSON.stringify(JSON.parse(before).baseline_canvas));
 expect(JSON.stringify(input.draft_canvas.canvas_view)).toBe(JSON.stringify(JSON.parse(before).draft_canvas.canvas_view));
});
test('interface endpoints include deleted baseline-only modules and diagnose IDs missing on both sides',()=>{
 const input=comparison();input.draft_canvas.canvas_view.nodes=input.draft_canvas.canvas_view.nodes.filter(node=>node.index!==0);
 const overlay=projectComparison(input,'overlay',['A-module','missing-module']);
 expect(overlay.nodes.find(node=>node.id==='baseline:A-module')?.data.interfaceAffected).toBe(true);
 expect(overlay.endpoints).toEqual([{id:'A-module',baseline:true,draft:false},{id:'missing-module',baseline:false,draft:false}]);
 expect(JSON.stringify(overlay.diagnostics)).toContain('找不到 Interface 影响模块：missing-module');
 expect(JSON.stringify(overlay.diagnostics)).toContain('关系 0 不可交互');
 const onlyDraft=projectComparison(input,'draft',['A-module']);expect(onlyDraft.nodes.every(node=>node.id.startsWith('draft:'))).toBe(true);expect(onlyDraft.endpoints[0].baseline).toBe(true);
 const onlyMain=projectComparison(input,'baseline',[]);expect(onlyMain.nodes.map(node=>node.position)).toEqual([{x:10,y:20},{x:400,y:20}]);
});
test('empty and invalid native entries preserve diagnostics and unknown raw fields without substituting a graph',()=>{
 const input=comparison();input.baseline_canvas=null;input.diagnostics=['未创建主干画布'];input.draft_canvas.canvas_view={nodes:[{index:0,opaque:true,raw:{id:'unknown',x:'invalid',future:{keep:true}}}],edges:[],diagnostics:[{message:'原始 canvas 诊断'}]};
 const before=JSON.stringify(input);const flow=projectComparison(input,'overlay',[]);
 expect(flow.nodes).toEqual([]);expect(flow.edges).toEqual([]);expect(JSON.stringify(flow.diagnostics)).toContain('原始 canvas 诊断');expect(JSON.stringify(flow.diagnostics)).toContain('无可显示几何');expect(JSON.stringify(input)).toBe(before);
 expect(projectComparison(input,'baseline',[]).nodes).toEqual([]);
});
test('a free card or non-Module object sharing a module ID cannot masquerade as an interface endpoint',()=>{
 const input=comparison();input.baseline_canvas=null;
 input.draft_canvas.canvas_view.nodes.push({index:2,opaque:false,raw:{id:'pretend-module',type:'text',text:'只是自由卡片',x:10,y:40,width:200,height:100}},
 {index:3,opaque:false,raw:{id:'interface-card',type:'file',towermd:{obj:'not-a-module'},x:20,y:80,width:200,height:100}});
 input.draft_canvas.object_views.push({id:'not-a-module',path:'docs/tower/interface.md',text:'接口正文',type:'Interface',title:'不是模块'});
 const flow=projectComparison(input,'overlay',['pretend-module','not-a-module']);
 expect(flow.nodes.filter(node=>node.data.interfaceAffected)).toEqual([]);
 expect(flow.endpoints).toEqual([{id:'pretend-module',baseline:false,draft:false},{id:'not-a-module',baseline:false,draft:false}]);
 expect(JSON.stringify(flow.diagnostics)).toContain('找不到 Interface 影响模块：pretend-module');expect(JSON.stringify(flow.diagnostics)).toContain('找不到 Interface 影响模块：not-a-module');
});
