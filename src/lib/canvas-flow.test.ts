import {expect,test} from 'vitest';
import {projectFlow,moveEdit} from './canvas-flow';
import {savedFixture} from './draft.fixture';
test('server raw and object text survive disposable Flow state and only saved/editable projection can move',()=>{
 const saved=savedFixture();const before=JSON.stringify(saved);const view=projectFlow(saved,true);
 expect(view.nodes[0].data.raw).toBe(saved.canvas_view.nodes[0].raw);expect(view.nodes[0].data.object?.text).toContain('future: 原样保留');
 view.nodes[0].selected=true;view.nodes[0].measured={width:999};view.nodes[0].position.x=500;
 expect(JSON.stringify(saved)).toBe(before);
 expect(moveEdit(projectFlow(saved,true).nodes[0],{x:-12.6,y:40.4})).toEqual({action:'move',node:'A-module',x:-13,y:40});
 expect(moveEdit(projectFlow(saved,false).nodes[0],{x:80,y:90})).toBeNull();
 expect(moveEdit(projectFlow(saved,true).nodes[0],{x:NaN,y:40})).toBeNull();
});
test('four kinds, ordinary cyclic edges, bindings and opaque extensions remain distinct',()=>{
 const saved=savedFixture();saved.canvas_view.nodes.push({index:2,opaque:false,raw:{id:'link',type:'link',url:'https://example.invalid',x:1,y:2,width:300,height:180}},{index:3,opaque:false,raw:{id:'group',type:'group',label:'分区',x:-100,y:-100,width:900,height:600}},{index:4,opaque:true,raw:{id:'unknown',type:'future',x:50,y:100,width:30,height:40,extension:'保留'}});
 saved.canvas_view.edges.push({index:1,opaque:false,raw:{id:'back',fromNode:'note',toNode:'A-module',towermd:{obj:'IF-real'}}});
 const view=projectFlow(saved,true);expect(view.nodes.map(node=>node.data.kind)).toEqual(['file','text','link','group','future']);
 expect(view.nodes.at(-1)?.draggable).toBe(false);expect(view.nodes.at(-1)?.data.raw).toBe(saved.canvas_view.nodes.at(-1)?.raw);
 expect(view.edges.map(edge=>[edge.source,edge.target])).toEqual([['A-module','note'],['note','A-module']]);
 expect(view.edges[0].data?.editable).toBe(true);expect(view.edges[1].data?.editable).toBe(false);expect(view.edges[1].data?.binding).toBe('IF-real');
});

test('drag-stop emits Move even when Flow position aliases a node already mutated in place',()=>{
 const saved=savedFixture();const view=projectFlow(saved,true);const node=view.nodes[0];const aliasedPosition=node.position;
 aliasedPosition.x=125.6;aliasedPosition.y=-37.4;
 expect(moveEdit(node,aliasedPosition)).toEqual({action:'move',node:'A-module',x:126,y:-37});
 expect((saved.canvas_view.nodes[0].raw as {x:number;y:number}).x).toBe(10);
 expect(moveEdit(node,{x:10,y:20})).toBeNull();
});
