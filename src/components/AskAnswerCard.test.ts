import {afterEach,expect,test,vi} from 'vitest';
import {act,cleanup,fireEvent,render,screen,waitFor} from '@testing-library/svelte';
import AskAnswerCard from './AskAnswerCard.svelte';
import App from '../App.svelte';
import type {AskAnswerClient,AskAnswerView,PreparedAskAnswer} from '../lib/ask-answer';
import {askView,askOriginal,askConfirmed,askId,askRequestId} from '../lib/ask-answer.fixture';
afterEach(cleanup);
function client(view=askView()):AskAnswerClient{return {load:vi.fn(async()=>view),pending:vi.fn(async()=>[]),prepare:vi.fn(async()=>askOriginal()),submit:vi.fn(async()=>askConfirmed())};}
async function load(){await fireEvent.input(screen.getByLabelText('CLI 返回的问答 Hold ID'),{target:{value:askId}});await fireEvent.click(screen.getByRole('button',{name:'读取问答'}));await screen.findByRole('region',{name:'问答原件'});}
async function answer(){await fireEvent.input(screen.getByLabelText('本次回答'),{target:{value:'先完成原型'}});await fireEvent.click(screen.getByRole('button',{name:'发送本次回答'}));}

test('默认空答，只明确提交后记录；绑定诊断折叠且没有自动批准',async()=>{
 const api=client();render(AskAnswerCard,{token:'selected',client:api});expect(api.load).not.toHaveBeenCalled();await load();
 expect((screen.getByLabelText('本次回答') as HTMLTextAreaElement).value).toBe('');
 expect((screen.getByRole('button',{name:'发送本次回答'}) as HTMLButtonElement).disabled).toBe(true);
 expect(screen.getByText('应该先做哪一步？')).toBeTruthy();expect(screen.queryByRole('button',{name:/批准/})).toBeNull();
 expect(screen.getByText('来源与绑定（诊断）').closest('details')!.open).toBe(false);
 await answer();await screen.findByText('回答已记录');
 expect(api.prepare).toHaveBeenCalledExactlyOnceWith('selected',askView().answer_ticket,'先完成原型');expect(api.submit).toHaveBeenCalledExactlyOnceWith('selected',askRequestId);
 expect(screen.queryByRole('button',{name:'发送本次回答'})).toBeNull();
});
test('uncertain只重试原UUID；准备响应丢失先找原队列不自动新建',async()=>{
 const api=client();api.submit=vi.fn().mockResolvedValueOnce({request_id:askRequestId,state:'uncertain',response:null,error:{code:'request_uncertain',message:'原回执尚未确认'}}).mockResolvedValueOnce(askConfirmed());
 render(AskAnswerCard,{token:'selected',client:api});await load();await answer();await screen.findByText('原回执尚未确认');
 await fireEvent.click(screen.getByRole('button',{name:'重试原请求'}));await screen.findByText('回答已记录');
 expect(api.prepare).toHaveBeenCalledTimes(1);expect(api.submit).toHaveBeenNthCalledWith(1,'selected',askRequestId);expect(api.submit).toHaveBeenNthCalledWith(2,'selected',askRequestId);cleanup();
 const lost=client();lost.pending=vi.fn().mockResolvedValueOnce([]).mockResolvedValueOnce([askOriginal()]);lost.prepare=vi.fn(async()=>{throw new Error('准备响应丢失');});
 render(AskAnswerCard,{token:'selected',client:lost});await load();await answer();await screen.findByRole('region',{name:'原回答等待确认'});
 expect(lost.prepare).toHaveBeenCalledTimes(1);expect(lost.submit).not.toHaveBeenCalled();await fireEvent.click(screen.getByRole('button',{name:'重试原请求'}));await screen.findByText('回答已记录');expect(lost.prepare).toHaveBeenCalledTimes(1);
});
test('当前过期可保存，历史无新回答但可补原回执，已有首答只读',async()=>{
 const expired=askView();expired.recipients[0].actionable=false;render(AskAnswerCard,{token:'selected',client:client(expired)});await load();expect(screen.getByText(/回答不会恢复运行/)).toBeTruthy();await answer();await screen.findByText('回答已记录');cleanup();
 const old=askView();old.current_validity={state:'historical_only',reason:'hub 已重建'};old.opening.stale=true;old.current_hold.opened_seq=0;old.actions=[];old.recipients[0].actionable=false;
 const api=client(old);api.pending=vi.fn(async()=>[askOriginal()]);api.submit=vi.fn(async()=>askConfirmed(true));
 render(AskAnswerCard,{token:'selected',client:api});await load();expect(screen.queryByLabelText('本次回答')).toBeNull();expect(screen.getByText(/不支持重建后的首次新答复/)).toBeTruthy();
 await fireEvent.click(screen.getByRole('button',{name:'重试原请求'}));await screen.findByText('原回答回执已确认 · 仅历史记录');expect(api.prepare).not.toHaveBeenCalled();cleanup();
 const done=askView();done.actions=[];done.current_hold.reply=askConfirmed().response!.receipt.result_ref.reply;
 render(AskAnswerCard,{token:'selected',client:client(done)});await load();expect(screen.getByRole('region',{name:'此前答复'})).toBeTruthy();expect(screen.queryByLabelText('本次回答')).toBeNull();
});
test('换Hold及切项目丢弃迟到读取和prepare返回，不发送旧回答',async()=>{
 let deliver!:(v:AskAnswerView)=>void;const api=client();api.load=vi.fn(()=>new Promise<AskAnswerView>(resolve=>deliver=resolve));
 const panel=render(AskAnswerCard,{token:'old',client:api});
 await fireEvent.input(screen.getByLabelText('CLI 返回的问答 Hold ID'),{target:{value:askId}});await fireEvent.click(screen.getByRole('button',{name:'读取问答'}));await waitFor(()=>expect(api.load).toHaveBeenCalledTimes(1));
 await fireEvent.input(screen.getByLabelText('CLI 返回的问答 Hold ID'),{target:{value:'H-other'}});await act(()=>deliver(askView()));expect(screen.queryByRole('region',{name:'问答原件'})).toBeNull();
 api.load=vi.fn(async()=>askView());await load();let prepared!:(v:PreparedAskAnswer)=>void;api.prepare=vi.fn(()=>new Promise<PreparedAskAnswer>(resolve=>prepared=resolve));await answer();await waitFor(()=>expect(api.prepare).toHaveBeenCalledTimes(1));
 await panel.rerender({token:'new',client:api});await act(()=>prepared(askOriginal()));expect(api.submit).not.toHaveBeenCalled();expect(screen.queryByRole('region',{name:'问答原件'})).toBeNull();expect((screen.getByLabelText('CLI 返回的问答 Hold ID') as HTMLInputElement).value).toBe('');
});
test('配置项目显式展开本次问答，展开不读取或写入，也不移动canvas',async()=>{
 const api=client();render(App,{choose:async()=>({token:'selected',displayPath:'/local/project',hasConfig:true}),load:async()=>({source:'repository' as const,revision:'f'.repeat(40),title:'真实项目',nodes:[],edges:[],diagnostics:[]}),askAnswerClient:api});
 await fireEvent.click(screen.getByRole('button',{name:'打开真实项目'}));await fireEvent.click(screen.getByRole('button',{name:'打开项目'}));await screen.findByRole('button',{name:'本次问答'});await fireEvent.click(screen.getByRole('button',{name:'本次问答'}));
 expect(screen.getByRole('region',{name:'本次问答'})).toBeTruthy();expect(api.load).not.toHaveBeenCalled();expect(api.prepare).not.toHaveBeenCalled();expect(api.submit).not.toHaveBeenCalled();
});
