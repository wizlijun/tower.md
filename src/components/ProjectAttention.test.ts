import { cleanup, render, screen } from '@testing-library/svelte'
import { afterEach, expect, it } from 'vitest'
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
  expect(card.container.textContent).toMatch(/≡ 4 件等你验收.*◎/)
  expect(screen.queryByText('选择查看项目态势')).toBeNull()
  cleanup()
  const list = render(AttentionList, { items: [item], backlog: { count: 4, threshold: 3 }, onOpen: () => {} })
  expect(list.container.textContent).toMatch(/≡ 4 件等你验收[\s\S]*需要关注[\s\S]*抽查 · 抽查：真实任务/)
  expect(screen.getByRole('button', { name: '抽查 · 抽查：真实任务' })).toBeTruthy()
})
