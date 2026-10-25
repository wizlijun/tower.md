import { describe, expect, it } from 'vitest'
import type { RecordedAnswer } from './human'
import {
  archiveLabel, archiveSummary, controllerNote, crossReviewLabel, crossReviewsFrom, overrideNote, overridesFrom, type CrossReviewEntry,
} from './controller'

const answer = (actor: string, extra: Partial<RecordedAnswer> = {}): RecordedAnswer => ({
  input: { hold: 'H-1', text: 'JSON', remember_as: { kind: 'temporary' } }, event: 'e1', actor, run: 'R-1',
  answered_at: '2026-10-25T10:00:00Z', proposal: null, ...extra,
})
const review = (extra: Partial<CrossReviewEntry> = {}): CrossReviewEntry => ({
  proposal: 'P-1', proposal_hash: 'h1', agent: 'codex/0.9', harness: 'codex', model: null, verdict: 'concerns',
  summary: '少了迁移说明', findings: [{ severity: 'medium', text: '缺迁移' }], error: null, cost_usd: null, ...extra,
})

describe('controller answers', () => {
  it('names the controller as a process with its basis, and says nothing for a person', () => {
    expect(controllerNote(answer('human:local'))).toBeNull()
    const note = controllerNote(answer('process:towermd-controller', { basis: ['任务卡：验收标准', '模块决策 D-1'], notify: true }))
    expect(note).toContain('不是人')
    expect(note).toContain('任务卡：验收标准；模块决策 D-1')
    expect(note).toContain('答后通知')
    expect(controllerNote(answer('process:towermd-controller'))).toContain('未给出依据')
  })

  it('tells a person which finished attempts came after the answer they replaced', () => {
    expect(overrideNote(answer('human:local'), undefined)).toBeNull()
    const replaced = answer('human:local', { overrides: { event: 'e0', text: 'CSV', answered_at: '2026-10-25T09:00:00Z' } })
    const overrides = overridesFrom({ ControllerOverride: {
      'H-1': { hold: 'H-1', run: 'R-1', task: 'T-1', replaced: 'e0', replaced_at: 't', replaced_text: 'CSV', answer: 'e1', affected: ['e5', 'e7'] },
      broken: { hold: 'x' },
    } })
    expect(Object.keys(overrides)).toEqual(['H-1'])
    expect(overrideNote(replaced, overrides['H-1'])).toMatch(/原总控回答"CSV".*T-1 已结束 2 次尝试/)
    expect(overrideNote(replaced, undefined)).toContain('尚无结束的尝试')
  })
})

describe('cross reviews', () => {
  it('reads only well-formed reviews and never shows an unfinished one as agreement', () => {
    const reviews = crossReviewsFrom({ CrossReview: { 'P-1': review(), 'P-2': review({ proposal: 'P-2', verdict: 'maybe' as never }) } })
    expect(Object.keys(reviews)).toEqual(['P-1'])
    expect(crossReviewLabel(review({ verdict: 'unfinished', agent: null, error: '超时' }), 'h1')).toBe('未完成（不等于同意） · codex · 费用未知')
    expect(crossReviewLabel(review({ cost_usd: 0.1234 }), 'h1')).toBe('有顾虑 · codex/0.9 · $0.12')
    expect(crossReviewLabel(review(), 'h2')).toContain('评审的是旧内容')
  })
})

describe('raw output archives', () => {
  it('counts attempts kept on this machine only', () => {
    expect(archiveLabel(archiveSummary(null))).toBeNull()
    const projection = { Artifact: { 'R-1/1': { store: 'local' }, 'R-1/2': { store: 'arca' }, bad: {} } }
    expect(archiveSummary(projection)).toEqual({ attempts: 2, localOnly: 1 })
    expect(archiveLabel(archiveSummary(projection))).toBe('原始输出 · 1/2 次尝试仅本机（未进 Arca）')
    expect(archiveLabel({ attempts: 3, localOnly: 0 })).toBe('原始输出 · 3 次尝试已入 Arca')
  })
})
