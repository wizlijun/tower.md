// Attention cards, their order and the "since your last visit" digest (core
// spec §7: FR-128, FR-129, FR-131, FR-40–42). Every card text, the order of
// the list and the three digest lines are produced here and nowhere else.
import type { DelegationView } from './api'
import type { RuntimeAttention, RuntimeTask } from './runtime'

/** FR-129: a card body has at most this many characters. */
export const CARD_CHARS = 100
/** FR-40, FR-128: the first screen shows at most this many items. */
export const FIRST_SCREEN = 3
/** Away longer than this, the digest gives counts instead of names (FR-131). */
export const COARSE_AFTER_MS = 24 * 60 * 60 * 1000
const MUTE_DAYS = 7

/** FR-129, in order: when, on which object, what happened, why, suggestion, what you must do. */
export interface AttentionCard { when: string; object: string; what: string; why: string; suggest: string; ask: string }

const kindText: Record<RuntimeAttention['kind'], { why: string; ask: string }> = {
  '禁止': { why: '人定内容只能经人批准进入主干', ask: '认领、撤销或转提案' },
  '检查未通过': { why: '完成只认检查证据', ask: '看证据后决定重跑或插话' },
  '提问': { why: '受托者等你回答才能继续', ask: '回答' },
  '待批提案': { why: '改动涉及人定内容，须人批准', ask: '批准或驳回' },
  '告警': { why: '来源自报，不是证据', ask: '核对后决定是否插话' },
  '抽查': { why: '已完成的任务按比例抽查', ask: '复核后确认或退回' },
  '汇报过期': { why: '受托者连续错过检查点汇报', ask: '插话、要求汇报或暂停' },
  '承诺弱化': { why: '完成标准、检查或门禁被放宽', ask: '核对后驳回、要求恢复或接受' },
}

/** Shape per kind, so an item never relies on colour alone. */
export const attentionSymbols: Record<RuntimeAttention['kind'], string> = {
  '禁止': '⊘', '检查未通过': '✕', '提问': '?', '待批提案': '◇', '告警': '!', '抽查': '◎', '汇报过期': '◷', '承诺弱化': '⇣',
}

function chars(text: string): string[] {
  return [...text]
}
function pad(value: number): string {
  return String(value).padStart(2, '0')
}
/** Local month, day and time; unknown times say so instead of guessing. */
export function when(at: number): string {
  if (!at) return '时间未知'
  const date = new Date(at)
  return `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`
}
function trim(text: string): string {
  return text.trim().replace(/[。.；;，,\s]+$/u, '')
}

/** The six segments of one item; the explanation is the item's own detail when it has one. */
export function attentionCard(item: RuntimeAttention): AttentionCard {
  const object = item.object ?? (item.tasks.length > 1 ? `${item.tasks[0]} 等 ${item.tasks.length} 个任务`
    : item.tasks.length ? `任务 ${item.tasks[0]}` : item.modules.length ? `模块 ${item.modules.join('、')}` : '项目')
  return { when: when(item.at), object, what: item.title, why: item.detail ?? kindText[item.kind].why,
    suggest: item.action, ask: kindText[item.kind].ask }
}

/**
 * One line, fixed order, at most 100 characters. When too long, the longest
 * segments are shortened first (with "…"), so short ones such as the time,
 * the object and what you must do stay whole.
 */
export function cardText(card: AttentionCard, limit = CARD_CHARS): string {
  const parts = [card.object, card.what, card.why, card.suggest, card.ask].map((part) => chars(trim(part)))
  const render = (p: string[][]) => `${card.when} · ${p[0].join('')}：${p[1].join('')}。因为${p[2].join('')}。建议${p[3].join('')}。需要你：${p[4].join('')}。`
  const fixed = chars(render(parts.map(() => []))).length
  let budget = limit - fixed
  if (budget >= parts.reduce((sum, part) => sum + part.length, 0)) return render(parts)
  // Water-filling: the largest cap that fits; parts above it are cut.
  const sorted = parts.map((part) => part.length).sort((a, b) => a - b)
  let cap = 0
  for (let index = 0; index < sorted.length; index += 1) {
    const rest = sorted.length - index
    const level = Math.floor(budget / rest)
    if (sorted[index] > level) { cap = level; break }
    budget -= sorted[index]
    cap = sorted[index]
  }
  return render(parts.map((part) => part.length <= cap ? part : [...part.slice(0, Math.max(cap - 1, 0)), '…']))
}

/**
 * FR-128 classes: 1 time-sensitive or protection-related; 2 a person is
 * waited on; 3 everything else, by how many objects it touches; 4 due
 * follow-ups (none exist yet).
 */
export function attentionClass(item: RuntimeAttention): 1 | 2 | 3 | 4 {
  if (item.urgent || item.kind === '禁止' || item.kind === '承诺弱化') return 1
  if (item.kind === '提问' || item.kind === '待批提案') return 2
  return 3
}
function affected(item: RuntimeAttention): number {
  return new Set([...item.tasks, ...item.modules]).size
}
/** Oldest known time first; an unknown time never counts as the longest wait. */
function older(a: RuntimeAttention, b: RuntimeAttention): number {
  return (a.at || Infinity) - (b.at || Infinity)
}
/**
 * FR-128 total order (gap analysis, ambiguity 10): class first; within the
 * waiting class the longest wait leads, elsewhere the most affected objects
 * (FR-40); then the older item, then the ID.
 */
