import { describe, expect, it } from 'vitest'
import { attentionCard, cardText, compareAttention, muteUntil, passiveCards, visitDigest, when, type AttentionCard } from './attention'
import type { RuntimeAttention, RuntimeTask } from './runtime'
import { delegated, delegationView } from '../test/delegation'

const item = (id: string, kind: RuntimeAttention['kind'], extra: Partial<RuntimeAttention> = {}): RuntimeAttention =>
  ({ id, kind, title: id, priority: 5, at: 0, modules: [], tasks: [], action: '处理', ...extra })
const length = (text: string) => [...text].length

describe('attention cards (FR-129)', () => {
  it('keeps six segments in a fixed order within 100 characters', () => {
    const card = attentionCard(item('q', '提问', { title: '是否沿用旧接口', tasks: ['T-1'], at: Date.parse('2026-10-23T08:05:00'), action: '打开问题回答。' }))
    expect(card).toEqual({ when: '10-23 08:05', object: '任务 T-1', what: '是否沿用旧接口', why: '受托者等你回答才能继续', suggest: '打开问题回答。', ask: '回答' })
    expect(cardText(card)).toBe('10-23 08:05 · 任务 T-1：是否沿用旧接口。因为受托者等你回答才能继续。建议打开问题回答。需要你：回答。')
    expect(attentionCard(item('x', '告警', { object: 'ops/disk', detail: 'ci 自报' }))).toMatchObject({ when: '时间未知', object: 'ops/disk', why: 'ci 自报' })
    expect(attentionCard(item('m', '待批提案', { modules: ['A', 'B'] })).object).toBe('模块 A、B')
    expect(attentionCard(item('p', '告警')).object).toBe('项目')
    const weakened = attentionCard(item('w', '承诺弱化', { title: '承诺弱化：1 处', object: '提案 P-1', detail: '门禁 crap（eng.crap）由硬改软', action: '打开提案核对' }))
    expect(cardText(weakened)).toBe('时间未知 · 提案 P-1：承诺弱化：1 处。因为门禁 crap（eng.crap）由硬改软。建议打开提案核对。需要你：核对后驳回、要求恢复或接受。')
    expect(attentionCard(item('w', '承诺弱化')).why).toBe('完成标准、检查或门禁被放宽')
  })

  it('shortens the longest segments first and never the time', () => {
    const card: AttentionCard = { when: '10-23 08:05', object: 'T-1', what: '长'.repeat(80), why: '原因'.repeat(30), suggest: '建议', ask: '回答' }
    const text = cardText(card)
    expect(length(text)).toBeLessThanOrEqual(100)
    expect(text.startsWith('10-23 08:05 · T-1：')).toBe(true)
    expect(text).toContain('建议建议。需要你：回答。')
    expect(text).toMatch(/长…。因为(原因)+原?…。/)
    for (const limit of [60, 80, 100]) expect(length(cardText(card, limit))).toBeLessThanOrEqual(limit)
  })
})

describe('attention order (FR-128)', () => {
  it('puts protection and time-sensitive first, then the longest wait, then the widest reach', () => {
    const items = [
      item('warn-wide', '告警', { modules: ['A', 'B', 'C'], at: 5 }),
      item('warn-narrow', '告警', { modules: ['A'], at: 1 }),
      item('question-new', '提问', { at: 300, modules: ['A', 'B'] }),
      item('proposal-old', '待批提案', { at: 100 }),
      item('question-unknown', '提问'),
      item('urgent', '告警', { urgent: true }),
      item('protected', '禁止', { at: 9 }),
      item('weakened', '承诺弱化', { at: 20 }),
    ]
    expect([...items].sort(compareAttention).map((entry) => entry.id)).toEqual([
      'protected', 'weakened', 'urgent', 'proposal-old', 'question-new', 'question-unknown', 'warn-wide', 'warn-narrow',
    ])
  })
})

