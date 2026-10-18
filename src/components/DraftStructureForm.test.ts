import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, expect, test, vi } from 'vitest';
import DraftStructureForm from './DraftStructureForm.svelte';
import DraftEditForm from './DraftEditForm.svelte';
import { savedFixture } from '../lib/draft.fixture';
afterEach(cleanup);

function fixture() {
  const saved = savedFixture();
  saved.object_views[0].fields = {paths: ['src/read/**', 'src/write/**'], verify: [{cwd: '.', argv: ['cargo', 'test']}]};
  saved.object_views.push({id: 'B', type: 'Module', title: '另一模块', path: 'docs/tower/arch/B.md', text: 'B原文', fields: {paths: ['other/**'], verify: []}},
    {id: 'AC', type: 'Acceptance', title: '验收', path: 'docs/tower/AC.md', text: '验收原文', fields: {module: 'A-module'}},
    {id: 'I1', type: 'Interface', title: '真实接口', path: 'docs/tower/arch/I1.md', text: '---\ntype: Interface\nid: I1\ntitle: 真实接口\nfuture: 保留\n---\n接口正文\n', fields: {from: 'A-module', to: 'B'}});
  return saved;
}
async function choose(name: string, value: string) {
  const select = screen.getByRole('combobox', {name}) as HTMLSelectElement;
  for (const option of select.options) option.selected = option.value === value || (option as HTMLOptionElement & {__value?: unknown}).__value === value;
  await fireEvent.change(select);
}
async function input(name: string, value: string) { await fireEvent.input(screen.getByRole('textbox', {name}), {target: {value}}); }
async function save() { await fireEvent.click(screen.getByRole('button', {name: '保存结构编辑到草稿'})); }

test('merge requires explicit modules and retained identity and submits only a typed edit', async () => {
  const saved = fixture(); const before = JSON.stringify(saved); const onedit = vi.fn(); render(DraftStructureForm, {saved, onedit});
  await input('合并后标题', '合并后'); await save(); expect(onedit).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole('checkbox', {name: /真模块/})); await fireEvent.click(screen.getByRole('checkbox', {name: /另一模块/}));
  await choose('保留模块 ID', 'B'); await save();
  expect(onedit).toHaveBeenLastCalledWith({action: 'merge', modules: ['A-module', 'B'], keep: 'B', title: '合并后'});
  expect(JSON.stringify(saved)).toBe(before);
});

test('split assigns exact source paths, acceptances, and both interface endpoints without guessing', async () => {
  const saved = fixture(); const before = JSON.stringify(saved); const onedit = vi.fn(); render(DraftStructureForm, {saved, onedit});
  await choose('结构操作', 'split'); await choose('源模块', 'A-module');
  await input('部分 1 ID', 'A-read'); await input('部分 1 标题', '读侧'); await input('部分 2 ID', 'A-write'); await input('部分 2 标题', '写侧');
  await save(); expect(onedit).not.toHaveBeenCalled();
  const assignment = screen.getByRole('combobox', {name: 'src/read/**'}) as HTMLSelectElement;
  const first = assignment.options[1].value; const second = assignment.options[2].value;
  await choose('src/read/**', first); await choose('src/write/**', second); await choose('验收 AC', first);
  await save(); expect(onedit).not.toHaveBeenCalled();
  await choose('接口 I1 起点', second); await save();
  expect(onedit).toHaveBeenLastCalledWith({action: 'split', module: 'A-module', parts: [
    {id: 'A-read', title: '读侧', paths: ['src/read/**'], acceptance_ids: ['AC'], verify: [], x: 0, y: 0},
    {id: 'A-write', title: '写侧', paths: ['src/write/**'], acceptance_ids: [], verify: [], x: 400, y: 0}
  ], interface_endpoints: {I1: ['A-write', 'B']}});
  expect(JSON.stringify(saved)).toBe(before);
});

test('verify edits preserve exact argv including spaces and empty arguments, with no execution', async () => {
  const onedit = vi.fn(); render(DraftStructureForm, {saved: fixture(), onedit});
  await choose('结构操作', 'confirm_verify'); await choose('源模块', 'A-module');
  expect((screen.getByRole('textbox', {name: '程序'}) as HTMLInputElement).value).toBe('cargo');
  await input('参数 1', 'argument with spaces'); await fireEvent.click(screen.getByRole('button', {name: '添加参数'}));
  await save(); expect(onedit).toHaveBeenLastCalledWith({action: 'confirm_verify', module: 'A-module', commands: [{cwd: '.', argv: ['cargo', 'argument with spaces', '']}]});
});

