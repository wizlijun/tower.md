import {expect,test,vi} from 'vitest';
vi.mock('@tauri-apps/api/core',()=>({invoke:vi.fn()}));
import {invoke} from '@tauri-apps/api/core';
import {currentDraftClient} from './current-draft';
test('当前基线草稿只调用四个固定 native 动作，不传路径、actor或替换正文',async()=>{
 vi.mocked(invoke).mockResolvedValue(null);await currentDraftClient.load('selected');await currentDraftClient.pending('selected');await currentDraftClient.prepare('selected','towermd/B0','main');await currentDraftClient.submit('selected','original');
 expect(vi.mocked(invoke).mock.calls).toEqual([['load_current_baseline',{token:'selected'}],['pending_draft_creations',{token:'selected'}],['prepare_create_baseline_draft',{token:'selected',baseline:'towermd/B0',mainRevision:'main'}],['submit_create_baseline_draft',{token:'selected',requestId:'original'}]]);
});
