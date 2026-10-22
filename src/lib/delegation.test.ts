import { describe, expect, it } from 'vitest'
import { buildBrief, checkpointLine, formatCount, formatMeasure, formatRatio, parseCheckpoints, reviewTimer, rfc3339Local } from './delegation'

describe('delegation card helpers', () => {
  it('parses checkpoint due times as local wall time and round-trips them', () => {
    const { checkpoints, invalid } = parseCheckpoints('起草方案 @ 2026-10-25 18:00\n\n  联调  \n坏日期 @ 2026-02-30 10:00\n@ 2026-10-25 18:00')
    expect(invalid).toEqual(['坏日期 @ 2026-02-30 10:00', '@ 2026-10-25 18:00'])
    expect(checkpoints).toEqual([{ text: '起草方案', due: rfc3339Local(new Date(2026, 9, 25, 18, 0)) }, { text: '联调' }])
    expect(checkpoints[0].due).toMatch(/^2026-10-25T18:00:00[+-]\d{2}:\d{2}$/)
    expect(new Date(checkpoints[0].due!).getTime()).toBe(new Date(2026, 9, 25, 18, 0).getTime())
    expect(checkpointLine(checkpoints[0])).toBe('起草方案 @ 2026-10-25 18:00')
  })

  it('builds the card in fixed order and omits blank optional fields', () => {
    const empty = buildBrief({ why: ' ', done_when: 'a\n\na\nb', limits: '', checkpoints: '', self_check: '' })
    expect(empty.brief).toEqual({ done_when: ['a', 'b'] })
    const full = buildBrief({ why: '因为', done_when: 'a', limits: '不碰 API', checkpoints: '一', self_check: '跑测试' })
    expect(Object.keys(full.brief)).toEqual(['why', 'done_when', 'limits', 'checkpoints', 'self_check'])
  })

  it('shows unknown metrics as unknown, never zero', () => {
    expect(formatCount(null)).toBe('未接入')
    expect(formatCount(0)).toBe('0')
    expect(formatRatio({ count: 0, total: 0, rate: null })).toBe('—')
    expect(formatRatio({ count: 1, total: 4, rate: 0.25 })).toBe('25%（1/4）')
    expect(formatMeasure({ count: 0, median: null, total: 0 })).toBe('—')
    expect(formatMeasure({ count: 2, median: 90_000, total: 180_000 })).toBe('中位 1 分 · 合计 3 分（2 次）')
  })

  it('counts review time only while the App is visible', () => {
    let clock = 0, visible = true
    const target = new EventTarget()
    const timer = reviewTimer({ now: () => clock, visible: () => visible, target })
    clock = 1000
    visible = false; target.dispatchEvent(new Event('visibilitychange'))
    clock = 60_000
    expect(timer.elapsed()).toBe(1000)
    visible = true; target.dispatchEvent(new Event('visibilitychange'))
    clock = 62_500
    expect(timer.elapsed()).toBe(3500)
    timer.dispose()
    visible = false; target.dispatchEvent(new Event('visibilitychange'))
    clock = 70_000
    expect(timer.elapsed()).toBe(11_000)
  })
})
