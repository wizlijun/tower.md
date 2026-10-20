import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { afterEach, expect, test, vi } from 'vitest';
import BaselineHistory from './BaselineHistory.svelte';
import { savedFixture } from '../lib/draft.fixture';
import type { CurrentBaseline, CurrentDraftClient } from '../lib/current-draft';

afterEach(cleanup);
const revision='a'.repeat(40);
function fixture(value:CurrentBaseline|null={baseline:'towermd/B2',main_revision:revision}):CurrentDraftClient {
  return {load:vi.fn(async()=>value),pending:vi.fn(),prepare:vi.fn(),submit:vi.fn()};
}
function noWrites(client:CurrentDraftClient) {
  expect(client.pending).not.toHaveBeenCalled();expect(client.prepare).not.toHaveBeenCalled();expect(client.submit).not.toHaveBeenCalled();
}

test('只读取已证明的最新基线，显示连续标签与来源，不推定逐版本出版回执',async()=>{
  const client=fixture();render(BaselineHistory,{token:'selected',client});
  const chain=await screen.findByRole('list',{name:'已核验基线版本'});
  expect(within(chain).getAllByRole('listitem').map(item=>item.textContent)).toEqual(['towermd/B0','towermd/B1','towermd/B2']);
  expect(screen.getByText(revision)).toBeTruthy();expect(client.load).toHaveBeenCalledExactlyOnceWith('selected');
  expect(screen.queryByText(/出版已记录/)).toBeNull();expect(screen.queryByRole('region',{name:'上次读取草稿'})).toBeNull();noWrites(client);
});

test('null明确显示空链，真实来源错误清掉旧链并允许显式重试',async()=>{
  const client=fixture(null);vi.mocked(client.load).mockResolvedValueOnce(null).mockRejectedValueOnce(new Error('原来源已变化')).mockResolvedValue({baseline:'towermd/B1',main_revision:'b'.repeat(40)});
  render(BaselineHistory,{token:'selected',client});await screen.findByText('尚无已核验基线。');
  expect(screen.queryByRole('list',{name:'已核验基线版本'})).toBeNull();
  await fireEvent.click(screen.getByRole('button',{name:'重新读取基线链'}));await screen.findByText('原来源已变化');
  expect(screen.queryByRole('list',{name:'已核验基线版本'})).toBeNull();
  await fireEvent.click(screen.getByRole('button',{name:'重试读取基线链'}));
  const chain=await screen.findByRole('list',{name:'已核验基线版本'});
  expect(within(chain).getAllByRole('listitem').map(item=>item.textContent)).toEqual(['towermd/B0','towermd/B1']);
  expect(screen.getByText('b'.repeat(40))).toBeTruthy();noWrites(client);
});

test.each([{baseline:'towermd/B02',main_revision:revision},{baseline:'towermd/B2',main_revision:'not-an-oid'}])('不完整响应不构造版本链：%o',async value=>{
  const client=fixture(value);render(BaselineHistory,{token:'selected',client});await screen.findByRole('alert');
  expect(screen.queryByRole('list',{name:'已核验基线版本'})).toBeNull();noWrites(client);
});

test('切token拒绝旧读取迟到结果，卸载后也不会恢复旧链',async()=>{
  let deliver!:(value:CurrentBaseline)=>void;
  const client=fixture();vi.mocked(client.load).mockImplementation(token=>token==='old'?new Promise(resolve=>{deliver=resolve;}):Promise.resolve({baseline:'towermd/B1',main_revision:'b'.repeat(40)}));
  const app=render(BaselineHistory,{token:'old',client});await waitFor(()=>expect(client.load).toHaveBeenCalledWith('old'));
  await app.rerender({token:'new',client});await screen.findByText('b'.repeat(40));
  deliver({baseline:'towermd/B9',main_revision:revision});await Promise.resolve();
  expect(screen.queryByText('towermd/B2')).toBeNull();expect(screen.queryByText(revision)).toBeNull();
  expect(client.load).toHaveBeenCalledTimes(2);noWrites(client);
  let afterDestroy!:(value:CurrentBaseline)=>void;
  vi.mocked(client.load).mockImplementationOnce(()=>new Promise(resolve=>{afterDestroy=resolve;}));
  await fireEvent.click(screen.getByRole('button',{name:'重新读取基线链'}));await waitFor(()=>expect(client.load).toHaveBeenCalledTimes(3));
  app.unmount();afterDestroy({baseline:'towermd/B9',main_revision:revision});await Promise.resolve();
  expect(screen.queryByRole('region',{name:'基线版本链'})).toBeNull();
});

test('仅以实际已读草稿展示末端，目标已在链内时明确是历史草稿',async()=>{
  const client=fixture({baseline:'towermd/B1',main_revision:revision});const draft=savedFixture('c'.repeat(40));draft.draft.bundle.target_baseline='towermd/B2';
  const app=render(BaselineHistory,{token:'selected',client,draft});await screen.findByRole('list',{name:'已核验基线版本'});
  const known=screen.getByRole('region',{name:'上次读取草稿'});expect(known.textContent).toContain(draft.id);expect(known.textContent).toContain(draft.revision);expect(known.textContent).toContain('towermd/B2');expect(known.textContent).toContain('当次读取有效');
  expect(screen.queryByRole('heading',{name:'上次读取草稿 · 历史草稿'})).toBeNull();
  const historical={...draft,current_validity:{state:'historical_only' as const,reason:'该原件只保留历史效力'}};
  await app.rerender({token:'selected',client,draft:historical});expect(screen.getByRole('heading',{name:'上次读取草稿 · 历史草稿'})).toBeTruthy();expect(screen.getByText(historical.current_validity.reason)).toBeTruthy();
  await app.rerender({token:'selected',client,draft:{...draft,draft:{...draft.draft,bundle:{...draft.draft.bundle,target_baseline:'towermd/B1'}}}});
  expect(screen.getByRole('heading',{name:'上次读取草稿 · 历史草稿'})).toBeTruthy();
  expect(screen.queryByRole('button',{name:/批准|创建|提交/})).toBeNull();noWrites(client);
});
