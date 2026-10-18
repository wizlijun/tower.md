import {act,cleanup,fireEvent,render,screen,waitFor,within} from '@testing-library/svelte';
import {afterEach,expect,test,vi} from 'vitest';
import RecoveryCard from './RecoveryCard.svelte';
import type {RecoveryClient,RecoveryView} from '../lib/recovery';
afterEach(()=>{cleanup();vi.useRealTimers();});
const id='a'.repeat(64);
function fixture(requestId=id):RecoveryView {const now=Date.now();return {request:{status:'awaiting_app_confirmation',request_id:requestId,project_id:'真实项目',main_revision:'b'.repeat(40),source_commit:'c'.repeat(40),source_seq:9,seq_boundary:4294967305,epoch_boundary:4294967300,source_epoch_high_water:4,source_reference:'main;refs/towermd/snapshot:missing',inputs_digest:'d'.repeat(64),requested_at:now,expires_at:now+300000,nonce:'0192f0aa-0000-7000-8000-000000000001'},human_actor:'human:fixture',can_confirm:true};}
function client(value=fixture()):RecoveryClient{return {load:vi.fn(async()=>value),confirm:vi.fn(async()=>({status:'rebuilt' as const,commit:'e'.repeat(40),seq:4294967308,epoch:4294967301,reason:null}))};}
async function load(){await fireEvent.input(screen.getByRole('textbox',{name:'已登记的恢复请求 ID'}),{target:{value:id}});await fireEvent.click(screen.getByRole('button',{name:'读取原恢复请求'}));await screen.findByRole('region',{name:'原恢复请求详情'});}
test('空态只指引 CLI，非法 ID 不读请求，不存在自动确认',async()=>{
 const api=client();render(RecoveryCard,{token:'selected',client:api});expect(screen.getByText('towermd hub rebuild')).toBeTruthy();expect(screen.queryByRole('button',{name:'确认恢复此请求'})).toBeNull();expect(api.load).not.toHaveBeenCalled();expect(api.confirm).not.toHaveBeenCalled();
 await fireEvent.input(screen.getByRole('textbox'),{target:{value:'../source'}});await fireEvent.click(screen.getByRole('button',{name:'读取原恢复请求'}));await screen.findByRole('alert');expect(api.load).not.toHaveBeenCalled();expect(screen.queryByRole('button',{name:'确认恢复此请求'})).toBeNull();
});
test('明确载入才显示完整原来源、边界和原期限，刷新不延长有效期或确认',async()=>{
 const value=fixture();const api=client(value);render(RecoveryCard,{token:'selected',client:api});await load();const details=screen.getByRole('region',{name:'原恢复请求详情'});
 for(const raw of [id,value.request.project_id,value.request.source_reference,value.request.source_commit,value.request.main_revision,value.request.inputs_digest,String(value.request.seq_boundary),String(value.request.epoch_boundary),String(value.request.source_epoch_high_water),String(value.request.requested_at),String(value.request.expires_at)])expect(within(details).getByText(raw,{selector:'code'})).toBeTruthy();
 expect(within(details).getByText('human:fixture')).toBeTruthy();expect(api.load).toHaveBeenCalledExactlyOnceWith('selected',id);expect(api.confirm).not.toHaveBeenCalled();
 await fireEvent.click(screen.getByRole('button',{name:'读取原恢复请求'}));await waitFor(()=>expect(api.load).toHaveBeenCalledTimes(2));expect(within(screen.getByRole('region',{name:'原恢复请求详情'})).getByText(String(value.request.expires_at),{selector:'code'})).toBeTruthy();expect(screen.getByRole('region',{name:'原恢复请求审计'})).toBeTruthy();expect(api.confirm).not.toHaveBeenCalled();
});
test('只有明确确认点击提交原 ID，完成后不能重复，原卡保留审计',async()=>{
 const api=client();render(RecoveryCard,{token:'selected',client:api});await load();await fireEvent.click(screen.getByRole('button',{name:'确认恢复此请求'}));await screen.findByRole('region',{name:'恢复结果'});expect(api.confirm).toHaveBeenCalledExactlyOnceWith('selected',id);expect(screen.getByText('e'.repeat(40))).toBeTruthy();expect((screen.getByRole('button',{name:'确认恢复此请求'}) as HTMLButtonElement).disabled).toBe(true);
 await fireEvent.input(screen.getByRole('textbox'),{target:{value:'b'.repeat(64)}});expect(screen.queryByRole('region',{name:'原恢复请求详情'})).toBeNull();expect(screen.getByRole('region',{name:'原恢复请求审计'}).textContent).toContain(id);expect(screen.getByRole('region',{name:'原恢复请求审计'}).textContent).toContain('e'.repeat(40));expect(api.load).toHaveBeenCalledTimes(1);expect(api.confirm).toHaveBeenCalledTimes(1);
});
test('到期倒计时只禁点击，原期限与原 ID 不变且不自动 prepare',async()=>{
 vi.useFakeTimers({toFake:['Date','setInterval','clearInterval']});vi.setSystemTime(new Date('2030-01-01T00:00:00Z'));
 const value=fixture();value.request.expires_at=Date.now()+2000;const api=client(value);render(RecoveryCard,{token:'selected',client:api});await load();expect((screen.getByRole('button',{name:'确认恢复此请求'}) as HTMLButtonElement).disabled).toBe(false);
 await act(()=>{vi.advanceTimersByTime(3000);});expect(screen.getByText(/原恢复请求已过期/)).toBeTruthy();expect((screen.getByRole('button',{name:'确认恢复此请求'}) as HTMLButtonElement).disabled).toBe(true);
 await fireEvent.click(screen.getByRole('button',{name:'读取原恢复请求'}));await waitFor(()=>expect(api.load).toHaveBeenCalledTimes(2));expect(screen.getByText(/原恢复请求已过期/)).toBeTruthy();expect(api.confirm).not.toHaveBeenCalled();expect(value.request.expires_at).toBe(new Date('2030-01-01T00:00:02Z').getTime());
});
test.each(['main/private 来源已变化，须新请求','恢复边界 ffffffffffffffffffffffffffffffffffffffff 已持久化，本地安装失败；须新请求','原恢复请求送达状态不确定'])('真实拒绝或不确定保留原诊断且不重新安装：%s',async(message)=>{
 const api=client();api.confirm=vi.fn(async()=>{throw {code:'recovery_failed',message,detail:'原始 HTTP 诊断'};});render(RecoveryCard,{token:'selected',client:api});await load();await fireEvent.click(screen.getByRole('button',{name:'确认恢复此请求'}));expect((await screen.findByRole('alert')).textContent).toContain(message);expect(screen.queryByRole('heading',{name:'恢复已完成'})).toBeNull();
 await fireEvent.click(screen.getByRole('button',{name:'读取原恢复请求'}));await waitFor(()=>expect(api.load).toHaveBeenCalledTimes(2));expect((screen.getByRole('button',{name:'确认恢复此请求'}) as HTMLButtonElement).disabled).toBe(true);expect(screen.getByRole('region',{name:'原恢复请求审计'}).textContent).toContain(message);expect(api.confirm).toHaveBeenCalledTimes(1);
});
test('缺失或无效保存请求没有确认按钮；销毁后的旧项目迟到响应不能显示',async()=>{
 const api=client();api.load=vi.fn(async()=>{throw {code:'recovery_missing',message:'恢复请求不存在；请运行 hub rebuild'};});const panel=render(RecoveryCard,{token:'selected',client:api});await fireEvent.input(screen.getByRole('textbox'),{target:{value:id}});await fireEvent.click(screen.getByRole('button',{name:'读取原恢复请求'}));await screen.findByRole('alert');expect(screen.queryByRole('button',{name:'确认恢复此请求'})).toBeNull();panel.unmount();
 let deliver!:(value:RecoveryView)=>void;const late=client();late.load=vi.fn(()=>new Promise<RecoveryView>(resolve=>deliver=resolve));const old=render(RecoveryCard,{token:'old',client:late});await fireEvent.input(screen.getByRole('textbox'),{target:{value:id}});await fireEvent.click(screen.getByRole('button',{name:'读取原恢复请求'}));await waitFor(()=>expect(late.load).toHaveBeenCalledTimes(1));old.unmount();deliver(fixture());await Promise.resolve();expect(screen.queryByRole('region',{name:'原恢复请求详情'})).toBeNull();expect(late.confirm).not.toHaveBeenCalled();
});
