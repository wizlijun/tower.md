import { cleanup, render, screen } from '@testing-library/svelte'
import { afterEach, expect, it, vi } from 'vitest'
import ProjectAttention from './ProjectAttention.svelte'
import type { RuntimeAttention } from '../lib/runtime'

afterEach(cleanup)

it('shows three escaped summaries with shapes and text and updates them when concerns resolve', async () => {
  const items: Pick<RuntimeAttention, 'id' | 'kind' | 'title'>[] = [
    { id: 'a', kind: '禁止', title: '受保护变更' },
    { id: 'b', kind: '提问', title: '<script>原始问题</script>' },
    { id: 'c', kind: '待批提案', title: '调整验收' },
    { id: 'd', kind: '告警', title: '第四条' },
  ]
  const result = render(ProjectAttention, { items })
  expect(screen.getByTitle('禁止 · 受保护变更').textContent).toContain('⊘')
  expect(screen.getByTitle('提问 · <script>原始问题</script>').textContent).toContain('?')
  expect(screen.getByTitle('待批提案 · 调整验收').textContent).toContain('◇')
  expect(screen.queryByText(/第四条/)).toBeNull()
  expect(result.container.querySelector('script')).toBeNull()
  expect(screen.queryByRole('button')).toBeNull()
  await result.rerender({ items: [items[3]] })
  expect(screen.queryByTitle('禁止 · 受保护变更')).toBeNull()
  expect(screen.getByTitle('告警 · 第四条').textContent).toContain('!')
  await result.rerender({ items: [] })
  expect(screen.getByText('选择查看项目态势')).toBeTruthy()
})

it('puts the review backlog line above the attention items on the card and in the list', async () => {
  const { default: AttentionList } = await import('./AttentionList.svelte')
  const item = { id: 's', kind: '抽查', title: '抽查：真实任务', priority: 4, at: 0, modules: ['A'], tasks: ['T-1'], action: '复核', delegation: 'T-1' } as RuntimeAttention
  const card = render(ProjectAttention, { items: [item], backlog: 4 })
  expect(card.container.textContent).toMatch(/≡ 4 件待验收.*◎/)
  expect(screen.queryByText('选择查看项目态势')).toBeNull()
  cleanup()
  const list = render(AttentionList, { items: [item], backlog: { count: 4, threshold: 3 }, onOpen: () => {} })
  expect(list.container.textContent).toMatch(/≡ 4 件待验收[\s\S]*关注清单[\s\S]*抽查 · 抽查：真实任务/)
  expect(screen.getByRole('button', { name: '抽查 · 抽查：真实任务' })).toBeTruthy()
})

it('says how many items are deferred instead of showing a fourth, on the card and in the list', async () => {
  const { default: AttentionList } = await import('./AttentionList.svelte')
  const items = [1, 2, 3, 4, 5].map((n) => ({ id: `q${n}`, kind: '提问', title: `问题 ${n}`, priority: 2, at: n,
    modules: ['A'], tasks: [], action: '回答这个问题', question: undefined } as RuntimeAttention))
  render(ProjectAttention, { items: items.slice(0, 3), later: 2 })
  expect(screen.getByText('另有 2 条已延后')).toBeTruthy()
  cleanup()
  const list = render(AttentionList, { items, quieted: 1, onOpen: () => {} })
  expect(list.container.querySelectorAll('section > ul > li')).toHaveLength(3)
  expect(screen.getByText('另有 2 条已延后')).toBeTruthy()
  expect(screen.getByText(/1 条提醒已标为知道了或静音/)).toBeTruthy()
  const card = list.container.querySelector('.attention-card')?.textContent ?? ''
  expect([...card].length).toBeLessThanOrEqual(100)
  expect(card).toMatch(/^\d\d-\d\d \d\d:\d\d · 模块 A：问题 1。因为.+。建议.+。需要你：回答。$/)
})

it('offers acknowledge and mute only on signals, and only when the App passes a handler', async () => {
  const { default: AttentionList } = await import('./AttentionList.svelte')
  const signal = { id: 'signal:ci:eng.crap:proj', kind: '告警', title: 'CRAP 偏高', priority: 5, at: 0, modules: [], tasks: [],
    object: 'proj', action: '核对', signal: { from: 'ci', kind: 'eng.crap', target: 'proj', seen: 'e1' } } as RuntimeAttention
  const plain = { id: 'warning', kind: '告警', title: '其他', priority: 5, at: 0, modules: [], tasks: [], action: '核对' } as RuntimeAttention
  const onDismiss = vi.fn()
  render(AttentionList, { items: [signal, plain], onOpen: () => {}, onDismiss })
  expect(screen.getAllByRole('button', { name: '知道了' })).toHaveLength(1)
  screen.getByRole('button', { name: '静音 7 天' }).click()
  screen.getByRole('button', { name: '知道了' }).click()
  expect(onDismiss.mock.calls.map(([item, mode]) => [item.id, mode])).toEqual([[signal.id, 'mute'], [signal.id, 'ack']])
  cleanup()
  render(AttentionList, { items: [signal], onOpen: () => {} })
  expect(screen.queryByRole('button', { name: '知道了' })).toBeNull()
})

