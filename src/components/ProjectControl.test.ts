import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import ProjectControl, { type ProjectControlState } from './ProjectControl.svelte'

afterEach(cleanup)

const paused: ProjectControlState = {
  paused: true, pause_event: 'pause-original',
  resume_requests: [{ event: 'request-1', pause_event: 'pause-original', actor: 'process:towermd-cli', at: '2026-10-21T00:00:00Z' }],
}

describe('explicit project admission control', () => {
  it('sends pause only after a click and describes checkpoint cooperation honestly', async () => {
    const pause = vi.fn(), resume = vi.fn()
    render(ProjectControl, {
      control: { paused: false, pause_event: null, resume_requests: [] },
      onPause: pause, onResume: resume, onRetry: vi.fn(),
    })
    expect(screen.getByText(/这不表示进程已停止/)).toBeTruthy()
    expect(pause).not.toHaveBeenCalled()
    await fireEvent.click(screen.getByRole('button', { name: '暂停项目' }))
    expect(pause).toHaveBeenCalledTimes(1)
    expect(resume).not.toHaveBeenCalled()
  })

  it('does not treat CLI resume requests as approval and binds the explicit click to the pause', async () => {
    const resume = vi.fn()
    render(ProjectControl, { control: paused, onPause: vi.fn(), onResume: resume, onRetry: vi.fn() })
    expect(screen.getByText('有 1 条恢复请求，须由你确认后才能恢复。')).toBeTruthy()
    expect(resume).not.toHaveBeenCalled()
    await fireEvent.click(screen.getByRole('button', { name: '确认恢复项目' }))
    expect(resume).toHaveBeenCalledWith('pause-original')
  })

  it('refuses to resume when the pause record is missing', async () => {
    const resume = vi.fn()
    render(ProjectControl, {
      control: { ...paused, pause_event: null }, onPause: vi.fn(), onResume: resume, onRetry: vi.fn(),
    })
    expect((screen.getByRole('button', { name: '确认恢复项目' }) as HTMLButtonElement).disabled).toBe(true)
    await fireEvent.click(screen.getByRole('button', { name: '确认恢复项目' }))
    expect(resume).not.toHaveBeenCalled()
  })

  it('preserves the original uncertain action after new state arrives and only retries', async () => {
    const retry = vi.fn(), pause = vi.fn(), resume = vi.fn()
    const view = render(ProjectControl, {
      control: paused, frozen: { kind: 'resume', pause_event: 'pause-original' },
      error: '回执未知', onPause: pause, onResume: resume, onRetry: retry,
    })
    await view.rerender({ control: { ...paused, pause_event: 'pause-new', resume_requests: [] } })
    expect(screen.getByText('pause-original')).toBeTruthy()
    expect(screen.queryByText('pause-new')).toBeNull()
    expect(screen.queryByRole('button', { name: '确认恢复项目' })).toBeNull()
    await fireEvent.click(screen.getByRole('button', { name: '重试原操作' }))
    expect(retry).toHaveBeenCalledTimes(1)
    expect(pause).not.toHaveBeenCalled()
    expect(resume).not.toHaveBeenCalled()
    await view.rerender({ busy: true })
    await fireEvent.click(screen.getByRole('button', { name: '正在确认…' }))
    expect(retry).toHaveBeenCalledTimes(1)
  })
})
