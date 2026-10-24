import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import { afterEach, expect, it, vi } from 'vitest'
import ProjectsOverview from './ProjectsOverview.svelte'
import type { AttentionEntry, Overview, ProjectHealth } from '../lib/api'

afterEach(cleanup)

const health = (color: ProjectHealth['color'], symbol: string, label: string, reasons: string[] = []): ProjectHealth =>
  ({ color, symbol, label, reasons, done: 0, running: 0, blocked: 0, interrupts: {}, check: null, areas: {} })
const entry = (project: string, id: string, label: string, card: string) =>
  ({ id, label, title: card, card, project, cmd: `towermd --project ${project} show ${id}` }) as AttentionEntry & { project: string; cmd: string }

it('shows each project with shape, text and colour, and at most three escaped items across projects', async () => {
  const overview: Overview = {
    projects: [
      { id: 'alpha', root: '/w/alpha', present: true, status: 'ready', health: health('red', '▲', '需处理', ['1 件受阻']), needs: 4, last_activity: '2026-10-24T08:00:00Z' },
      { id: 'beta', root: '/w/beta', present: true, status: 'ready', health: health('grey', '○', '没有任务'), needs: 0, last_activity: null },
      { id: 'gone', root: '/w/gone', present: false, status: 'unknown', health: null, needs: null, last_activity: null },
    ],
    attention: {
      items: [entry('alpha', 'a', '禁止', '<script>受保护</script>'), entry('alpha', 'b', '提问', '要不要 CSV'), entry('beta', 'c', '告警', '构建坏了'), entry('beta', 'd', '告警', '第四条')],
      shown: 3, total: 5, later: 2, budget: 3,
    },
  }
  const onOpen = vi.fn()
  const result = render(ProjectsOverview, { overview, current: 'beta', onOpen })
  const alpha = screen.getByRole('button', { name: /alpha/ })
  expect(alpha.classList.contains('health-red')).toBe(true)
  expect(alpha.textContent).toContain('▲ 需处理')
  expect(alpha.textContent).toContain('待处理 4')
  expect(alpha.textContent).toContain('1 件受阻')
  expect(screen.getByRole('button', { name: /beta/ }).textContent).toContain('○ 没有任务')
  expect((screen.getByRole('button', { name: /beta/ }) as HTMLButtonElement).disabled).toBe(true)
  expect((screen.getByRole('button', { name: /gone/ }) as HTMLButtonElement).disabled).toBe(true)
  expect(screen.getByRole('button', { name: /gone/ }).textContent).toContain('目录不在')

  const items = result.container.querySelectorAll('.overview-attention li')
  expect(items).toHaveLength(3)
  expect(items[0].textContent).toContain('alpha')
  expect(items[0].textContent).toContain('⊘')
  expect(result.container.querySelector('script')).toBeNull()
  expect(screen.queryByText(/第四条/)).toBeNull()
  expect(screen.getByText('另有 2 件稍后')).toBeTruthy()

  await fireEvent.click(alpha)
  expect(onOpen).toHaveBeenCalledWith('alpha')
})

it('says so when no daemon serves the cross-project list', () => {
  render(ProjectsOverview, { overview: { projects: [], attention: null }, onOpen: () => {} })
  expect(screen.getByText('还没有打开过项目。')).toBeTruthy()
  expect(screen.getByText('守护进程未运行，暂无跨项目关注。')).toBeTruthy()
})
