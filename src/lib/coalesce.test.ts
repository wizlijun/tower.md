import { afterEach, describe, expect, it, vi } from 'vitest'
import { coalescer, MERGE_MS } from './coalesce'

afterEach(() => { vi.useRealTimers() })

describe('one visual change a second (FR-50)', () => {
  it('shows only the latest of several changes, once, a second after the first', () => {
    vi.useFakeTimers()
    const show = vi.fn()
    const merge = coalescer<number>(show)
    merge.push(1)
    vi.advanceTimersByTime(400)
    merge.push(2)
    merge.push(3)
    vi.advanceTimersByTime(MERGE_MS - 401)
    expect(show).not.toHaveBeenCalled()
    vi.advanceTimersByTime(1)
    expect(show.mock.calls).toEqual([[3]])
    merge.push(4)
    vi.advanceTimersByTime(MERGE_MS)
    expect(show.mock.calls).toEqual([[3], [4]])
  })

  it('shows at once when asked and drops what was waiting; stop drops it too', () => {
    vi.useFakeTimers()
    const show = vi.fn()
    const merge = coalescer<string>(show)
    merge.push('later')
    merge.push('now', true)
    expect(show.mock.calls).toEqual([['now']])
    vi.advanceTimersByTime(MERGE_MS * 2)
    expect(show.mock.calls).toEqual([['now']])
    merge.push('waiting')
    merge.stop()
    vi.advanceTimersByTime(MERGE_MS * 2)
    expect(show.mock.calls).toEqual([['now']])
  })
})