export function compareAttention(a: RuntimeAttention, b: RuntimeAttention): number {
  const ca = attentionClass(a), cb = attentionClass(b)
  if (ca !== cb) return ca - cb
  const byWait = older(a, b), byReach = affected(b) - affected(a)
  return (ca === 2 ? byWait || byReach : byReach || byWait) || a.id.localeCompare(b.id)
}

export type SignalLevel = 'log' | 'passive' | 'active' | 'time_sensitive'
const levels: SignalLevel[] = ['log', 'passive', 'active', 'time_sensitive']
export interface SignalEntry {
  from: { id: string; kind: string }
  target: string
  count: number
  data: { kind: string; level: SignalLevel; title: string; detail?: string; value?: unknown; threshold?: unknown }
  last: { at: string; eid: string; seq: number; actor: string }
  acknowledged?: { at: string; eid: string; actor: string } | null
  muted?: { until: string; level: SignalLevel } | null
}
/**
 * A person quieted this signal in the App: "知道了" until a newer message,
 * or a mute until it ends, broken through when the sender escalates.
 */
export function signalQuiet(entry: SignalEntry, now: number): boolean {
  if (entry.acknowledged) return true
  const muted = entry.muted
  return !!muted && Date.parse(muted.until) > now && levels.indexOf(entry.data.level) <= levels.indexOf(muted.level)
}
export function signalEntries(group: unknown): SignalEntry[] {
  const signals = group && typeof group === 'object' ? (group as { signals?: unknown }).signals : undefined
  return (Array.isArray(signals) ? signals : []).filter((value): value is SignalEntry => {
    const entry = value as Partial<SignalEntry> | null
    return !!entry && typeof entry.target === 'string' && typeof entry.from?.id === 'string' && typeof entry.data?.kind === 'string'
      && typeof entry.data?.title === 'string' && levels.includes(entry.data?.level as SignalLevel) && typeof entry.last?.eid === 'string'
  })
}
export function muteUntil(now: number): string {
  return new Date(now + MUTE_DAYS * 24 * 60 * 60 * 1000).toISOString()
}

/**
 * FR-119, FR-130: passive signals in the window, for the digest only. Three
 * or more senders on one target become one card.
 */
export function passiveCards(group: unknown, since: number, now: number): { target: string; text: string }[] {
  const recent = signalEntries(group).filter((entry) => entry.data.level === 'passive' && !signalQuiet(entry, now)
    && Date.parse(entry.last.at) >= since)
  const byTarget = new Map<string, SignalEntry[]>()
  for (const entry of recent) byTarget.set(entry.target, [...byTarget.get(entry.target) ?? [], entry])
  return [...byTarget].sort(([a], [b]) => a.localeCompare(b)).flatMap(([target, entries]) => {
    const senders = new Set(entries.map((entry) => entry.from.id))
    if (senders.size >= 3) {
      const titles = entries.slice(0, 2).map((entry) => entry.data.title).join('、')
      return [{ target, text: `${target}：${senders.size} 个来源的 ${entries.length} 条提醒（${titles}${entries.length > 2 ? ' 等' : ''}）` }]
    }
    return entries.map((entry) => ({ target, text: `${target}：${entry.data.title}（${entry.from.id} 自报）` }))
  })
}

export interface DigestInput {
  since: number
  /** No recorded visit: the window is the last 7 days. */
  fallback: boolean
  now: number
  tasks: RuntimeTask[]
  delegation: DelegationView | null
  signals: unknown
  attention: RuntimeAttention[]
}
function names(items: string[], coarse: boolean, unit: string): string {
  return coarse || items.length > 2 ? `${items.length} ${unit}` : items.join('、')
}
/**
 * FR-131: three lines — what finished, where things are stuck, what needs
 * you. The longer you were away, the coarser (counts instead of names).
 */
export function visitDigest(input: DigestInput): { heading: string; lines: [string, string, string]; details: string[] } {
  const coarse = input.now - input.since > COARSE_AFTER_MS
  const finished = (input.delegation?.objects ?? []).filter((object) => object.attempts.some((attempt) =>
    attempt.done && Date.parse(attempt.done.at) >= input.since)).map((object) => object.title)
  const stuck = input.tasks.filter((task) => task.needs_review || task.state === 'held' || task.state === 'paused')
    .map((task) => task.input.title)
  const blocked = (input.delegation?.report_requests ?? []).filter((request) => request.open && request.reply?.data.state === 'blocked')
    .map((request) => request.object)
  const stuckNames = [...new Set([...stuck, ...blocked])]
  const passive = passiveCards(input.signals, input.since, input.now)
  const later = Math.max(input.attention.length - FIRST_SCREEN, 0)
  const now = Math.min(input.attention.length, FIRST_SCREEN)
  return {
    heading: input.fallback ? '最近 7 天' : `自上次查看以来（${when(input.since)} 起）`,
    lines: [
      finished.length ? `完成了：${names(finished, coarse, '件')}` : '完成了：没有新完成的事',
      stuckNames.length || passive.length
        ? `卡在：${stuckNames.length ? names(stuckNames, coarse, '件') : '没有卡住的事'}${passive.length ? `；另有 ${passive.length} 条提醒` : ''}`
        : '卡在：没有卡住的事',
      now ? `需要你：${now} 件${later ? `，另有 ${later} 条已延后` : ''}` : '需要你：暂时没有',
    ],
    details: [...finished.map((title) => `完成 · ${title}`), ...stuckNames.map((title) => `卡住 · ${title}`), ...passive.map((card) => `提醒 · ${card.text}`)],
  }
}
