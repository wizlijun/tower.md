import { cleanup, render, screen } from '@testing-library/svelte';
import { afterEach, expect, test } from 'vitest';
import App from './App.svelte';

afterEach(cleanup);

test('启动时提供项目入口且没有假模块或批准动作', () => {
  render(App);
  const entry = screen.getByRole('button', { name: '打开项目' }) as HTMLButtonElement;
  expect(entry).toBeTruthy();
  expect(entry.disabled).toBe(false);
  expect(screen.getByRole('heading', { name: 'Tower.md' })).toBeTruthy();
  expect(screen.getAllByRole('button')).toHaveLength(1);
  expect(screen.queryByRole('button', { name: /批准|暂停/ })).toBeNull();
  expect(screen.queryByText(/示例模块|Module [A-Z]/)).toBeNull();
});