it('labels each item with its timing, keeps the person’s budget and notes focus time', async () => {
  const { default: AttentionList } = await import('./AttentionList.svelte')
  const items = [1, 2, 3].map((n) => ({ id: `a${n}`, kind: '告警', title: `告警 ${n}`, priority: 5, at: n, modules: [], tasks: [], action: '核对',
    urgent: n === 1, ...(n === 1 ? { timing: 'now' as const } : {}) } as RuntimeAttention))
  const list = render(AttentionList, { items, budget: 1, focus: true, onOpen: () => {} })
  expect(list.container.querySelectorAll('section > ul > li')).toHaveLength(1)
  expect(screen.getByText('另有 2 条已延后')).toBeTruthy()
  expect(list.container.querySelector('section > ul > li .attention-timing')?.textContent).toContain('立即')
  expect([...list.container.querySelectorAll('details .attention-timing')].map((node) => node.textContent?.trim())).toEqual(['○ 下次查看', '○ 下次查看'])
  expect(screen.getByText(/专注时段：只有必须停下的事会立即通知/)).toBeTruthy()
})

it('approves, answers and confirms on the card only when the App passes a handler (FR-138)', async () => {
  const { default: AttentionList } = await import('./AttentionList.svelte')
  const { fireEvent } = await import('@testing-library/svelte')
  const base = { priority: 3, at: 0, modules: [], tasks: [], action: '处理' }
  const question = (id: string, options: string[], layer = 'L3') => ({ ...base, id, kind: '提问', title: id, question: {
    draft: { id, text: id, options, recommended: options.length ? 0 : null, context: [] }, binding: { run: 'R', task: 'T', module: 'A' },
    targets: [{ id: 'X', layer }] } }) as RuntimeAttention
  const items = [
    question('choose', ['沿用', '替换']), question('free', [], 'L1'),
    { ...base, id: 'P-1', kind: '待批提案', title: '调整', proposal: 'P-1', approve: { draft_id: 'D', project_id: 'p', source_revision: 'r' } },
    { ...base, id: 'P-2', kind: '待批提案', title: '弱化', proposal: 'P-2' },
    { ...base, id: 'spot-check:T-1:A1', kind: '抽查', title: '抽查', delegation: 'T-1', confirm: { object: 'T-1', attempt: 'A1', unproven: [2] } },
  ] as RuntimeAttention[]
  const onAnswer = vi.fn(), onApprove = vi.fn(), onConfirm = vi.fn()
  render(AttentionList, { items, budget: 3, onOpen: () => {}, onAnswer, onApprove, onConfirm })
  await fireEvent.click(screen.getByRole('button', { name: '回答：沿用（推荐）' }))
  expect(onAnswer.mock.calls.map(([item, text]) => [item.id, text])).toEqual([['choose', '沿用']])
  expect(screen.getByText(/涉及人定的内容，回答将转为待批准提案/)).toBeTruthy()
  const submit = screen.getByRole('button', { name: '回答' }) as HTMLButtonElement
  expect(submit.disabled).toBe(true)
  await fireEvent.input(screen.getByLabelText('回答内容'), { target: { value: '换成新接口' } })
  await fireEvent.click(submit)
  expect(onAnswer.mock.calls[1][1]).toBe('换成新接口')
  await fireEvent.click(screen.getByText('另有 2 条已延后'))
  expect(screen.getAllByRole('button', { name: '批准' })).toHaveLength(1)
  await fireEvent.click(screen.getByRole('button', { name: '批准' }))
  expect(onApprove.mock.calls.map(([item]) => item.id)).toEqual(['P-1'])
  expect(screen.getByText(/1 条标准未证实/)).toBeTruthy()
  await fireEvent.click(screen.getByRole('button', { name: '确认完成' }))
  expect(onConfirm.mock.calls[0][0].id).toBe('spot-check:T-1:A1')
  expect(typeof onConfirm.mock.calls[0][1]).toBe('number')
  cleanup()
  render(AttentionList, { items, onOpen: () => {} })
  expect(screen.queryByRole('button', { name: '批准' })).toBeNull()
  expect(screen.queryByRole('button', { name: /^回答/ })).toBeNull()
})

it('opens an extension’s change summary behind “为什么” on the card and escapes it (FR-138, FR-139)', async () => {
  const { default: AttentionList } = await import('./AttentionList.svelte')
  const item = { id: 'P-1', kind: '待批提案', title: '放宽验收', priority: 3, at: 0, modules: [], tasks: ['T-1'], action: '批准', proposal: 'P-1' } as RuntimeAttention
  const changes = [
    { from: 'pkg:eng-diff', target: 'T-1', at: '2026-10-23T10:00:00Z', lines: ['新增函数：3', '<b>删除测试</b>：1'], text: '' },
    { from: 'pkg:eng-diff', target: 'T-9', at: '2026-10-23T10:00:00Z', lines: ['无关：1'], text: '' },
  ]
  const list = render(AttentionList, { items: [item], changes, onOpen: () => {} })
  expect(screen.getByText('为什么')).toBeTruthy()
  expect(list.container.textContent).toContain('T-1 · pkg:eng-diff 提供')
  expect(screen.getByText('<b>删除测试</b>：1')).toBeTruthy()
  expect(list.container.querySelector('b')).toBeNull()
  expect(list.container.textContent).not.toContain('无关')
  cleanup()
  render(AttentionList, { items: [item], changes: [], onOpen: () => {} })
  expect(screen.queryByText('为什么')).toBeNull()
})
