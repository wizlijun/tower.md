import { describe, expect, it } from 'vitest'
import { attentionCard, attentionSettings, attentionItem, attentionList, cardText, inFocus, interruptionKey, muteUntil, newInterruptions, notification, passiveCards, timing, visitDigest, when, type AttentionCard } from './attention'
import type { AttentionEntry } from './api'
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

describe('the daemon’s attention list (FR-128)', () => {
  // The order itself is tested where it is decided: `towermd_core::attention_list`.
  const entry = (id: string, extra: Record<string, unknown> = {}): AttentionEntry => ({ id, label: '告警', title: id, at: 0, objects: [], areas: ['A'],
    object: null, detail: null, action: '处理', urgent: false, delegation: null, signal: null, class: 3, timing: 'next_visit', quiet: false, card: '', ...extra }) as AttentionEntry

  it('reads the list leniently and keeps only entries the App can show', () => {
    expect(attentionList(undefined)).toBeNull()
    expect(attentionList({ items: 'x', shown: 1, asleep: 0 })).toBeNull()
    const list = attentionList({ items: [entry('a'), entry('b', { label: '未知' }), null, entry('c', { objects: 'x' })], shown: 1, asleep: 0 })
    expect(list).toEqual({ items: [entry('a')], shown: 1, asleep: 0, budget: 3 })
  })

  it('turns an entry into an App item with the daemon’s timing', () => {
    expect(attentionItem(entry('u', { label: '禁止', class: 1, urgent: true, timing: 'now', object: 'ops/db', objects: ['T-1'] }))).toEqual({
      id: 'u', kind: '禁止', title: 'u', priority: 1, at: 0, modules: ['A'], tasks: ['T-1'], action: '处理', detail: undefined,
      object: 'ops/db', urgent: true, delegation: undefined, timing: 'now', quiet: false })
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

  it('labels Tower.md’s own passive judgement, such as a first silence, as its judgement rather than a claim', () => {
    const own = { ...signal('process:towermd', 'core.silent', 'T-3'), last: { at: '2026-10-23T10:00:00Z', eid: 's', seq: 2, actor: 'process:towermd' } }
    expect(passiveCards({ signals: [own] }, Date.parse('2026-10-22T00:00:00Z'), now)).toEqual([
      { target: 'T-3', text: 'T-3：core.silent 提醒（Tower.md 判断）' },
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

  it('counts the first screen against the person’s budget', () => {
    const digest = visitDigest({ since: now - 3_600_000, fallback: false, now, delegation: null, attention, tasks: [], signals: null, budget: 1 })
    expect(digest.lines[2]).toBe('需要你：1 件，另有 4 条已延后')
  })

  it('says it covers the last 7 days when no visit was recorded', () => {
    const digest = visitDigest({ since: now - 7 * 86_400_000, fallback: true, now, delegation: null, attention: [], tasks: [], signals: null })
    expect(digest).toEqual({ heading: '最近 7 天', lines: ['完成了：没有新完成的事', '卡在：没有卡住的事', '需要你：暂时没有'], details: [], changes: [] })
  })

  it('formats times in local time and admits unknown ones', () => {
    expect(when(0)).toBe('时间未知')
    expect(when(new Date(2026, 0, 2, 3, 4).getTime())).toBe('01-02 03:04')
  })
})

describe('budget and interruption timing (FR-128, FR-130)', () => {
  it('reads the budget and focus windows leniently and never grows the first screen', () => {
    expect(attentionSettings(undefined)).toEqual({ budget: 3, focus_time: [] })
    expect(attentionSettings({ budget: 9, focus_time: 'x' })).toEqual({ budget: 3, focus_time: [] })
    expect(attentionSettings({ budget: 0 }).budget).toBe(3)
    expect(attentionSettings({ budget: 1.5 }).budget).toBe(3)
    expect(attentionSettings({ budget: 2, focus_time: [{ start: 1320, end: 480 }, { start: 5, end: 5 }, { start: -1, end: 3 }, null, { start: 600, end: 1440 }] }))
      .toEqual({ budget: 2, focus_time: [{ start: 1320, end: 480 }] })
  })

  it('tells local focus time, including windows across midnight', () => {
    const settings = attentionSettings({ focus_time: [{ start: 1320, end: 480 }, { start: 810, end: 840 }] })
    const at = (hours: number, minutes = 0) => new Date(2026, 9, 23, hours, minutes).getTime()
    expect([at(23), at(2), at(7, 59), at(13, 30), at(13, 59)].map((time) => inFocus(settings, time))).toEqual([true, true, true, true, true])
    expect([at(8), at(12), at(14), at(21, 59)].map((time) => inFocus(settings, time))).toEqual([false, false, false, false])
    expect(inFocus(attentionSettings(null), at(2))).toBe(false)
  })

  it('interrupts now only for time-sensitive items, once per message', () => {
    const urgent = { ...item('signal:ci:sec:proj', '禁止'), urgent: true, timing: 'now' as const, signal: { from: 'ci', kind: 'sec', target: 'proj', seen: 'e1' } }
    const plain = item('q1', '提问')
    expect([timing(urgent), timing(plain)]).toEqual(['now', 'next_visit'])
    expect(newInterruptions([urgent, plain], new Set()).map((entry) => entry.id)).toEqual([urgent.id])
    expect(newInterruptions([urgent], new Set([interruptionKey(urgent)]))).toEqual([])
    const newer = { ...urgent, signal: { ...urgent.signal, seen: 'e2' } }
    expect(newInterruptions([newer], new Set([interruptionKey(urgent)]))).toEqual([newer])
    const { title, body } = notification(urgent)
    expect(title).toBe('立即 · 禁止')
    expect(body).toBe(cardText(attentionCard(urgent)))
  })
})
