import {expect,test,vi} from 'vitest';
vi.mock('@tauri-apps/api/core',()=>({invoke:vi.fn()}));
import {invoke} from '@tauri-apps/api/core';
import {recoveryClient,decodeRecoveryView,decodeRebuildReport,validRecoveryId} from './recovery';
const id='a'.repeat(64);
const view={request:{status:'awaiting_app_confirmation',request_id:id,project_id:'project',main_revision:'b'.repeat(40),source_commit:'c'.repeat(40),source_seq:9,seq_boundary:4294967305,epoch_boundary:4294967300,source_epoch_high_water:4,source_reference:'main;refs/towermd/snapshot:missing',inputs_digest:'d'.repeat(64),requested_at:1000,expires_at:301000,nonce:'0192f0aa-0000-7000-8000-000000000001'},human_actor:'human:fixture',can_confirm:true};
test('恢复只调用两个固定 native 动作，前端不传来源、期限、目录或人工身份',async()=>{
 vi.mocked(invoke).mockReset().mockResolvedValueOnce(view).mockResolvedValueOnce({status:'rebuilt',commit:'e'.repeat(40),seq:4294967308,epoch:4294967301,reason:null});
 await recoveryClient.load('selected',id);await recoveryClient.confirm('selected',id);
 expect(vi.mocked(invoke).mock.calls).toEqual([['load_rebuild_request',{token:'selected',id}],['confirm_rebuild',{token:'selected',id}]]);
});
test('缺少保存身份、伪造人工身份、错原 ID、不可精确显示边界与伪完成均拒绝',()=>{
 expect(decodeRecoveryView(view,id)).toBe(view);for(const invalid of ['',id.toUpperCase(),'../source','a'.repeat(63),id+'\n'])expect(validRecoveryId(invalid)).toBe(false);
 for(const invalid of [null,{...view,human_actor:'process:cli'},{...view,human_actor:'human:with space'},{...view,human_actor:'human:fixture\n'},{...view,human_actor:'human:fixture\u0085'},{...view,request:{...view.request,request_id:'b'.repeat(64)}},{...view,request:{...view.request,seq_boundary:Number.MAX_SAFE_INTEGER+1}},{...view,request:{...view.request,expires_at:1000}}])expect(()=>decodeRecoveryView(invalid,id)).toThrow();
 for(const invalid of [{status:'ready'}, {status:'rebuilt',commit:null,seq:5,epoch:5,reason:null},{status:'rebuilt',commit:'e'.repeat(40),seq:1.2,epoch:5,reason:null}])expect(()=>decodeRebuildReport(invalid)).toThrow();
});
