// Follow-ups, quiet objects and resurfacing (core spec FR-134–136). The
// daemon derives which follow-ups are due and which objects are quiet or
// resurfaced; this module only turns that into attention items, labels and
// the "上次到哪了" recap. Nothing here writes.
import type { DelegationView, FollowUpEntry, FollowUpWhen, Mark, Resurfaced, Rhythm } from './api'
import type { QuestionCard, RecordedAnswer } from './human'
import type { RuntimeAttention, RuntimeTask } from './runtime'

/** The same limits the daemon checks (`follow_ups::TEXT_CHARS`, `MAX_OPEN`). */
export const FOLLOW_UP_TEXT_CHARS = 200
export const MAX_OPEN_FOLLOW_UPS = 5
/** Each recap list stays short: it is a reminder, not the history. */
const RECAP_ITEMS = 3

function record(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {}
}
function time(value: unknown): number {
  const parsed = typeof value === 'string' ? Date.parse(value) : NaN
  return Number.isFinite(parsed) ? parsed : 0
}
function mark(value: unknown): value is Mark {
  const entry = record(value)
  return typeof entry.at === 'string' && typeof entry.eid === 'string' && Number.isSafeInteger(entry.seq) && typeof entry.actor === 'string'
}
function validWhen(value: unknown): value is FollowUpWhen {
  const when = record(value)
  if (when.on === 'date') return typeof when.at === 'string'
  return ['done', 'activity', 'signal'].includes(String(when.on)) && typeof when.target === 'string'
}

/** Follow-ups from the projection: open ones first, newest first within each half. */
export function followUpsFrom(projection: unknown): FollowUpEntry[] {
  return Object.values(record(record(projection).FollowUp)).filter((value): value is FollowUpEntry => {
    const entry = record(value)
    return typeof entry.id === 'string' && typeof entry.text === 'string' && validWhen(entry.when) && mark(entry.added)
  }).sort((a, b) => Number(!!a.done) - Number(!!b.done) || b.added.seq - a.added.seq)
}

/** The daemon's derived rhythm, read leniently: an older daemon has none. */
export function rhythmFrom(value: unknown): Rhythm | null {
  const raw = record(value)
  if (!Number.isInteger(raw.dormant_after) || !Array.isArray(raw.objects)) return null
  const objects = raw.objects.filter((value): value is Rhythm['objects'][number] => {
    const entry = record(value)
    return typeof entry.id === 'string' && typeof entry.quiet === 'boolean' && (entry.last === null || mark(entry.last))
  })
  return { dormant_after: raw.dormant_after as number, objects, due: (Array.isArray(raw.due) ? raw.due : []).filter((id): id is string => typeof id === 'string') }
}

/** A rejection the person can fix before sending; `null` when the sentence is fine. */
export function followUpTextError(text: string): string | null {
  const value = text.trim()
  if (!value) return '先写一句话。'
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001f\u007f-\u009f]/.test(value)) return '一句话里不能有换行或控制字符。'
  if ([...value].length > FOLLOW_UP_TEXT_CHARS) return `一句话最多 ${FOLLOW_UP_TEXT_CHARS} 个字。`
  return null
}

function pad(value: number): string {
  return String(value).padStart(2, '0')
}
/** The condition in words; a target that is the project reads as "项目". */
export function whenLabel(when: FollowUpWhen, title: (id: string) => string = (id) => id): string {
  if (when.on === 'date') {
    const at = time(when.at)
    if (!at) return `到 ${when.at}`
    const date = new Date(at)
    return `到 ${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`
  }
  const target = title(when.target)
  if (when.on === 'done') return `${target} 完成时`
  if (when.on === 'activity') return `${target} 有新动静时`
  return `${target} 出现${when.kind ? ` ${when.kind} ` : ''}信号时`
}

/** Why a quiet object came back, in words. */
export function resurfacedLabel(reason: Resurfaced): string {
  if (reason.by === 'follow_up') return '跟进项到点'
  if (reason.by === 'check_every') return `到了 ${reason.days} 天的复查节奏`
  return `沉寂后有新动静（${reason.event.actor} 自报）`
}

/** "上次到哪了" (FR-136): last conclusion, what was overturned, what is still open. */
export interface Recap { conclusion: string | null; overturned: string[]; open: string[] }
export interface RecapSources {
  delegation: DelegationView | null
  /** The `TowerApi` group: merged reports a sender made about a target. */
  towerApi: unknown
  questions: QuestionCard[]
  answers: Record<string, RecordedAnswer>
  /** The `Proposal` group. */
  proposals: unknown
  followUps: FollowUpEntry[]
}
/**
 * Built from what is already recorded: the latest confirmed verdict or
 * report on the object is the conclusion; send-backs and rejected proposals
 * touching it are what was overturned; unanswered questions, open requests
 * and open follow-ups on it are what is still open.
 */