describe('passive signals in the digest (FR-130)', () => {
  const now = Date.parse('2026-10-23T12:00:00Z')
  const signal = (from: string, kind: string, target: string, at = '2026-10-23T10:00:00Z', level = 'passive') => ({
    from: { id: from, kind: 'source' }, target, count: 1, data: { kind, level, title: `${kind} 提醒` },
    last: { at, eid: `${from}-${kind}`, seq: 1, actor: 'process:towermd-cli' },
  })

  it('merges three or more senders on one target into one card and skips old, quiet and louder ones', () => {
    const group = { signals: [
      signal('ci', 'eng.lint', 'T-1'), signal('bot', 'eng.size', 'T-1'), signal('scan', 'eng.deps', 'T-1'),
      signal('ci', 'eng.lint', 'T-2'), signal('ci', 'eng.old', 'T-2', '2026-10-01T00:00:00Z'),
      signal('ci', 'eng.loud', 'T-2', undefined, 'active'),
      { ...signal('ci', 'eng.seen', 'T-2'), acknowledged: { at: '2026-10-23T11:00:00Z', eid: 'a', actor: 'human:local' } },
    ] }
    expect(passiveCards(group, Date.parse('2026-10-22T00:00:00Z'), now)).toEqual([
      { target: 'T-1', text: 'T-1：3 个来源的 3 条提醒（eng.lint 提醒、eng.size 提醒 等）' },
      { target: 'T-2', text: 'T-2：eng.lint 提醒（ci 自报）' },
    ])
  })

  it('mutes for seven days', () => {
    expect(muteUntil(now)).toBe('2026-10-30T12:00:00.000Z')
  })
})

describe('since your last visit (FR-131)', () => {
  const now = Date.parse('2026-10-23T12:00:00Z')
  const task = (id: string, state: RuntimeTask['state'], needs_review = false): RuntimeTask => ({
    input: { id, title: `任务 ${id}`, module: 'A', touches: [], allowed_paths: [], depends_on: [], acceptance: [] },
    state, baseline: 'B0', needs_review, active_run: null,
  })
  const finished = (id: string, at: string) => {
    const object = delegated({ id, title: `完成 ${id}` })
    object.attempts = object.attempts.map((attempt) => ({ ...attempt, done: { at, eid: `D-${id}`, seq: 3 } }))
    return object
  }
  const attention = [1, 2, 3, 4, 5].map((n) => item(`a${n}`, '告警'))

  it('names things after a short absence and counts them after a long one', () => {
    const delegation = delegationView({ objects: [finished('T-1', '2026-10-23T09:00:00Z'), finished('T-0', '2026-10-01T09:00:00Z')] })
    const short = visitDigest({ since: Date.parse('2026-10-23T08:00:00Z'), fallback: false, now, delegation, attention,
      tasks: [task('T-2', 'held'), task('T-3', 'running')], signals: undefined })
    expect(short.heading).toMatch(/^自上次查看以来（10-23 \d\d:00 起）$/)
    expect(short.lines).toEqual(['完成了：完成 T-1', '卡在：任务 T-2', '需要你：3 件，另有 2 条已延后'])
    expect(short.details).toEqual(['完成 · 完成 T-1', '卡住 · 任务 T-2'])
    const long = visitDigest({ since: Date.parse('2026-10-20T08:00:00Z'), fallback: false, now, delegation, attention: [],
      tasks: [task('T-2', 'held')], signals: { signals: [{ from: { id: 'ci', kind: 'source' }, target: 'T-9', count: 1,
        data: { kind: 'eng.lint', level: 'passive', title: '警告' }, last: { at: '2026-10-22T00:00:00Z', eid: 'e', seq: 1, actor: 'process:towermd-cli' } }] } })
    expect(long.lines).toEqual(['完成了：1 件', '卡在：1 件；另有 1 条提醒', '需要你：暂时没有'])
    expect(long.details).toContain('提醒 · T-9：警告（ci 自报）')
  })

  it('says it covers the last 7 days when no visit was recorded', () => {
    const digest = visitDigest({ since: now - 7 * 86_400_000, fallback: true, now, delegation: null, attention: [], tasks: [], signals: null })
    expect(digest).toEqual({ heading: '最近 7 天', lines: ['完成了：没有新完成的事', '卡在：没有卡住的事', '需要你：暂时没有'], details: [] })
  })

  it('formats times in local time and admits unknown ones', () => {
    expect(when(0)).toBe('时间未知')
    expect(when(new Date(2026, 0, 2, 3, 4).getTime())).toBe('01-02 03:04')
  })
})
