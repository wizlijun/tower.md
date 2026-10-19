import {expect,test} from 'vitest';
import {attentionItems,moduleSummary,overviewFlow,taskDiagnostics,unresolvedItems} from './live-overview';
import {progressFixture,proposalFixture} from './proposal-attention.test-fixtures';
import type {ProjectOverview} from './project';
const automatic = (id:string,kind='forbidden'):Record<string,unknown> => {
 const bytes=Array.from('docs/A.md',char=>char.charCodeAt(0));const forbidden=kind==='forbidden';
 return {type:'Hold',id,kind,scope:'TaskBranch',gate:forbidden?'G2':'G5',task:'T',run:'R',evidence:'E',commit:'c'.repeat(40),tree:'d'.repeat(40),authority:'b'.repeat(40),baseline:'towermd/B1',fences:[{name:'module:A',holder:'R',epoch:1}],rejection:forbidden?{kind:'protected',path:bytes}:{kind:'not_latest_main'},path_bytes:forbidden?bytes:null,path:forbidden?'docs/A.md':null,opened_ms:1};
};
test('真实复核标记上卷模块与关注，字符串不造卡，节点坐标仍属基线',()=>{
 const view=progressFixture([]);view.tasks=[{type:'Task',id:'T',title:'实际任务',module:'A',state:'ready',needs_recheck:true},{type:'Task',id:'string',module:'A',state:'running',needs_recheck:'true'}];
 expect(attentionItems(view).map(item=>[item.kind,item.tasks])).toEqual([['recheck',['T']]]);
 expect(moduleSummary(view,'A')).toContain('↻ 需复核 · 1 个任务需复核');expect(view.holds).toEqual([]);
 const overview:ProjectOverview={source:'baseline',revision:view.revision,title:'project',edges:[],diagnostics:[],nodes:[{id:'module',obj:'A',objectType:'Module',title:'A',kind:'text',status:'bound',text:'原文',x:25,y:60,width:300,height:180}]};
 const before=overviewFlow(overview,{...view,tasks:view.tasks.map(task=>({...task,needs_recheck:false}))});const after=overviewFlow(overview,view);
 expect(after.nodes[0].position).toEqual(before.nodes[0].position);expect(after.nodes[0].data.text).toContain('1 个任务需复核');expect(after.nodes[0].class).toBe('live-node-recheck');
 expect(attentionItems({...view,state:'recovery_required'}).some(item=>item.kind==='recheck')).toBe(false);
});
test('正式中断排序及提案优先级保持，派生复核在提案后且不会伪造Review Hold',()=>{
 const view=progressFixture([proposalFixture()]);view.tasks=[{type:'Task',id:'T',module:'A',state:'ready',needs_recheck:true},{type:'Task',id:'blocked',module:'red',state:'blocked'}];
 view.holds=[automatic('forbidden'),{type:'Hold',id:'newer-gate',kind:'gatefailed',gate:'G3',task:'other',opened_ms:20},{type:'Hold',id:'older-gate',kind:'gatefailed',gate:'G3',task:'other',opened_ms:10},{type:'Hold',id:'review',body:{kind:'review'},task:'other',opened_ms:1},{type:'Hold',id:'ask',body:{kind:'ask',question:'actual question'},task:'other',opened_ms:1,attention:true}];
 expect(attentionItems(view).map(item=>item.id)).toEqual(['forbidden','older-gate','newer-gate','review','ask','proposal-1','recheck:T','module:red']);
 expect(unresolvedItems(view).filter(item=>item.kind==='review').map(item=>item.id)).toEqual(['review']);
});
test('自动诊断只有无历史来源标记的当前原件占关注，历史原因保留Z2读取',()=>{
 const view=progressFixture([]);view.tasks=[{type:'Task',id:'T',module:'A',state:'running'}];
 const history={...automatic('history','gatefailed'),source_state:'historical_only',source_reason:'原 Run/fences 已失效'};const unknown={...automatic('unknown'),source_state:'unknown',source_reason:'来源未核实'};
 view.holds=[automatic('current'),history,unknown];expect(attentionItems(view).map(item=>item.id)).toEqual(['current']);expect(moduleSummary(view,'A')).toContain('⛔ 禁止 · 1 项中断');
 expect(taskDiagnostics(view,'T')).toEqual(view.holds);expect(taskDiagnostics(view,'T')[1].source_reason).toBe('原 Run/fences 已失效');
 expect(attentionItems({...view,holds:[history]})).toEqual([]);expect(attentionItems({...view,holds:[{...automatic('unqualified'),gate:'G0'}]})).toEqual([]);expect(moduleSummary({...view,holds:[history]},'A')).not.toContain('项中断');
});