test.each([{verify: 'cargo test'}, {verify: ["'cargo' 'test' '--package' 'name with spaces'"]}])('legacy verification $verify requires explicit manual configuration, never shell splitting', async ({verify}) => {
  const saved = fixture(); saved.object_views[0].fields!.verify = verify; const before = JSON.stringify(saved); const onedit = vi.fn(); render(DraftStructureForm, {saved, onedit});
  await choose('结构操作', 'confirm_verify'); await choose('源模块', 'A-module');
  expect(screen.getByRole('alert').textContent).toContain('不能将它默认为空列表');
  expect((screen.getByRole('button', {name: '保存结构编辑到草稿'}) as HTMLButtonElement).disabled).toBe(true);
  await save(); expect(onedit).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole('button', {name: '手动配置替换验证命令'}));
  expect(screen.queryByRole('textbox', {name: '程序'})).toBeNull();
  await fireEvent.click(screen.getByRole('button', {name: '添加验证命令'})); await input('程序', 'cargo');
  await fireEvent.click(screen.getByRole('button', {name: '添加参数'})); await input('参数 1', 'exact argument with spaces'); await save();
  expect(onedit).toHaveBeenLastCalledWith({action: 'confirm_verify', module: 'A-module', commands: [{cwd: '.', argv: ['cargo', 'exact argument with spaces']}]});
  expect(JSON.stringify(saved)).toBe(before);
});

test('an absent optional verify field can be explicitly configured without changing raw documents', async () => {
  const saved = fixture(); delete saved.object_views[0].fields!.verify; const before = JSON.stringify(saved); const onedit = vi.fn(); render(DraftStructureForm, {saved, onedit});
  await choose('结构操作', 'confirm_verify'); await choose('源模块', 'A-module');
  await fireEvent.click(screen.getByRole('button', {name: '添加验证命令'})); await input('程序', 'cargo'); await save();
  expect(onedit).toHaveBeenLastCalledWith({action: 'confirm_verify', module: 'A-module', commands: [{cwd: '.', argv: ['cargo']}]});
  expect(JSON.stringify(saved)).toBe(before);
});

test('interface editing keeps full original Markdown and uses explicit module endpoints', async () => {
  const saved = fixture(); const before = JSON.stringify(saved); const onedit = vi.fn(); render(DraftStructureForm, {saved, onedit});
  await choose('结构操作', 'put_interface'); await choose('已有接口', 'I1'); await choose('接口起点', 'B'); await choose('接口终点', 'A-module'); await save();
  expect(onedit).toHaveBeenLastCalledWith({action: 'put_interface', object: {id: 'I1', path: 'docs/tower/arch/I1.md', text: saved.object_views[3].text}, from: 'B', to: 'A-module'});
  await choose('结构操作', 'remove_interface'); await choose('删除的接口', 'I1'); await save();
  expect(onedit).toHaveBeenLastCalledWith({action: 'remove_interface', interface: 'I1'}); expect(JSON.stringify(saved)).toBe(before);
});

test('new interface template quotes its ID and does not overwrite an existing interface', async () => {
  const onedit = vi.fn(); render(DraftStructureForm, {saved: fixture(), onedit});
  await choose('结构操作', 'put_interface'); await input('接口 ID', 'I: quoted'); await fireEvent.click(screen.getByRole('button', {name: '填入新接口模板'}));
  expect((screen.getByRole('textbox', {name: '接口原文'}) as HTMLTextAreaElement).value).toContain('id: "I: quoted"');
  await choose('已有接口', 'I1'); expect((screen.getByRole('button', {name: '填入新接口模板'}) as HTMLButtonElement).disabled).toBe(true);
});

test('a refreshed saved revision clears stale structure input and busy disables all mutations', async () => {
  const saved = fixture(); const onedit = vi.fn(); const view = render(DraftEditForm, {saved, onedit});
  await fireEvent.click(screen.getByText('模块、接口与验证命令'));
  await input('合并后标题', '旧内容');
  const newer = {...saved, revision: 'f'.repeat(40)}; await view.rerender({saved: newer, onedit, busy: true});
  expect((screen.getByRole('textbox', {name: '合并后标题'}) as HTMLInputElement).value).toBe('');
  expect((screen.getByRole('combobox', {name: '结构操作'}) as HTMLSelectElement).disabled).toBe(true);
  await save(); expect(onedit).not.toHaveBeenCalled();
});
