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