export function recap(object: string, sources: RecapSources): Recap {
  const conclusions: { at: number; text: string }[] = []
  const overturned: { at: number; text: string }[] = []
  const found = sources.delegation?.objects.find((entry) => entry.id === object)
  for (const attempt of found?.attempts ?? []) {
    const verdict = attempt.verdict
    if (!verdict) continue
    if (verdict.kind === 'confirmed') conclusions.push({ at: time(verdict.event.at), text: verdict.takeaway?.text ? `已确认完成：${verdict.takeaway.text}` : '已确认完成' })
    else overturned.push({ at: time(verdict.event.at), text: `退回${verdict.reason ? `：${verdict.reason}` : ''}` })
  }
  const reports = record(sources.towerApi).reports
  for (const value of Array.isArray(reports) ? reports : []) {
    const entry = record(value), data = record(entry.data), from = record(entry.from), last = record(entry.last)
    if (entry.target !== object || typeof data.summary !== 'string') continue
    conclusions.push({ at: time(last.at), text: `${typeof from.id === 'string' ? from.id : '来源'} 汇报：${data.summary}` })
  }
  for (const request of sources.delegation?.requests ?? []) {
    if (request.data.target === object && request.reply) conclusions.push({ at: time(request.reply.event.at), text: `${request.reply.data.from} 回复：${request.reply.data.summary}` })
  }
  for (const request of sources.delegation?.report_requests ?? []) {
    if (request.object === object && request.reply) conclusions.push({ at: time(request.reply.event.at), text: `汇报：${request.reply.data.summary}` })
  }
  for (const [id, value] of Object.entries(record(sources.proposals))) {
    const entry = record(value), proposal = record(entry.proposal), impact = record(proposal.impact)
    const tasks = Array.isArray(impact.tasks) ? impact.tasks : []
    if (entry.status === 'rejected' && tasks.includes(object)) overturned.push({ at: time(entry.at), text: `驳回提案 ${id}${typeof proposal.reason === 'string' ? `：${proposal.reason}` : ''}` })
  }
  const latest = (items: { at: number; text: string }[]) => [...items].sort((a, b) => b.at - a.at).map((item) => item.text)
  const open = [
    ...sources.questions.filter((question) => question.binding.task === object && !sources.answers[question.draft.id]).map((question) => `问题：${question.draft.text}`),
    ...(sources.delegation?.requests ?? []).filter((request) => request.open && request.data.target === object)
      .map((request) => `${request.data.ask === 'question' ? '提问' : '汇报请求'}：${request.data.text}`),
    ...(sources.delegation?.report_requests ?? []).filter((request) => request.open && request.object === object).map((request) => `汇报请求：${request.text}`),
    ...sources.followUps.filter((entry) => !entry.done && 'target' in entry.when && entry.when.target === object).map((entry) => `跟进：${entry.text}`),
  ]
  return { conclusion: latest(conclusions)[0] ?? null, overturned: latest(overturned).slice(0, RECAP_ITEMS), open: open.slice(0, RECAP_ITEMS) }
}

export interface RhythmInput {
  rhythm: Rhythm | null
  followUps: FollowUpEntry[]
  tasks: RuntimeTask[]
  delegation: DelegationView | null
  recap: (object: string) => Recap
}
/**
 * FR-134, FR-136: due follow-ups and resurfaced objects become class-4
 * attention items. A follow-up that resurfaced its quiet target carries that
 * target's recap; it is settled by completing the follow-up. An object that
 * came back for its rhythm or a connector is settled by "看过了".
 */
export function rhythmAttention(input: RhythmInput): RuntimeAttention[] {
  const rhythm = input.rhythm
  if (!rhythm) return []
  const project = rhythm.objects[0]?.id
  const title = (id: string) => id === project ? '项目' : input.delegation?.objects.find((entry) => entry.id === id)?.title ?? id
  const place = (target: string | undefined) => {
    const tasks = input.tasks.filter((task) => task.input.id === target)
    const known = input.delegation?.objects.some((entry) => entry.id === target) ?? false
    return { tasks: tasks.map((task) => task.input.id), modules: tasks.map((task) => task.input.module),
      delegation: known ? target : undefined, object: !target || target === project || tasks.length ? undefined : target }
  }
  const items: RuntimeAttention[] = []
  for (const id of rhythm.due) {
    const entry = input.followUps.find((item) => item.id === id && !item.done)
    if (!entry) continue
    const target = 'target' in entry.when ? entry.when.target : undefined
    const back = rhythm.objects.find((object) => object.id === target && object.resurfaced?.by === 'follow_up' && object.resurfaced.follow_up === id)
    items.push({ id: `follow-up:${id}`, kind: '跟进', priority: 6, at: time(entry.added.at), title: entry.text, ...place(target),
      detail: `你设的跟进：${whenLabel(entry.when, title)}${back ? `；${title(back.id)} 已休眠，现在浮现` : ''}`,
      action: '回头看一眼；看完点「完成跟进」。', followUp: id,
      recap: back ? input.recap(back.id) : undefined })
  }
  for (const object of rhythm.objects) {
    const reason = object.resurfaced
    if (!reason || reason.by === 'follow_up') continue
    items.push({ id: `resurfaced:${object.id}`, kind: '浮现', priority: 6,
      at: reason.by === 'connector' ? time(reason.event.at) : time(object.last?.at), title: `${title(object.id)} 重新浮现`, ...place(object.id),
      detail: resurfacedLabel(reason), action: '看一眼上次到哪了，再决定接着做还是放下。', resurfaced: object.id, recap: input.recap(object.id) })
  }
  return items
}

/** Canvas marks by node key: object ids, plus the project card as `project-<id>`. Colour and label only, never position. */
export type RhythmMark = 'quiet' | 'resurfaced'
export function rhythmMarks(rhythm: Rhythm | null): Record<string, RhythmMark> {
  const marks: Record<string, RhythmMark> = {}
  rhythm?.objects.forEach((object, index) => {
    const value: RhythmMark | null = object.resurfaced ? 'resurfaced' : object.quiet ? 'quiet' : null
    if (!value) return
    marks[object.id] = value
    if (index === 0) marks[`project-${object.id}`] = value
  })
  return marks
}
export const rhythmLabels: Record<RhythmMark, { symbol: string; text: string }> = {
  quiet: { symbol: '◌', text: '休眠' }, resurfaced: { symbol: '↥', text: '浮现' },
}
