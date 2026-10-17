import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, expect, test, vi } from 'vitest';
import ProjectPicker from './ProjectPicker.svelte';
import type { InitOutput, SelectedProject } from '../lib/project';
afterEach(cleanup);
const project = (token = 'one', hasConfig = false): SelectedProject => ({ token, displayPath: `/项目 空格/${token}`, hasConfig });
const output: InitOutput = { stdout: '<script>原文 warning / hint</script>', stderr: 'stderr 原文', exitCode: 0 };
const tick = () => new Promise((resolve) => setTimeout(resolve, 0));
test('取消选择不初始化也不撤销当前项目', async () => {
  const choose = vi.fn().mockResolvedValueOnce(project()).mockResolvedValueOnce(null);
  const initialize = vi.fn();
  render(ProjectPicker, { choose, initialize });
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await screen.findByText('/项目 空格/one');
  await fireEvent.click(screen.getByRole('button', { name: '切换项目' }));
  await tick();
  expect(screen.getByText('/项目 空格/one')).toBeTruthy();
  expect(initialize).not.toHaveBeenCalled();
});
test('必须非空目录且明确点击；初始化时不能切换，原文转义并提醒未提交', async () => {
  let finish!: (output: InitOutput) => void;
  const initialize = vi.fn(() => new Promise<InitOutput>((resolve) => { finish = resolve; }));
  const { container } = render(ProjectPicker, { choose: async () => project(), initialize });
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  const input = await screen.findByRole('textbox', { name: '文档目录' });
  expect((input as HTMLInputElement).value).toBe('docs/tower');
  expect(screen.getByText(/intents.*arch.*specs.*baselines.*state/)).toBeTruthy();
  expect(initialize).not.toHaveBeenCalled();
  await fireEvent.input(input, { target: { value: '   ' } });
  expect((screen.getByRole('button', { name: '初始化项目' }) as HTMLButtonElement).disabled).toBe(true);
  await fireEvent.input(input, { target: { value: '文档 空格 \'单引号\' "双引号" $(touch x)' } });
  await fireEvent.click(screen.getByRole('button', { name: '初始化项目' }));
  expect(initialize).toHaveBeenCalledWith('one', '文档 空格 \'单引号\' "双引号" $(touch x)');
  expect((screen.getByRole('button', { name: '切换项目' }) as HTMLButtonElement).disabled).toBe(true);
  finish(output);
  await screen.findByText(/文件未提交.*自行提交到 main/);
  expect(screen.getByText(output.stdout)).toBeTruthy();
  expect(screen.getByText(output.stderr)).toBeTruthy();
  expect(container.querySelector('script')).toBeNull();
  expect(screen.queryByRole('textbox')).toBeNull();
});
test('已有配置不问目录不自动初始化，不宣称权威就绪', async () => {
  const initialize = vi.fn();
  render(ProjectPicker, { choose: async () => project('existing', true), initialize });
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await screen.findByText(/检测到配置.*权威状态尚未接入/);
  expect(screen.queryByRole('textbox')).toBeNull();
  expect(initialize).not.toHaveBeenCalled();
});
test('本会话确认过的 canonical 项目目录不问第二遍，失败保留诊断', async () => {
  const choose = vi.fn().mockResolvedValueOnce(project()).mockResolvedValueOnce(project('two')).mockResolvedValueOnce(project());
  const initialize = vi.fn().mockRejectedValue({ code: 'cli_failed', message: '初始化失败', detail: '<工具诊断>' });
  render(ProjectPicker, { choose, initialize });
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await fireEvent.input(await screen.findByRole('textbox'), { target: { value: 'my docs' } });
  await fireEvent.click(screen.getByRole('button', { name: '初始化项目' }));
  await screen.findByText('<工具诊断>');
  await fireEvent.click(screen.getByRole('button', { name: '切换项目' }));
  await screen.findByText('/项目 空格/two');
  await fireEvent.click(screen.getByRole('button', { name: '切换项目' }));
  await screen.findByText('/项目 空格/one');
  expect(screen.queryByRole('textbox')).toBeNull();
  expect(screen.getByText('my docs')).toBeTruthy();
  expect(initialize).toHaveBeenCalledTimes(1);
});

test('选择等待期间禁止重复选择，延迟成功和后续取消都保留可用的新 token', async () => {
  let nativeCurrent = 'one';
  let deliverSecond!: (value: SelectedProject) => void;
  const choose = vi.fn().mockResolvedValueOnce(project()).mockImplementationOnce(() => {
    nativeCurrent = 'two';
    return new Promise<SelectedProject>((resolve) => { deliverSecond = resolve; });
  }).mockResolvedValueOnce(null);
  const initialize = vi.fn(async (token: string) => {
    if (token !== nativeCurrent) throw { code: 'project_missing', message: '项目令牌已失效' };
    return output;
  });
  render(ProjectPicker, { choose, initialize });
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await screen.findByText('/项目 空格/one');
  const switching = screen.getByRole('button', { name: '切换项目' }) as HTMLButtonElement;
  await fireEvent.click(switching);
  expect(switching.disabled).toBe(true);
  // Also exercise the handler guard, independently of the HTML disabled attribute.
  switching.disabled = false;
  await fireEvent.click(switching);
  expect(choose).toHaveBeenCalledTimes(2);
  deliverSecond(project('two'));
  await screen.findByText('/项目 空格/two');
  expect(switching.disabled).toBe(false);
  await fireEvent.click(switching);
  await tick();
  expect(choose).toHaveBeenCalledTimes(3);
  expect(screen.getByText('/项目 空格/two')).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: '初始化项目' }));
  expect(initialize).toHaveBeenCalledExactlyOnceWith('two', 'docs/tower');
  await screen.findByText(/文件未提交.*自行提交到 main/);
});

test('native 已切换但响应尚未返回时，按钮和 handler 都阻止初始化旧 token', async () => {
  let nativeCurrent = 'one';
  let deliverSecond!: (value: SelectedProject) => void;
  const choose = vi.fn().mockResolvedValueOnce(project()).mockImplementationOnce(() => {
    nativeCurrent = 'two';
    return new Promise<SelectedProject>((resolve) => { deliverSecond = resolve; });
  });
  const initialize = vi.fn(async (token: string) => {
    if (token !== nativeCurrent) throw { code: 'project_missing', message: '项目令牌已失效' };
    return output;
  });
  render(ProjectPicker, { choose, initialize });
  await fireEvent.click(screen.getByRole('button', { name: '打开项目' }));
  await screen.findByText('/项目 空格/one');
  await fireEvent.click(screen.getByRole('button', { name: '切换项目' }));
  const init = screen.getByRole('button', { name: '初始化项目' }) as HTMLButtonElement;
  expect(init.disabled).toBe(true);
  init.disabled = false;
  await fireEvent.click(init);
  expect(initialize).not.toHaveBeenCalled();
  deliverSecond(project('two'));
  await screen.findByText('/项目 空格/two');
  await fireEvent.click(screen.getByRole('button', { name: '初始化项目' }));
  expect(initialize).toHaveBeenCalledExactlyOnceWith('two', 'docs/tower');
});
