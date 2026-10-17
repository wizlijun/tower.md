import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte';
import { afterEach, expect, test } from 'vitest';
import App from './App.svelte';

afterEach(cleanup);

test('直接展示明确标识的原型，选择节点更新详情且没有批准动作', async () => {
  render(App);
  expect(screen.getByText('原型 · 演示数据')).toBeTruthy();
  expect(screen.getByRole('application')).toBeTruthy();
  expect(screen.getAllByRole('button', { pressed: false })).toHaveLength(5);
  await fireEvent.click(screen.getByRole('button', { name: '构建可交互画布，进行中' }));
  const details = screen.getByRole('complementary', { name: '节点详情' });
  expect(within(details).getByRole('heading', { name: '构建可交互画布' })).toBeTruthy();
  expect(within(details).getByText('连接真实项目投影')).toBeTruthy();
  expect(screen.queryByRole('button', { name: /批准|完成任务/ })).toBeNull();
});

test('缩放按钮改变比例，真实项目入口可以展开和收起', async () => {
  render(App);
  const ratio = screen.getByLabelText('缩放比例');
  const before = ratio.textContent;
  await fireEvent.click(screen.getByRole('button', { name: '放大' }));
  expect(ratio.textContent).not.toBe(before);
  await fireEvent.click(screen.getByRole('button', { name: '缩小' }));
  expect(ratio.textContent).toBe(before);
  await fireEvent.click(screen.getByRole('button', { name: '真实项目入口' }));
  expect(screen.getByRole('button', { name: '打开项目' })).toBeTruthy();
  await fireEvent.click(screen.getByRole('button', { name: '返回画布' }));
  expect(screen.queryByRole('button', { name: '打开项目' })).toBeNull();
});

test('平移改变视口，选择节点保持节点坐标', async () => {
  const { container } = render(App);
  const canvas = screen.getByRole('application');
  const world = container.querySelector('.canvas-world') as HTMLElement;
  const node = screen.getByRole('button', { name: '构建可交互画布，进行中' });
  const position = node.getAttribute('style');
  await fireEvent.keyDown(canvas, { key: 'ArrowRight' });
  expect(world.style.transform).toContain('translate(-40px, 0px)');
  await fireEvent.wheel(canvas, { deltaY: 25, deltaX: 10 });
  expect(world.style.transform).toContain('translate(-50px, -25px)');
  await fireEvent.click(node);
  expect(node.getAttribute('style')).toBe(position);
  expect(world.style.transform).toContain('translate(-50px, -25px)');
  Object.defineProperty(canvas, 'setPointerCapture', { value: () => undefined });
  await fireEvent.pointerDown(canvas, { button: 0, pointerId: 1, clientX: 100, clientY: 100 });
  await fireEvent.pointerMove(canvas, { pointerId: 1, clientX: 140, clientY: 130 });
  expect(world.style.transform).toContain('translate(-10px, 5px)');
  await fireEvent.pointerUp(canvas, { pointerId: 1 });
  await fireEvent.pointerMove(canvas, { pointerId: 1, clientX: 200, clientY: 200 });
  expect(world.style.transform).toContain('translate(-10px, 5px)');
});
