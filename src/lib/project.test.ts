import { afterEach, expect, test, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { chooseProject, initializeProject } from './project';
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
afterEach(() => vi.resetAllMocks());
test('项目选择 invoke 不传路径或 CLI 参数，取消原样返回 null', async () => {
  vi.mocked(invoke).mockResolvedValue(null);
  expect(await chooseProject()).toBeNull();
  expect(invoke).toHaveBeenCalledExactlyOnceWith('choose_project');
});
test('初始化 invoke 只传 token 与单一原样文档目录', async () => {
  const result = { stdout: '原文', stderr: 'warning', exitCode: 0 };
  const directory = '中文 空格 \'引号\' "双引号" $(touch x)';
  vi.mocked(invoke).mockResolvedValue(result);
  expect(await initializeProject('native-token', directory)).toEqual(result);
  expect(invoke).toHaveBeenCalledExactlyOnceWith('initialize_project', { token: 'native-token', docsDir: directory });
});
