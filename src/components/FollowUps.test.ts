import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import FollowUps from './FollowUps.svelte'
import type { FollowUpEntry } from '../lib/api'
import { selectValue } from '../test/select'

afterEach(cleanup)

const mark = (seq: number) => ({ at: '2026-10-23T01:00:00Z', eid: `e${seq}`, seq, actor: 'human:local' })
const entry = (id: string, done = false): FollowUpEntry =>
  ({ id, text: `跟进 ${id}`, when: { on: 'done', target: 'T-1' }, added: mark(1), ...(done ? { done: mark(2) } : {}) })
const targets = [{ id: 'proj', title: '项目' }, { id: 'T-1', title: '登录' }]

describe('follow-ups panel', () => {
  it('sends the chosen condition on the chosen object and completes an open one', async () => {
    const onAdd = vi.fn().mockResolvedValue(true), onFinish = vi.fn()
    render(FollowUps, { followUps: [entry('a'), entry('b', true)], targets, onAdd, onFinish, onRetry: vi.fn() })
    expect(screen.getByText('跟进项（1/5）')).toBeTruthy()
    expect(screen.getByText(/登录 完成时/)).toBeTruthy()
    await fireEvent.click(screen.getByRole('button', { name: '完成' }))
    expect(onFinish).toHaveBeenCalledWith(entry('a'))
    const add = screen.getByRole('button', { name: '添加跟进' }) as HTMLButtonElement
    await fireEvent.input(screen.getByRole('textbox', { name: '跟进什么' }), { target: { value: '登录做完再看' } })
    expect(add.disabled).toBe(true)
    await selectValue(screen.getByRole('combobox', { name: '到点条件' }), 'done')
    await selectValue(screen.getByRole('combobox', { name: '对象' }), 'T-1')
    await fireEvent.click(add)
    expect(onAdd).toHaveBeenCalledWith('登录做完再看', { on: 'done', target: 'T-1' })
  })

  it('refuses a sixth open follow-up and offers only a retry while a send is unconfirmed', async () => {
    render(FollowUps, { followUps: ['a', 'b', 'c', 'd', 'e'].map((id) => entry(id)), targets, onAdd: vi.fn(), onFinish: vi.fn(), onRetry: vi.fn() })
    expect((screen.getByRole('button', { name: '添加跟进' }) as HTMLButtonElement).disabled).toBe(true)
    expect(screen.getByText(/最多 5 条未完成的跟进项/)).toBeTruthy()
    cleanup()
    const onRetry = vi.fn()
    render(FollowUps, { followUps: [], targets, frozen: true, error: '回执未知', onAdd: vi.fn(), onFinish: vi.fn(), onRetry })
    expect(screen.queryByRole('button', { name: '添加跟进' })).toBeNull()
    await fireEvent.click(screen.getByRole('button', { name: '重试原提交' }))
    expect(onRetry).toHaveBeenCalled()
    expect(screen.getByRole('alert').textContent).toBe('回执未知')
  })
})
