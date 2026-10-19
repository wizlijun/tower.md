import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { afterEach, expect, test, vi } from 'vitest';
import InterventionBar from './InterventionBar.svelte';
import type { InterventionClient, InterventionView, InterventionAction } from '../lib/interventions';
import { viewFixture, originalFixture, confirmedFixture, requestId } from '../lib/interventions.fixture';
afterEach(cleanup);
async function choose(name: string, value: string) {
  const select = screen.getByRole('combobox', { name }) as HTMLSelectElement;
  for (const option of select.options) option.selected = option.value === value || (option as HTMLOptionElement & { __value?: unknown }).__value === value;
  // happy-dom 20.9.0 omits OPTION from :checked. Use actual DOM selectedness
  // only for this select during change; production Svelte stays unchanged.
  const querySelector = select.querySelector.bind(select);
  const checked = vi.spyOn(select, 'querySelector').mockImplementation(
    <E extends Element = Element>(selector: string): E | null => (
      selector === ':checked'
        ? Array.from(select.options).find(option => option.selected) ?? null
        : querySelector(selector)
    ) as E | null,
  );
  try { await fireEvent.change(select); }
  finally { checked.mockRestore(); }
}
const clientFixture = (): InterventionClient => { let lastAction:InterventionAction|undefined;return { load: vi.fn(async () => viewFixture()), pending: vi.fn(async () => []), prepare: vi.fn(async (_token, _ticket, action) => {lastAction=action;return originalFixture(action);}), submit: vi.fn(async () => confirmedFixture(lastAction)), requestResume: vi.fn(async () => {}) }; };
test('常驻栏默认全局且只读加载；明确发送模块提示才创建并提交一次原操作', async () => {
  const client = clientFixture();
  render(InterventionBar, { token: 'selected', client, modules: [{ id: 'M-1', title: '实际模块' }] });
  await waitFor(() => expect(screen.getByRole('button', { name: '明确发送此范围插话' })).toBeTruthy());
  await waitFor(() => expect((screen.getByLabelText('发送范围') as HTMLSelectElement).disabled).toBe(false));
  expect((screen.getByLabelText('发送范围') as HTMLSelectElement).value).toBe('project');
  expect(client.prepare).not.toHaveBeenCalled(); expect(client.submit).not.toHaveBeenCalled(); expect(client.requestResume).not.toHaveBeenCalled();
  await choose('发送范围', 'module');
  await choose('选择模块', 'M-1');
  await fireEvent.input(screen.getByLabelText('插话内容'), { target: { value: '  保留 API 兼容性  ' } });
  expect(client.prepare).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole('button', { name: '明确发送此范围插话' }));
  await waitFor(() => expect(client.submit).toHaveBeenCalledExactlyOnceWith('selected', requestId));
  expect(client.prepare).toHaveBeenCalledWith('selected', viewFixture().action_ticket, { action: 'interject', scope: { kind: 'module', module: 'M-1' }, text: '  保留 API 兼容性  ' });
});
test('未确定的原操作只重试原UUID，不重做prepare或创建第二份操作', async () => {
  const client = clientFixture(); vi.mocked(client.submit).mockResolvedValueOnce({ request_id: requestId, state: 'uncertain', response: null, error: { code: 'uncertain', message: '回包尚未确定' } });
  render(InterventionBar, { token: 'selected', client });
  await waitFor(() => expect((screen.getByLabelText('插话内容') as HTMLTextAreaElement).disabled).toBe(false));
  await fireEvent.input(screen.getByLabelText('插话内容'), { target: { value: '仅补充本次要求' } });
  await fireEvent.click(screen.getByRole('button', { name: '明确发送此范围插话' }));
  await screen.findByText('回包尚未确定');
  const retry = screen.getByRole('button', { name: '重试这个原操作' }) as HTMLButtonElement;
  await waitFor(() => expect(retry.disabled).toBe(false));
  expect(client.submit).toHaveBeenCalledTimes(1);
  await fireEvent.click(retry);
  await waitFor(() => expect(client.submit).toHaveBeenCalledTimes(2));
  expect(client.prepare).toHaveBeenCalledTimes(1);
  expect(client.submit).toHaveBeenNthCalledWith(1, 'selected', requestId); expect(client.submit).toHaveBeenNthCalledWith(2, 'selected', requestId);
});
test('恢复先建立process请求并显示精确范围，范围变化后不能确认旧请求', async () => {
  const client = clientFixture();
  let view: InterventionView = { ...viewFixture(), paused: true, pause_revision: 'pause-1' };
  vi.mocked(client.load).mockImplementation(async () => view);
  vi.mocked(client.requestResume).mockImplementation(async () => { view = { ...view, resume_request: { request: 'resume-1', pause_revision: 'pause-1', runs: ['R-1'] } }; });
  render(InterventionBar, { token: 'selected', client });
  await waitFor(() => expect((screen.getByRole('button', { name: '读取恢复确认范围' }) as HTMLButtonElement).disabled).toBe(false));
  await fireEvent.click(screen.getByRole('button', { name: '读取恢复确认范围' }));
  await screen.findByRole('region', { name: '原恢复范围确认' });
  expect(screen.getByText('resume-1')).toBeTruthy(); expect(screen.getByText('pause-1')).toBeTruthy(); expect(screen.getByText('R-1')).toBeTruthy();
  expect(client.requestResume).toHaveBeenCalledExactlyOnceWith('selected'); expect(client.prepare).not.toHaveBeenCalled(); expect(client.submit).not.toHaveBeenCalled();
  view = { ...view, active_runs: [...view.active_runs, { run: 'R-2', task: 'T-2', module: 'M-1' }] };
  await fireEvent.click(screen.getByRole('button', { name: '重新读取控制状态' }));
  await screen.findByText('暂停版本或运行范围已变化；须重新读取范围，旧确认已禁用。');
  expect((screen.getByRole('button', { name: '确认恢复以上精确范围' }) as HTMLButtonElement).disabled).toBe(true);
  expect(client.prepare).not.toHaveBeenCalled(); expect(client.submit).not.toHaveBeenCalled();
});
test('暂停首先显示具体影响，取消不写入，只有明确确认才提交pause_all', async () => {
  const client = clientFixture(); render(InterventionBar, { token: 'selected', client });
  await waitFor(() => expect((screen.getByRole('button', { name: '暂停全部…' }) as HTMLButtonElement).disabled).toBe(false));
  await fireEvent.click(screen.getByRole('button', { name: '暂停全部…' }));
  expect(client.prepare).not.toHaveBeenCalled(); expect(client.submit).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole('button', { name: '取消暂停' }));
  expect(client.prepare).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole('button', { name: '暂停全部…' }));
  await fireEvent.click(screen.getByRole('button', { name: '确认暂停新的任务领取' }));
  await waitFor(() => expect(client.submit).toHaveBeenCalledExactlyOnceWith('selected', requestId));
  expect(client.prepare).toHaveBeenCalledWith('selected', viewFixture().action_ticket, { action: 'pause_all' });
});
