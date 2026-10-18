import {beforeEach,expect,test,vi} from 'vitest';
const {invoke}=vi.hoisted(()=>({invoke:vi.fn()}));vi.mock('@tauri-apps/api/core',()=>({invoke}));
import {draftClient,decodeSaved} from './draft';import {savedFixture} from './draft.fixture';
beforeEach(()=>invoke.mockReset());
test('saved wire preserves full raw and requires durable source identity, never upgrades scan completion',()=>{
 const saved=savedFixture();expect(decodeSaved(saved,saved.id)).toBe(saved);
 expect(()=>decodeSaved({...saved,state:'completed'},saved.id)).toThrow('真实保存身份');
 expect(()=>decodeSaved({...saved,revision:null},saved.id)).toThrow('revision');
 expect(()=>decodeSaved({...saved,canvas_view:null},saved.id)).toThrow('原文');
});
test('invoke uses fixed typed edits and retry carries only original request ID',async()=>{
 invoke.mockResolvedValue(savedFixture());await draftClient.load('token','draft-1');
 const edit={action:'move' as const,node:'note',x:3,y:8};await draftClient.prepare('token','draft-1','revision',edit);await draftClient.submit('token','original-id');await draftClient.pending('token','draft-1');
 expect(invoke.mock.calls).toEqual([['load_draft',{token:'token',id:'draft-1'}],['prepare_draft_edit',{token:'token',id:'draft-1',expectedRevision:'revision',edit}],['submit_draft_edit',{token:'token',requestId:'original-id'}],['pending_draft_edits',{token:'token',id:'draft-1'}]]);
});
