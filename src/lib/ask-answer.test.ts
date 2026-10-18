import {expect,test,vi} from 'vitest';
vi.mock('@tauri-apps/api/core',()=>({invoke:vi.fn()}));
import {invoke} from '@tauri-apps/api/core';
import {askAnswerClient,decodeAskAnswerView,decodeAskAnswerOutcome} from './ask-answer';
import {askView,askOriginal,askConfirmed,askId,askRequestId} from './ask-answer.fixture';

test('固定命令只传 token、ticket、文本或原UUID，不传actor/path',async()=>{
 const wire=vi.mocked(invoke);wire.mockResolvedValueOnce(askView()).mockResolvedValueOnce(askOriginal()).mockResolvedValueOnce([askOriginal()]).mockResolvedValueOnce(askConfirmed());
 await askAnswerClient.load('selected',askId);
 await askAnswerClient.prepare('selected',askView().answer_ticket,'先完成原型');
 await askAnswerClient.pending('selected',askId);
 await askAnswerClient.submit('selected',askRequestId);
 expect(wire.mock.calls).toEqual([['load_ask_answer',{token:'selected',id:askId}],['prepare_ask_answer',{token:'selected',answerTicket:askView().answer_ticket,text:'先完成原型'}],['pending_ask_answers',{token:'selected',id:askId}],['submit_ask_answer',{token:'selected',requestId:askRequestId}]]);
 wire.mockReset();
});
test('历史有action或回执替换原回答必须拒绝，当前过期仍可回答',()=>{
 const expired=askView();expired.recipients[0].actionable=false;expect(decodeAskAnswerView(expired,askId).actions).toEqual(['answer']);
 const historical=askView();historical.current_validity={state:'historical_only',reason:'rebuild'};historical.opening.stale=true;
 expect(()=>decodeAskAnswerView(historical,askId)).toThrow();
 historical.actions=[];historical.recipients[0].actionable=false;historical.current_hold.opened_seq=0;
 expect(decodeAskAnswerView(historical,askId).current_validity.state).toBe('historical_only');
 const changed=askConfirmed();changed.response!.receipt.result_ref.reply.text='另一份回答';
 expect(()=>decodeAskAnswerOutcome(changed,askRequestId,askOriginal())).toThrow(/已保存原回答/);
});
