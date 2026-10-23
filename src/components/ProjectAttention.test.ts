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
